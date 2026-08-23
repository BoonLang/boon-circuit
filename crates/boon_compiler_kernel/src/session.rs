use crate::owner::KernelProjectSolveSession;
use crate::{
    KernelAbiInput, KernelCheckedSnapshot, KernelCompileWork, KernelDefinitionFactsInput,
    KernelDemandedDefinitionSnapshot, KernelInterfaceSnapshot, KernelOwnerBuildError,
    KernelOwnerId, KernelProjectProgramInput, KernelSolveError, KernelSolvedProject,
    PackedKernelProjectProgram,
};
use boon_contract::{
    PackedTextCatalogBuilder, ProjectTextSnapshot, QualifiedPathId, QualifiedSymbolId,
};
use boon_syntax::{SourceUnitId, StableCheckOwnerKey};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

/// One-shot construction owner for a kernel project and its text namespace.
///
/// Direct packed producers intern symbols and parent-linked paths while they
/// build normalized rows, then consume this builder together with the final
/// program/link/ABI inputs. The catalog cannot be detached or frozen through
/// this API, so every successful project owns exactly the authority that
/// issued its IDs.
#[derive(Debug)]
pub struct KernelProjectInputBuilder {
    text: PackedTextCatalogBuilder,
}

impl KernelProjectInputBuilder {
    pub fn new() -> Self {
        Self {
            text: PackedTextCatalogBuilder::new(),
        }
    }

    pub fn with_text_capacity(symbols: usize, symbol_bytes: usize, paths: usize) -> Self {
        Self {
            text: PackedTextCatalogBuilder::with_capacity(symbols, symbol_bytes, paths),
        }
    }

    pub fn intern_symbol(
        &mut self,
        value: &str,
    ) -> Result<QualifiedSymbolId, KernelOwnerBuildError> {
        self.text
            .intern_symbol(value)
            .map_err(|error| KernelOwnerBuildError::new(error.to_string()))
    }

    pub fn intern_path<'a>(
        &mut self,
        segments: impl IntoIterator<Item = &'a str>,
    ) -> Result<QualifiedPathId, KernelOwnerBuildError> {
        self.text
            .intern_path(segments)
            .map_err(|error| KernelOwnerBuildError::new(error.to_string()))
    }

    pub fn intern_symbol_path(
        &mut self,
        segments: impl IntoIterator<Item = QualifiedSymbolId>,
    ) -> Result<QualifiedPathId, KernelOwnerBuildError> {
        self.text
            .intern_symbol_path(segments)
            .map_err(|error| KernelOwnerBuildError::new(error.to_string()))
    }

    pub fn extend_path(
        &mut self,
        parent: QualifiedPathId,
        segment: QualifiedSymbolId,
    ) -> Result<QualifiedPathId, KernelOwnerBuildError> {
        self.text
            .extend_path(parent, segment)
            .map_err(|error| KernelOwnerBuildError::new(error.to_string()))
    }

    pub fn root_path(&self) -> QualifiedPathId {
        self.text.root_path()
    }

    /// Freezes the sole text catalog exactly once and binds the resulting
    /// authority to the final immutable project.
    pub fn finish(
        mut self,
        mut program: KernelProjectProgramInput,
        definition_facts: Box<[KernelDefinitionFactsInput]>,
        definition_keys: Box<[StableCheckOwnerKey]>,
        abi: KernelAbiInput,
    ) -> Result<KernelProjectInput, KernelOwnerBuildError> {
        if program.owners.len() != definition_facts.len() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel project input has {} owners but {} definition-fact tables",
                program.owners.len(),
                definition_facts.len()
            )));
        }
        if program.owners.len() != definition_keys.len() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel project input has {} owners but {} stable definition keys",
                program.owners.len(),
                definition_keys.len()
            )));
        }
        crate::text::populate_reserved_project_text(&mut self.text)?;
        let text = self.text.freeze();
        crate::text::validate_compatibility_project_text(
            &text,
            &program.owners,
            &definition_facts,
            &abi,
        )?;
        crate::validate_compatibility_project_input(&program, &definition_facts)?;
        let basis_fingerprints = Arc::from(crate::project_definition_basis_fingerprints(
            &program,
            &definition_facts,
        )?);
        let mut terms = crate::TypeTermArena::with_text(text.clone());
        crate::pack_project_closed_type_roots(&mut program, &mut terms)?;
        let packed_input_term_count = terms.len();
        let program = crate::pack_kernel_project_program(program, &terms)?;
        KernelProjectInput::from_explicit_text(
            program,
            definition_facts,
            definition_keys,
            abi,
            text,
            terms,
            packed_input_term_count,
            basis_fingerprints,
        )
    }

    /// Transitional adapter for rich producers. It is intentionally private:
    /// new callers must intern IDs during row construction instead of asking
    /// the kernel to rediscover text after the project is complete.
    fn from_rich_compatibility(
        program: &KernelProjectProgramInput,
        definition_facts: &[KernelDefinitionFactsInput],
        abi: &KernelAbiInput,
    ) -> Result<Self, KernelOwnerBuildError> {
        let mut builder = Self::new();
        crate::text::populate_compatibility_project_text(
            &mut builder.text,
            &program.owners,
            definition_facts,
            abi,
        )?;
        Ok(builder)
    }
}

impl Default for KernelProjectInputBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable, fully linked input for one kernel revision.
///
/// Parser arenas and legacy owner DTOs do not cross this boundary. The dense
/// owner programs are the normalized syntax product; their external owner IDs
/// and the definition-fact tables are the resolved project-link overlay.
#[derive(Debug)]
pub struct KernelProjectInput {
    syntax_units: Box<[KernelSyntaxUnitInput]>,
    links: KernelResolvedProjectLinkOverlay,
    program: Arc<PackedKernelProjectProgram>,
    /// Single immutable packed authority for every post-construction consumer.
    /// Rich compatibility facts live only in the one-shot construction state
    /// and are released after code, interfaces, and receipts have been sealed.
    runtime_facts: Arc<crate::PackedDefinitionFactsStore>,
    text: ProjectTextSnapshot,
    /// One-shot state moved into `KernelSession` before this input can be
    /// observed as revision metadata. Keeping the arena and V14 basis together
    /// prevents either construction authority from surviving as duplicate
    /// persistent state after preparation.
    construction: Option<KernelProjectConstruction>,
}

#[derive(Debug)]
struct KernelProjectConstruction {
    definition_facts: Arc<[KernelDefinitionFactsInput]>,
    /// Rich ABI input is consumed by graph compilation and compact ABI
    /// packing. It must not survive as a second authority on the immutable
    /// project once construction has been moved into a session.
    abi: Arc<KernelAbiInput>,
    terms: crate::TypeTermArena,
    packed_input_term_count: usize,
    basis_fingerprints_v14: Arc<[[u8; 32]]>,
}

/// One immutable normalized syntax unit. Definitions retain their stable
/// parser-owned identity while the dense IDs remain revision-local.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelSyntaxUnitInput {
    pub source_unit_id: SourceUnitId,
    pub definitions: Box<[KernelOwnerId]>,
}

/// Resolved stable-to-dense owner overlay for one project revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelResolvedProjectLinkOverlay {
    definitions: Box<[StableCheckOwnerKey]>,
    definition_by_key: BTreeMap<StableCheckOwnerKey, KernelOwnerId>,
}

impl KernelResolvedProjectLinkOverlay {
    pub fn definitions(&self) -> &[StableCheckOwnerKey] {
        &self.definitions
    }

    pub fn definition_id(&self, key: &StableCheckOwnerKey) -> Option<KernelOwnerId> {
        self.definition_by_key.get(key).copied()
    }

    pub fn definition_key(&self, owner: KernelOwnerId) -> Option<&StableCheckOwnerKey> {
        self.definitions.get(owner.0 as usize)
    }
}

impl KernelProjectInput {
    pub fn new(
        program: KernelProjectProgramInput,
        definition_facts: Box<[KernelDefinitionFactsInput]>,
        definition_keys: Box<[StableCheckOwnerKey]>,
    ) -> Result<Self, KernelOwnerBuildError> {
        Self::new_with_abi(
            program,
            definition_facts,
            definition_keys,
            KernelAbiInput::default(),
        )
    }

    pub fn new_with_abi(
        program: KernelProjectProgramInput,
        definition_facts: Box<[KernelDefinitionFactsInput]>,
        definition_keys: Box<[StableCheckOwnerKey]>,
        abi: KernelAbiInput,
    ) -> Result<Self, KernelOwnerBuildError> {
        let builder =
            KernelProjectInputBuilder::from_rich_compatibility(&program, &definition_facts, &abi)?;
        builder.finish(program, definition_facts, definition_keys, abi)
    }

    fn from_explicit_text(
        program: PackedKernelProjectProgram,
        definition_facts: Box<[KernelDefinitionFactsInput]>,
        definition_keys: Box<[StableCheckOwnerKey]>,
        abi: KernelAbiInput,
        text: ProjectTextSnapshot,
        terms: crate::TypeTermArena,
        packed_input_term_count: usize,
        basis_fingerprints_v14: Arc<[[u8; 32]]>,
    ) -> Result<Self, KernelOwnerBuildError> {
        if program.definition_count() != definition_facts.len() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel project input has {} owners but {} definition-fact tables",
                program.definition_count(),
                definition_facts.len()
            )));
        }
        if program.definition_count() != definition_keys.len() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel project input has {} owners but {} stable definition keys",
                program.definition_count(),
                definition_keys.len()
            )));
        }
        let mut definition_by_key = BTreeMap::new();
        let mut units = BTreeMap::<SourceUnitId, Vec<KernelOwnerId>>::new();
        for (index, key) in definition_keys.iter().enumerate() {
            let relocations = &definition_facts[index].relocations;
            if let Some(expression) = relocations
                .expressions
                .iter()
                .filter_map(|expression| match expression {
                    crate::KernelExpressionRelocation::Authored(expression) => Some(expression),
                    crate::KernelExpressionRelocation::SyntheticDefinitionResult => None,
                })
                .find(|expression| &expression.source_unit_id != key.source_unit_id())
            {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel definition {key:?} contains expression relocation from source unit {}",
                    expression.source_unit_id
                )));
            }
            if let Some(statement) = relocations
                .statements
                .iter()
                .find(|statement| &statement.source_unit_id != key.source_unit_id())
            {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel definition {key:?} contains statement relocation from source unit {}",
                    statement.source_unit_id
                )));
            }
            let owner = KernelOwnerId(
                u32::try_from(index)
                    .expect("kernel project definition count exceeds the dense u32 namespace"),
            );
            if definition_by_key.insert(key.clone(), owner).is_some() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel project input repeats stable definition key {key:?}"
                )));
            }
            units
                .entry(key.source_unit_id().clone())
                .or_default()
                .push(owner);
        }
        let syntax_units = units
            .into_iter()
            .map(|(source_unit_id, definitions)| KernelSyntaxUnitInput {
                source_unit_id,
                definitions: definitions.into_boxed_slice(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let definition_facts: Arc<[KernelDefinitionFactsInput]> = Arc::from(definition_facts);
        let runtime_facts = crate::pack_definition_facts_store(&text, definition_facts.as_ref())?;
        Ok(Self {
            syntax_units,
            links: KernelResolvedProjectLinkOverlay {
                definitions: definition_keys,
                definition_by_key,
            },
            program: Arc::new(program),
            runtime_facts,
            construction: Some(KernelProjectConstruction {
                definition_facts,
                abi: Arc::new(abi),
                terms,
                packed_input_term_count,
                basis_fingerprints_v14,
            }),
            text,
        })
    }

    pub fn definition_count(&self) -> usize {
        self.program.definition_count()
    }

    pub fn program(&self) -> &PackedKernelProjectProgram {
        self.program.as_ref()
    }

    pub fn call_count(&self) -> usize {
        self.program.call_count()
    }

    pub fn definition_result(&self, owner: KernelOwnerId) -> Option<crate::KernelExpressionId> {
        self.program.definition_result(owner)
    }

    pub fn syntax_units(&self) -> &[KernelSyntaxUnitInput] {
        &self.syntax_units
    }

    pub fn links(&self) -> &KernelResolvedProjectLinkOverlay {
        &self.links
    }

    pub fn expression_relocations(
        &self,
        owner: KernelOwnerId,
    ) -> Option<&[crate::KernelExpressionRelocation]> {
        Some(self.runtime_facts(owner)?.expression_relocations())
    }

    pub fn statement_relocations(
        &self,
        owner: KernelOwnerId,
    ) -> Option<&[boon_syntax::StableStatementKey]> {
        Some(self.runtime_facts(owner)?.statement_relocations())
    }

    pub fn diagnostic_values(&self, owner: KernelOwnerId) -> Option<&[crate::KernelExpressionId]> {
        Some(self.runtime_facts(owner)?.diagnostic_values())
    }

    pub fn statement_count(&self, owner: KernelOwnerId) -> Option<usize> {
        Some(self.runtime_facts(owner)?.statements().len())
    }

    pub fn statement_value(
        &self,
        owner: KernelOwnerId,
        statement: crate::KernelStatementId,
    ) -> Option<Option<crate::KernelExpressionId>> {
        self.runtime_facts(owner)?
            .statements()
            .get(statement.0 as usize)
            .filter(|row| row.id == statement)
            .map(|row| row.value)
    }

    pub fn render_slot_statement(
        &self,
        owner: KernelOwnerId,
        ordinal: usize,
    ) -> Option<(crate::KernelStatementId, crate::KernelExpressionId)> {
        self.runtime_facts(owner)?
            .statements()
            .iter()
            .filter_map(|row| {
                (row.value_use == crate::KernelStatementValueUse::RenderSlot)
                    .then_some(row.value)
                    .flatten()
                    .map(|value| (row.id, value))
            })
            .nth(ordinal)
    }

    pub fn parameter_name(&self, owner: KernelOwnerId, ordinal: u32) -> Option<&str> {
        let declaration = self
            .runtime_facts(owner)?
            .declarations()
            .iter()
            .find(|declaration| {
                matches!(
                    declaration.origin,
                    crate::KernelDeclarationOrigin::Parameter {
                        ordinal: candidate,
                        ..
                    } if candidate == ordinal
                )
            })?;
        self.program.owner(owner)?.symbol(declaration.name)
    }

    pub(crate) fn runtime_facts(
        &self,
        owner: KernelOwnerId,
    ) -> Option<crate::PackedDefinitionFactsRef<'_>> {
        self.runtime_facts.definition(owner)
    }

    pub fn text(&self) -> &ProjectTextSnapshot {
        &self.text
    }

    fn take_construction(&mut self) -> KernelProjectConstruction {
        self.construction
            .take()
            .expect("kernel project input construction authority is consumed exactly once")
    }

    fn compile_with_construction(
        &self,
        construction: KernelProjectConstruction,
    ) -> Result<crate::KernelProjectProgram, KernelOwnerBuildError> {
        let KernelProjectConstruction {
            definition_facts,
            abi,
            terms,
            packed_input_term_count,
            basis_fingerprints_v14,
        } = construction;
        crate::compile_project_program_with_definition_facts_abi_text_and_terms(
            Arc::clone(&self.program),
            definition_facts,
            Arc::clone(&self.runtime_facts),
            abi,
            self.text.clone(),
            terms,
            packed_input_term_count,
            basis_fingerprints_v14,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KernelRevisionId(pub u64);

/// Product boundary for one kernel check.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CheckDemand {
    /// Solve public interfaces required for diagnostics, but do not construct
    /// definition artifacts or currentness receipts.
    Diagnostics,
    /// Publish the complete checked image and exact currentness metadata.
    CheckedImage,
    /// Publish only these stable authored definitions. The session resolves
    /// them through the current revision's dense link overlay.
    Definitions(Box<[StableCheckOwnerKey]>),
}

impl CheckDemand {
    fn canonicalize(self) -> Result<Self, KernelCheckError> {
        let Self::Definitions(definitions) = self else {
            return Ok(self);
        };
        let mut definitions = definitions.into_vec();
        definitions.sort_unstable();
        definitions.dedup();
        if definitions.is_empty() {
            return Err(KernelCheckError::invalid_demand(
                "kernel demanded-definition request is empty",
            ));
        }
        Ok(Self::Definitions(definitions.into_boxed_slice()))
    }
}

#[derive(Clone, Debug)]
pub struct KernelDemandedCheckSnapshot {
    owners: Box<[KernelOwnerId]>,
    project: Arc<KernelProjectInput>,
    definition_code: Arc<crate::DefinitionCodeStore>,
    interface: Arc<KernelInterfaceSnapshot>,
    work: crate::KernelSolveWork,
}

impl PartialEq for KernelDemandedCheckSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.owners == other.owners
            && Arc::ptr_eq(&self.project, &other.project)
            && self.definition_code == other.definition_code
            && self.interface == other.interface
            && self.work == other.work
    }
}

impl Eq for KernelDemandedCheckSnapshot {}

#[derive(Clone, Copy)]
pub struct KernelDemandedCheckDefinitionRef<'a> {
    owner: &'a StableCheckOwnerKey,
    dense_owner: KernelOwnerId,
    definition: crate::KernelDefinitionRef<'a>,
}

impl<'a> KernelDemandedCheckDefinitionRef<'a> {
    pub fn owner(&self) -> &StableCheckOwnerKey {
        self.owner
    }

    pub const fn dense_owner(self) -> KernelOwnerId {
        self.dense_owner
    }

    pub const fn definition(self) -> crate::KernelDefinitionRef<'a> {
        self.definition
    }
}

impl KernelDemandedCheckSnapshot {
    pub fn definition_count(&self) -> usize {
        self.owners.len()
    }

    pub fn dense_owners(&self) -> &[KernelOwnerId] {
        &self.owners
    }

    pub fn definition_refs(
        &self,
    ) -> impl ExactSizeIterator<Item = KernelDemandedCheckDefinitionRef<'_>> + '_ {
        self.owners.iter().copied().map(|dense_owner| {
            let owner = self
                .project
                .links()
                .definition_key(dense_owner)
                .expect("validated demanded owner retains its stable key");
            let definition = crate::KernelDefinitionRef::from_authorities(
                self.project.program(),
                &self.definition_code,
                dense_owner,
            )
            .expect("validated demanded owner remains in every shared authority");
            KernelDemandedCheckDefinitionRef {
                owner,
                dense_owner,
                definition,
            }
        })
    }

    pub fn project(&self) -> &Arc<KernelProjectInput> {
        &self.project
    }

    pub fn definition_code(&self) -> &Arc<crate::DefinitionCodeStore> {
        &self.definition_code
    }

    pub fn type_store(&self) -> &Arc<crate::FrozenTypeStore> {
        self.definition_code.type_store()
    }

    pub fn interface(&self) -> &Arc<KernelInterfaceSnapshot> {
        &self.interface
    }

    pub const fn work(&self) -> crate::KernelSolveWork {
        self.work
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelCheckProduct {
    Diagnostics(Arc<KernelInterfaceSnapshot>),
    CheckedImage(Arc<KernelCheckedSnapshot>),
    Definitions(Arc<KernelDemandedCheckSnapshot>),
}

impl KernelCheckProduct {
    /// Number of definition views published by this demand.
    ///
    /// These views borrow the shared project and packed-code authorities; the
    /// count deliberately says nothing about compatibility DTO materialization.
    pub fn published_definition_count(&self) -> usize {
        match self {
            Self::Diagnostics(_) => 0,
            Self::CheckedImage(snapshot) => snapshot.definition_count(),
            Self::Definitions(snapshot) => snapshot.definition_count(),
        }
    }

    pub fn sealed_definition_count(&self) -> usize {
        match self {
            Self::CheckedImage(snapshot) => snapshot.currentness.len(),
            Self::Diagnostics(_) | Self::Definitions(_) => 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckResult {
    pub revision: KernelRevisionId,
    pub product: KernelCheckProduct,
    pub compile_work: KernelCompileWork,
    /// True when this product came from the current revision cache, the
    /// retained quiescent graph, or a stronger cached checked image without
    /// another compile/solve.
    pub reused: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelCheckError {
    Build(KernelOwnerBuildError),
    Solve(KernelSolveError),
    InvalidDemand(Box<str>),
}

impl KernelCheckError {
    fn invalid_demand(message: impl Into<Box<str>>) -> Self {
        Self::InvalidDemand(message.into())
    }
}

impl fmt::Display for KernelCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build(error) => error.fmt(formatter),
            Self::Solve(error) => error.fmt(formatter),
            Self::InvalidDemand(message) => formatter.write_str(message),
        }
    }
}

impl Error for KernelCheckError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Build(error) => Some(error),
            Self::Solve(error) => Some(error),
            Self::InvalidDemand(_) => None,
        }
    }
}

impl From<KernelOwnerBuildError> for KernelCheckError {
    fn from(error: KernelOwnerBuildError) -> Self {
        Self::Build(error)
    }
}

impl From<KernelSolveError> for KernelCheckError {
    fn from(error: KernelSolveError) -> Self {
        Self::Solve(error)
    }
}

#[derive(Clone)]
struct CachedKernelCheck {
    product: KernelCheckProduct,
    compile_work: KernelCompileWork,
}

struct CachedSolvedProject {
    project: KernelSolvedProject,
    compile_work: KernelCompileWork,
}

/// Revision owner for the permanent dense kernel API.
///
/// The first cut caches completed products within one immutable revision. A
/// future incremental tranche will retain solver/type arenas across
/// `replace_project`; this API deliberately establishes that ownership before
/// persistent red/green reuse is implemented.
pub struct KernelSession {
    revision: KernelRevisionId,
    project: Arc<KernelProjectInput>,
    pending: Option<KernelProjectConstruction>,
    prepared: Option<KernelProjectSolveSession>,
    solved: Option<CachedSolvedProject>,
    failed: Option<KernelCheckError>,
    checks: BTreeMap<CheckDemand, CachedKernelCheck>,
}

impl KernelSession {
    pub fn new(mut project: KernelProjectInput) -> Self {
        let pending = Some(project.take_construction());
        Self {
            revision: KernelRevisionId(1),
            project: Arc::new(project),
            pending,
            prepared: None,
            solved: None,
            failed: None,
            checks: BTreeMap::new(),
        }
    }

    pub const fn revision(&self) -> KernelRevisionId {
        self.revision
    }

    pub fn project(&self) -> &KernelProjectInput {
        &self.project
    }

    pub fn replace_project(&mut self, mut project: KernelProjectInput) -> KernelRevisionId {
        self.revision = KernelRevisionId(
            self.revision
                .0
                .checked_add(1)
                .expect("kernel session revision counter exhausted"),
        );
        self.pending = Some(project.take_construction());
        self.project = Arc::new(project);
        self.prepared = None;
        self.solved = None;
        self.failed = None;
        self.checks.clear();
        self.revision
    }

    /// Compile the current revision into one retained equation graph without
    /// solving it. The session remains the sole owner of the packed input and
    /// compiled type authority, so profiling cannot split them into an
    /// independently ownable tuple.
    pub fn prepare(&mut self) -> Result<KernelCompileWork, KernelCheckError> {
        if let Some(cached) = self.checks.values().next() {
            return Ok(cached.compile_work);
        }
        self.ensure_prepared()?;
        Ok(self
            .prepared
            .as_ref()
            .map(KernelProjectSolveSession::compile_work)
            .or_else(|| self.solved.as_ref().map(|solved| solved.compile_work))
            .or_else(|| {
                self.checks
                    .values()
                    .next()
                    .map(|cached| cached.compile_work)
            })
            .expect("a prepared kernel revision retains compile work"))
    }

    /// Bring the retained equation graph to quiescence without publishing a
    /// checked image. This is primarily useful to measure graph solving apart
    /// from optional checked-image construction.
    pub fn solve_graph(&mut self) -> Result<KernelCompileWork, KernelCheckError> {
        if let Some(cached) = self.checks.get(&CheckDemand::CheckedImage) {
            return Ok(cached.compile_work);
        }
        self.ensure_solved()?;
        Ok(self
            .solved
            .as_ref()
            .expect("a solved kernel revision retains its graph")
            .compile_work)
    }

    pub fn check(&mut self, demand: CheckDemand) -> Result<KernelCheckResult, KernelCheckError> {
        let demand = demand.canonicalize()?;
        self.validate_demand(&demand)?;
        if let Some(cached) = self.checks.get(&demand) {
            return Ok(KernelCheckResult {
                revision: self.revision,
                product: cached.product.clone(),
                compile_work: cached.compile_work,
                reused: true,
            });
        }
        if let Some(cached) = self.project_from_checked_image(&demand)? {
            self.checks.insert(demand, cached.clone());
            return Ok(KernelCheckResult {
                revision: self.revision,
                product: cached.product,
                compile_work: cached.compile_work,
                reused: true,
            });
        }

        let reused_solve = self.prepared.is_some() || self.solved.is_some();
        let (product, compile_work) = match &demand {
            CheckDemand::Diagnostics => {
                if let Some(solved) = self.solved.as_ref() {
                    (
                        KernelCheckProduct::Diagnostics(solved.project.interface_snapshot()),
                        solved.compile_work,
                    )
                } else {
                    self.ensure_prepared()?;
                    let (compile_work, interfaces) = {
                        let prepared = self
                            .prepared
                            .as_mut()
                            .expect("kernel diagnostics own a prepared graph");
                        (
                            prepared.compile_work(),
                            prepared.solve_interfaces().map_err(KernelCheckError::from),
                        )
                    };
                    match interfaces {
                        Ok(interfaces) => (
                            KernelCheckProduct::Diagnostics(Arc::new(interfaces)),
                            compile_work,
                        ),
                        Err(error) => {
                            // Interface solving mutates the retained graph. A
                            // failed solve is terminal for this immutable
                            // revision: replay the original error instead of
                            // exposing a partially advanced graph to a later
                            // demand.
                            self.prepared = None;
                            self.failed = Some(error.clone());
                            return Err(error);
                        }
                    }
                }
            }
            CheckDemand::CheckedImage => {
                self.ensure_solved()?;
                let solved = self
                    .solved
                    .take()
                    .expect("kernel checked image owns a solved graph");
                let checked = match solved.project.into_checked_snapshot() {
                    Ok(checked) => checked,
                    Err(error) => {
                        let error = KernelCheckError::from(error);
                        self.failed = Some(error.clone());
                        return Err(error);
                    }
                };
                (
                    KernelCheckProduct::CheckedImage(Arc::new(checked)),
                    solved.compile_work,
                )
            }
            CheckDemand::Definitions(definitions) => {
                self.ensure_solved()?;
                let dense = definitions
                    .iter()
                    .map(|definition| {
                        self.project.links().definition_id(definition).ok_or_else(|| {
                            KernelCheckError::invalid_demand(format!(
                                "kernel definition demand references missing owner {definition:?}"
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let demanded = self
                    .solved
                    .as_ref()
                    .expect("kernel definition demand owns a solved graph")
                    .project
                    .demanded_definitions(&dense)?;
                let compile_work = self
                    .solved
                    .as_ref()
                    .expect("kernel definition demand retains its solved graph")
                    .compile_work;
                (
                    KernelCheckProduct::Definitions(Arc::new(
                        self.attach_stable_definition_keys(demanded, dense.into_boxed_slice())?,
                    )),
                    compile_work,
                )
            }
        };
        let cached = CachedKernelCheck {
            product: product.clone(),
            compile_work,
        };
        self.checks.insert(demand, cached);
        Ok(KernelCheckResult {
            revision: self.revision,
            product,
            compile_work,
            reused: reused_solve,
        })
    }

    fn ensure_prepared(&mut self) -> Result<(), KernelCheckError> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if self.prepared.is_none() && self.solved.is_none() {
            let construction = self.pending.take().ok_or_else(|| {
                KernelCheckError::invalid_demand(
                    "kernel revision construction authority was consumed without a prepared solve",
                )
            })?;
            let prepared = self
                .project
                .compile_with_construction(construction)
                .map_err(KernelCheckError::from)
                .and_then(|program| program.into_solve_session().map_err(KernelCheckError::from));
            match prepared {
                Ok(prepared) => self.prepared = Some(prepared),
                Err(error) => {
                    self.failed = Some(error.clone());
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn ensure_solved(&mut self) -> Result<(), KernelCheckError> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if self.solved.is_some() {
            return Ok(());
        }
        self.ensure_prepared()?;
        let prepared = self
            .prepared
            .take()
            .expect("kernel full solve owns a prepared graph");
        let compile_work = prepared.compile_work();
        match prepared.finish_graph() {
            Ok(project) => {
                self.solved = Some(CachedSolvedProject {
                    project,
                    compile_work,
                });
            }
            Err(error) => {
                let error = KernelCheckError::from(error);
                self.failed = Some(error.clone());
                return Err(error);
            }
        }
        Ok(())
    }

    fn validate_demand(&self, demand: &CheckDemand) -> Result<(), KernelCheckError> {
        let CheckDemand::Definitions(definitions) = demand else {
            return Ok(());
        };
        if let Some(owner) = definitions
            .iter()
            .find(|owner| self.project.links().definition_id(owner).is_none())
        {
            return Err(KernelCheckError::invalid_demand(format!(
                "kernel definition demand references missing owner {owner:?}"
            )));
        }
        Ok(())
    }

    fn project_from_checked_image(
        &self,
        demand: &CheckDemand,
    ) -> Result<Option<CachedKernelCheck>, KernelCheckError> {
        let Some(checked) = self.checks.get(&CheckDemand::CheckedImage) else {
            return Ok(None);
        };
        let KernelCheckProduct::CheckedImage(snapshot) = &checked.product else {
            unreachable!("checked-image cache key owns a checked-image product")
        };
        let product = match demand {
            CheckDemand::CheckedImage => return Ok(None),
            CheckDemand::Diagnostics => {
                KernelCheckProduct::Diagnostics(Arc::clone(&snapshot.interface))
            }
            CheckDemand::Definitions(definitions) => {
                let owners = definitions
                    .iter()
                    .map(|owner| {
                        self.project
                            .links()
                            .definition_id(owner)
                            .expect("definition demand was validated")
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                KernelCheckProduct::Definitions(Arc::new(KernelDemandedCheckSnapshot {
                    owners,
                    project: Arc::clone(&self.project),
                    definition_code: Arc::clone(&snapshot.definition_code),
                    interface: Arc::clone(&snapshot.interface),
                    work: snapshot.work,
                }))
            }
        };
        Ok(Some(CachedKernelCheck {
            product,
            compile_work: checked.compile_work,
        }))
    }

    fn attach_stable_definition_keys(
        &self,
        demanded: KernelDemandedDefinitionSnapshot,
        owners: Box<[KernelOwnerId]>,
    ) -> Result<KernelDemandedCheckSnapshot, KernelCheckError> {
        let (dense_selection, definition_code, interface, work) = demanded.into_selected_parts();
        if owners.len() != dense_selection.len()
            || owners
                .iter()
                .any(|owner| dense_selection.binary_search(owner).is_err())
        {
            return Err(KernelCheckError::invalid_demand(
                "kernel stable and dense definition selections disagree",
            ));
        }
        Ok(KernelDemandedCheckSnapshot {
            owners,
            project: Arc::clone(&self.project),
            definition_code,
            interface,
            work,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        KernelDiagnosticKind, KernelDiagnosticSite, KernelExternalExpression, KernelExternalTarget,
        KernelOwnerEdgeRole, KernelOwnerInputEdge, KernelOwnerNode, KernelOwnerNodeKind,
        KernelOwnerProgramInput, KernelPureBuiltinKind, KernelTypeMismatch,
    };
    use boon_checked::{FlowMode, Type};

    fn value_owner(kind: KernelOwnerNodeKind) -> KernelOwnerProgramInput {
        KernelOwnerProgramInput {
            nodes: Box::new([KernelOwnerNode {
                kind,
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: crate::KernelExpressionId(0),
        }
    }

    fn external_owner(provider: u32) -> KernelOwnerProgramInput {
        KernelOwnerProgramInput {
            nodes: Box::new([KernelOwnerNode {
                kind: KernelOwnerNodeKind::ValueRead {
                    fields: Box::new([]),
                    mode_narrowing: None,
                },
                inputs: Box::new([KernelOwnerInputEdge {
                    role: KernelOwnerEdgeRole::ReadProvider,
                    expression: crate::KernelExpressionId(1),
                }]),
                mode: FlowMode::Continuous,
            }]),
            formal_count: 0,
            external_expressions: Box::new([KernelExternalExpression {
                owner: KernelOwnerId(provider),
                target: KernelExternalTarget::Result,
            }]),
            result: crate::KernelExpressionId(0),
        }
    }

    fn diagnostic_project() -> KernelProjectInput {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::PureBuiltin {
                        kind: KernelPureBuiltinKind::TextLength,
                    },
                    inputs: Box::new([KernelOwnerInputEdge {
                        role: KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        expression: crate::KernelExpressionId(0),
                    }]),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: crate::KernelExpressionId(1),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: Box::new([KernelOwnerInputEdge {
                        role: KernelOwnerEdgeRole::CallArgument { ordinal: 0 },
                        expression: crate::KernelExpressionId(0),
                    }]),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: crate::KernelExpressionId(1),
        };
        KernelProjectInput::new(
            KernelProjectProgramInput {
                owners: vec![callee, caller].into_boxed_slice(),
            },
            vec![KernelDefinitionFactsInput::default(); 2].into_boxed_slice(),
            (0..2)
                .map(|index| {
                    StableCheckOwnerKey::UnitRoot(
                        SourceUnitId::from_path(&format!("diagnostic-owner-{index}.bn"))
                            .expect("fixture source path is canonical"),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .expect("diagnostic fixture has aligned definition facts")
    }

    fn project(first: KernelOwnerNodeKind) -> KernelProjectInput {
        let program = KernelProjectProgramInput {
            owners: vec![value_owner(first), external_owner(0), external_owner(1)]
                .into_boxed_slice(),
        };
        KernelProjectInput::new(
            program,
            vec![KernelDefinitionFactsInput::default(); 3].into_boxed_slice(),
            (0..3)
                .map(|index| {
                    StableCheckOwnerKey::UnitRoot(
                        SourceUnitId::from_path(&format!("session-owner-{index}.bn"))
                            .expect("fixture source path is canonical"),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .expect("session fixture has aligned definition facts")
    }

    #[test]
    fn project_input_builder_rejects_foreign_text_ids() {
        let mut left = KernelProjectInputBuilder::new();
        let foreign = left.intern_symbol("foreign").unwrap();
        let mut right = KernelProjectInputBuilder::new();

        let error = right
            .intern_symbol_path([foreign])
            .expect_err("a path cannot adopt a symbol from another project authority");
        assert!(error.to_string().contains("different project authority"));
    }

    #[test]
    fn explicit_project_text_must_cover_the_final_rows() {
        let error = KernelProjectInputBuilder::new()
            .finish(
                KernelProjectProgramInput {
                    owners: vec![value_owner(KernelOwnerNodeKind::Tag("Missing".into()))]
                        .into_boxed_slice(),
                },
                vec![KernelDefinitionFactsInput::default()].into_boxed_slice(),
                vec![StableCheckOwnerKey::UnitRoot(
                    SourceUnitId::from_path("missing-text.bn").unwrap(),
                )]
                .into_boxed_slice(),
                KernelAbiInput::default(),
            )
            .expect_err("an explicit catalog missing row text must fail closed");
        assert!(error.to_string().contains("missing symbol `Missing`"));
    }

    #[test]
    fn project_text_coordinates_are_deterministic_for_the_same_build_order() {
        fn build() -> (
            KernelProjectInput,
            boon_contract::SymbolId,
            boon_contract::PathId,
        ) {
            let mut builder = KernelProjectInputBuilder::new();
            let alpha = builder.intern_symbol("alpha").unwrap();
            let beta = builder.intern_symbol("beta").unwrap();
            let path = builder.intern_symbol_path([alpha, beta]).unwrap();
            let alpha = alpha.coordinate();
            let path = path.coordinate();
            let project = builder
                .finish(
                    KernelProjectProgramInput {
                        owners: Box::new([]),
                    },
                    Box::new([]),
                    Box::new([]),
                    KernelAbiInput::default(),
                )
                .unwrap();
            (project, alpha, path)
        }

        let (left, left_alpha, left_path) = build();
        let (right, right_alpha, right_path) = build();
        assert_eq!(left_alpha, right_alpha);
        assert_eq!(left_path, right_path);
        assert_eq!(left.text().symbol(left_alpha), Some("alpha"));
        assert_eq!(right.text().symbol(right_alpha), Some("alpha"));
        assert_eq!(
            left.text().path_digest(left_path),
            right.text().path_digest(right_path)
        );
        assert!(!left.text().same_authority(right.text()));
    }

    #[test]
    fn project_input_owns_stable_syntax_units_and_the_dense_link_overlay() {
        let project = project(KernelOwnerNodeKind::Number);
        assert_eq!(project.syntax_units().len(), 3);
        assert!(
            project
                .syntax_units()
                .iter()
                .all(|unit| unit.definitions.len() == 1)
        );
        for (index, key) in project.links().definitions().iter().enumerate() {
            let owner = KernelOwnerId(index as u32);
            assert_eq!(project.links().definition_id(key), Some(owner));
            assert_eq!(project.links().definition_key(owner), Some(key));
            assert_eq!(
                key.source_unit_id(),
                &project.syntax_units()[index].source_unit_id
            );
        }
    }

    #[test]
    fn project_input_consumes_rich_closed_roots_into_one_type_authority() {
        let closed = Type::List(Type::shared(Type::Number));
        let program = KernelProjectProgramInput {
            owners: vec![
                value_owner(KernelOwnerNodeKind::Known(closed.clone())),
                value_owner(KernelOwnerNodeKind::Source(closed.clone())),
                value_owner(KernelOwnerNodeKind::FixedAbiCall {
                    result: closed.clone(),
                }),
                value_owner(KernelOwnerNodeKind::Known(closed)),
            ]
            .into_boxed_slice(),
        };
        let facts = vec![KernelDefinitionFactsInput::default(); 4].into_boxed_slice();
        let expected_basis = crate::project_definition_basis_fingerprints(&program, &facts)
            .expect("rich fixture has a stable V14 basis");
        let project = KernelProjectInput::new(
            program,
            facts,
            (0..4)
                .map(|index| {
                    StableCheckOwnerKey::UnitRoot(
                        SourceUnitId::from_path(&format!("packed-root-{index}.bn")).unwrap(),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .expect("closed roots pack during one-shot project construction");

        let construction = project
            .construction
            .as_ref()
            .expect("unprepared input owns one construction authority");
        assert_eq!(
            construction.basis_fingerprints_v14.as_ref(),
            expected_basis.as_ref()
        );
        let refs = project
            .program()
            .owners()
            .map(|owner| match owner.nodes()[0].kind {
                crate::PackedKernelOwnerNodeKind::Known(reference)
                | crate::PackedKernelOwnerNodeKind::Source(reference)
                | crate::PackedKernelOwnerNodeKind::FixedAbiCall { result: reference } => reference,
                packed => panic!("project retained unexpected packed root {packed:?}"),
            })
            .collect::<Vec<_>>();
        assert!(refs.windows(2).all(|pair| pair[0] == pair[1]));
        let terms = &construction.terms;
        assert!(
            refs.iter()
                .all(|reference| terms.resolve_type_ref(*reference).is_some())
        );
        assert_eq!(construction.packed_input_term_count, terms.len());
    }

    #[test]
    fn project_builder_rejects_internal_packed_rows_before_v14_hashing() {
        let text =
            crate::text::compatibility_project_text_snapshot(&[], &[], &KernelAbiInput::default())
                .unwrap();
        let foreign = crate::TypeTermArena::with_text(text);
        let reference = foreign.type_ref(foreign.number());
        let error = KernelProjectInputBuilder::new()
            .finish(
                KernelProjectProgramInput {
                    owners: vec![value_owner(KernelOwnerNodeKind::KnownPacked(reference))]
                        .into_boxed_slice(),
                },
                vec![KernelDefinitionFactsInput::default()].into_boxed_slice(),
                vec![StableCheckOwnerKey::UnitRoot(
                    SourceUnitId::from_path("premature-packed.bn").unwrap(),
                )]
                .into_boxed_slice(),
                KernelAbiInput::default(),
            )
            .expect_err("process-local packed coordinates cannot enter the stable V14 hash");
        assert!(error.to_string().contains("before the stable V14 basis"));
    }

    #[test]
    fn packed_closed_roots_reject_foreign_type_authorities() {
        let text =
            crate::text::compatibility_project_text_snapshot(&[], &[], &KernelAbiInput::default())
                .unwrap();
        let foreign = crate::TypeTermArena::with_text(text.clone());
        let reference = foreign.type_ref(foreign.number());
        let mut target = crate::TypeTermArena::with_text(text);
        let mut program = KernelProjectProgramInput {
            owners: vec![value_owner(KernelOwnerNodeKind::KnownPacked(reference))]
                .into_boxed_slice(),
        };
        let error = crate::pack_project_closed_type_roots(&mut program, &mut target)
            .expect_err("an in-range term from another arena must fail closed");
        assert!(error.to_string().contains("another project authority"));
    }

    #[test]
    fn packed_closed_roots_reject_same_authority_solver_variables() {
        let text =
            crate::text::compatibility_project_text_snapshot(&[], &[], &KernelAbiInput::default())
                .unwrap();
        let mut terms = crate::TypeTermArena::with_text(text);
        let variable = terms.variable(crate::TypeVariableId(7));
        let reference = terms.type_ref(variable);
        let mut program = KernelProjectProgramInput {
            owners: vec![value_owner(KernelOwnerNodeKind::KnownPacked(reference))]
                .into_boxed_slice(),
        };

        let error = crate::pack_project_closed_type_roots(&mut program, &mut terms)
            .expect_err("a packed project-input root with variables must fail closed");
        assert!(error.to_string().contains("solver variables"));
    }

    #[test]
    fn failed_preparation_replays_the_original_error_after_consuming_construction() {
        let mut project = KernelProjectInput::new(
            KernelProjectProgramInput {
                owners: vec![value_owner(KernelOwnerNodeKind::Number)].into_boxed_slice(),
            },
            vec![KernelDefinitionFactsInput::default()].into_boxed_slice(),
            vec![StableCheckOwnerKey::UnitRoot(
                SourceUnitId::from_path("invalid-result.bn").unwrap(),
            )]
            .into_boxed_slice(),
        )
        .expect("the test starts from one valid packed project");
        Arc::get_mut(&mut project.program)
            .expect("the unshared test project owns its packed program")
            .corrupt_result_for_test(0, crate::KernelExpressionId(1))
            .expect("the test project retains owner zero");
        let mut session = KernelSession::new(project);

        let first = session
            .check(CheckDemand::Diagnostics)
            .expect_err("the out-of-range result must fail preparation");
        let second = session
            .check(CheckDemand::Diagnostics)
            .expect_err("a repeated check must replay the same terminal failure");
        assert_eq!(second, first);
        assert!(
            !second
                .to_string()
                .contains("construction authority was consumed")
        );
    }

    #[test]
    fn project_input_rejects_cross_unit_definition_relocations() {
        let definition_unit = SourceUnitId::from_path("definition.bn").unwrap();
        let foreign_unit = SourceUnitId::from_path("foreign.bn").unwrap();
        let statement = boon_syntax::StableStatementKey {
            source_unit_id: definition_unit.clone(),
            route: boon_syntax::StableStatementRoute {
                owner: None,
                statement_route: Vec::new(),
            },
        };
        let error = KernelProjectInput::new(
            KernelProjectProgramInput {
                owners: vec![value_owner(KernelOwnerNodeKind::Number)].into_boxed_slice(),
            },
            vec![KernelDefinitionFactsInput {
                linkage: crate::KernelDefinitionLinkage {
                    root_statement: Some(crate::KernelStatementId(0)),
                    public_declaration: Some(crate::KernelDeclarationReference::Local(
                        crate::KernelDeclarationId(0),
                    )),
                    result_expression: Some(crate::KernelExpressionId(0)),
                    context_formal_ordinal: None,
                },
                relocations: crate::KernelDefinitionRelocations {
                    expressions: vec![crate::KernelExpressionRelocation::Authored(
                        boon_syntax::StableExpressionKey {
                            source_unit_id: foreign_unit,
                            route_digest_v1: [7; 32],
                        },
                    )]
                    .into_boxed_slice(),
                    statements: vec![statement].into_boxed_slice(),
                },
                statements: vec![crate::KernelStatementInput {
                    id: crate::KernelStatementId(0),
                    kind: crate::KernelStatementKind::Field {
                        name: "value".into(),
                    },
                    value: Some(crate::KernelExpressionId(0)),
                    value_use: crate::KernelStatementValueUse::RuntimeValue,
                    children: Box::new([]),
                }]
                .into_boxed_slice(),
                declarations: vec![crate::KernelDeclarationInput {
                    id: crate::KernelDeclarationId(0),
                    origin: crate::KernelDeclarationOrigin::Statement {
                        statement: crate::KernelStatementId(0),
                    },
                    name: "value".into(),
                    kind: crate::KernelDeclarationKind::Field,
                    value: Some(crate::KernelExpressionId(0)),
                    declared_flow_type: None,
                }]
                .into_boxed_slice(),
                presentation: crate::KernelDefinitionPresentation {
                    containing_scope: crate::KernelScopeReference::ProjectRoot,
                    scopes: Box::new([]),
                    expressions: vec![crate::KernelExpressionPresentation {
                        expression: crate::KernelExpressionId(0),
                        scope: crate::KernelScopeReference::ProjectRoot,
                        declaration: Some(crate::KernelDeclarationReference::Local(
                            crate::KernelDeclarationId(0),
                        )),
                        declaration_scope: None,
                        span: crate::KernelSourceSpan::default(),
                    }]
                    .into_boxed_slice(),
                    statements: vec![crate::KernelStatementPresentation {
                        statement: crate::KernelStatementId(0),
                        scope: crate::KernelScopeReference::ProjectRoot,
                        body_scope: None,
                        span: crate::KernelSourceSpan::default(),
                    }]
                    .into_boxed_slice(),
                    declarations: vec![crate::KernelDeclarationPresentation {
                        declaration: crate::KernelDeclarationId(0),
                        scope: crate::KernelScopeReference::ProjectRoot,
                        body_scope: None,
                        span: crate::KernelSourceSpan::default(),
                    }]
                    .into_boxed_slice(),
                },
                ..KernelDefinitionFactsInput::default()
            }]
            .into_boxed_slice(),
            vec![StableCheckOwnerKey::UnitRoot(definition_unit)].into_boxed_slice(),
        )
        .expect_err("cross-unit relocation must fail closed");
        assert!(
            error
                .to_string()
                .contains("expression relocation from source unit"),
            "unexpected cross-unit relocation error: {error}"
        );
    }

    #[test]
    fn diagnostics_stop_before_definition_materialization_and_receipt_sealing() {
        let mut session = KernelSession::new(project(KernelOwnerNodeKind::Number));
        let result = session
            .check(CheckDemand::Diagnostics)
            .expect("diagnostics demand solves");
        let KernelCheckProduct::Diagnostics(snapshot) = &result.product else {
            panic!("diagnostics demand returned another product")
        };
        assert_eq!(snapshot.definition_count(), 3);
        assert!((0..snapshot.definition_count()).all(|owner| {
            snapshot
                .materialize_formal_flows(KernelOwnerId(owner as u32))
                .is_some_and(|formals| formals.is_empty())
        }));
        assert_eq!(
            snapshot
                .materialize_result_flow(KernelOwnerId(2))
                .unwrap()
                .ty,
            Type::Number
        );
        assert_eq!(snapshot.diagnostic_count(), 0);
        assert_eq!(result.product.published_definition_count(), 0);
        assert_eq!(result.product.sealed_definition_count(), 0);
        assert!(!result.reused);
        assert!(
            session.prepared.is_some() && session.solved.is_none(),
            "diagnostics must retain a staged equation graph without forcing every body"
        );

        let checked = session
            .check(CheckDemand::CheckedImage)
            .expect("checked image extends the quiescent diagnostics graph");
        assert!(
            checked.reused,
            "the shared type graph must not compile twice"
        );
        assert!(
            session.solved.is_none(),
            "a complete checked image must replace the pre-publication graph"
        );
        let KernelCheckProduct::CheckedImage(checked_snapshot) = &checked.product else {
            panic!("checked-image demand returned another product")
        };
        assert_eq!(
            snapshot.materialize_public_results().collect::<Vec<_>>(),
            checked_snapshot
                .interface
                .materialize_public_results()
                .collect::<Vec<_>>(),
            "diagnostics and checked-image demands must share one public interface authority"
        );
        for owner in 0..snapshot.definition_count() {
            let owner = KernelOwnerId(owner as u32);
            assert_eq!(
                snapshot.materialize_formal_flows(owner),
                checked_snapshot.interface.materialize_formal_flows(owner),
                "diagnostics and checked-image demands must share callable formal authorities"
            );
        }
        assert_eq!(
            session.prepare().unwrap(),
            checked.compile_work,
            "preparation profiling must reuse a published checked image"
        );
        assert_eq!(
            session.solve_graph().unwrap(),
            checked.compile_work,
            "graph profiling must reuse a published checked image"
        );

        let repeated = session
            .check(CheckDemand::Diagnostics)
            .expect("same-revision diagnostics reuse");
        assert!(repeated.reused);
        assert_eq!(repeated.product, result.product);
    }

    #[test]
    fn diagnostics_demand_publishes_typed_failures_and_reuses_them_in_checked_images() {
        let mut session = KernelSession::new(diagnostic_project());
        let result = session
            .check(CheckDemand::Diagnostics)
            .expect("typed diagnostics demand solves");
        let KernelCheckProduct::Diagnostics(diagnostics) = &result.product else {
            panic!("diagnostics demand returned another product")
        };
        let materialized_diagnostics = diagnostics.materialize_diagnostics();
        let [diagnostic] = materialized_diagnostics.as_ref() else {
            panic!("diagnostics demand must publish one call failure")
        };
        assert_eq!(
            diagnostic.site,
            KernelDiagnosticSite::CallInput {
                call: crate::KernelExpressionId(1),
                target: KernelOwnerId(0),
                formal_ordinal: 0,
            }
        );
        assert!(matches!(
            diagnostic.kind,
            KernelDiagnosticKind::CallInputType {
                actual: Type::Number,
                expected: Type::Text,
                mismatch: KernelTypeMismatch::Type,
            }
        ));
        assert_eq!(result.product.published_definition_count(), 0);
        assert_eq!(result.product.sealed_definition_count(), 0);

        let checked = session
            .check(CheckDemand::CheckedImage)
            .expect("checked image reuses diagnostic solve");
        assert!(checked.reused);
        let KernelCheckProduct::CheckedImage(checked) = checked.product else {
            panic!("checked demand returned another product")
        };
        assert_eq!(
            checked.interface.materialize_diagnostics(),
            materialized_diagnostics,
            "one graph evaluation owns both diagnostics-only and checked-image facts"
        );
    }

    #[test]
    fn demanded_definitions_publish_only_the_canonical_requested_set() {
        let mut session = KernelSession::new(project(KernelOwnerNodeKind::Text));
        let first = session.project().links().definitions()[1].clone();
        let second = session.project().links().definitions()[2].clone();
        crate::owner::reset_test_compatibility_definition_materializations();
        let result = session
            .check(CheckDemand::Definitions(
                vec![second.clone(), first.clone(), second].into_boxed_slice(),
            ))
            .expect("definition demand solves");
        let KernelCheckProduct::Definitions(snapshot) = &result.product else {
            panic!("definition demand returned another product")
        };
        let definitions = snapshot.definition_refs().collect::<Vec<_>>();
        assert_eq!(
            definitions
                .iter()
                .map(|definition| definition.dense_owner())
                .collect::<Vec<_>>(),
            vec![KernelOwnerId(1), KernelOwnerId(2)]
        );
        assert_eq!(definitions[0].owner(), &first);
        assert!(definitions.iter().all(|definition| {
            snapshot
                .interface()
                .materialize_result_flow(definition.dense_owner())
                .is_some_and(|result| result.ty == Type::Text)
        }));
        assert_eq!(result.product.published_definition_count(), 2);
        assert_eq!(result.product.sealed_definition_count(), 0);
        assert_eq!(
            crate::owner::test_compatibility_definition_materializations(),
            0,
            "sparse demand must select borrowed rows without compatibility artifacts",
        );
        assert!(Arc::ptr_eq(snapshot.project(), &session.project));
    }

    #[test]
    fn checked_image_satisfies_weaker_demands_without_another_solve() {
        let mut session = KernelSession::new(project(KernelOwnerNodeKind::Number));
        let checked = session
            .check(CheckDemand::CheckedImage)
            .expect("checked-image demand solves");
        assert_eq!(checked.product.published_definition_count(), 3);
        assert_eq!(checked.product.sealed_definition_count(), 3);
        assert!(!checked.reused);
        let KernelCheckProduct::CheckedImage(checked_snapshot) = &checked.product else {
            panic!("checked demand returned another product")
        };
        assert!(Arc::ptr_eq(
            checked_snapshot.definition_code.runtime_facts_store(),
            &session.project.runtime_facts,
        ));
        assert!(Arc::ptr_eq(
            &checked_snapshot.program,
            &session.project.program,
        ));

        let demanded = session.project().links().definitions()[1].clone();
        let sparse = session
            .check(CheckDemand::Definitions(Box::new([demanded.clone()])))
            .expect("checked image satisfies sparse definition demand");
        assert!(sparse.reused);
        assert_eq!(sparse.product.published_definition_count(), 1);
        let KernelCheckProduct::Definitions(sparse_snapshot) = &sparse.product else {
            panic!("sparse demand returned another product")
        };
        assert!(Arc::ptr_eq(sparse_snapshot.project(), &session.project));
        let sparse_definition = sparse_snapshot
            .definition_refs()
            .next()
            .expect("one sparse definition remains selected");
        assert_eq!(sparse_definition.owner(), &demanded);
        assert_eq!(sparse_definition.dense_owner(), KernelOwnerId(1));

        let diagnostics = session
            .check(CheckDemand::Diagnostics)
            .expect("checked image satisfies diagnostics demand");
        assert!(diagnostics.reused);
        assert_eq!(diagnostics.product.published_definition_count(), 0);
    }

    #[test]
    fn replacing_the_project_advances_revision_and_clears_products() {
        let mut session = KernelSession::new(project(KernelOwnerNodeKind::Number));
        let first = session
            .check(CheckDemand::Diagnostics)
            .expect("first revision solves");
        assert_eq!(first.revision, KernelRevisionId(1));

        assert_eq!(
            session.replace_project(project(KernelOwnerNodeKind::Text)),
            KernelRevisionId(2)
        );
        let second = session
            .check(CheckDemand::Diagnostics)
            .expect("replacement revision solves");
        assert_eq!(second.revision, KernelRevisionId(2));
        assert!(!second.reused);
        let KernelCheckProduct::Diagnostics(snapshot) = second.product else {
            panic!("replacement diagnostics returned another product")
        };
        assert_eq!(
            snapshot
                .materialize_result_flow(KernelOwnerId(2))
                .unwrap()
                .ty,
            Type::Text
        );
    }
}
