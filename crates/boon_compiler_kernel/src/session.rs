use crate::{
    KernelAbiInput, KernelCheckedSnapshot, KernelCompileWork, KernelDefinitionFactsInput,
    KernelDemandedDefinitionSnapshot, KernelInterfaceSnapshot, KernelOwnerBuildError,
    KernelOwnerId, KernelProjectProgramInput, KernelSolveError, KernelSolvedProject,
    compile_project_program_with_definition_facts,
};
use boon_syntax::{SourceUnitId, StableCheckOwnerKey};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

/// Immutable, fully linked input for one kernel revision.
///
/// Parser arenas and legacy owner DTOs do not cross this boundary. The dense
/// owner programs are the normalized syntax product; their external owner IDs
/// and the definition-fact tables are the resolved project-link overlay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelProjectInput {
    syntax_units: Box<[KernelSyntaxUnitInput]>,
    links: KernelResolvedProjectLinkOverlay,
    program: KernelProjectProgramInput,
    definition_facts: Box<[KernelDefinitionFactsInput]>,
    abi: KernelAbiInput,
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
        Ok(Self {
            syntax_units,
            links: KernelResolvedProjectLinkOverlay {
                definitions: definition_keys,
                definition_by_key,
            },
            program,
            definition_facts,
            abi,
        })
    }

    pub fn definition_count(&self) -> usize {
        self.program.owners.len()
    }

    pub fn program(&self) -> &KernelProjectProgramInput {
        &self.program
    }

    pub fn syntax_units(&self) -> &[KernelSyntaxUnitInput] {
        &self.syntax_units
    }

    pub fn links(&self) -> &KernelResolvedProjectLinkOverlay {
        &self.links
    }

    pub fn definition_facts(&self) -> &[KernelDefinitionFactsInput] {
        &self.definition_facts
    }

    pub const fn abi(&self) -> &KernelAbiInput {
        &self.abi
    }

    pub fn compile(&self) -> Result<crate::KernelProjectProgram, KernelOwnerBuildError> {
        compile_project_program_with_definition_facts(self.program(), self.definition_facts())
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelDemandedCheckArtifact {
    pub owner: StableCheckOwnerKey,
    pub dense_owner: KernelOwnerId,
    pub definition: crate::DefinitionArtifact,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelDemandedCheckSnapshot {
    pub definitions: Box<[KernelDemandedCheckArtifact]>,
    pub work: crate::KernelSolveWork,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelCheckProduct {
    Diagnostics(Arc<KernelInterfaceSnapshot>),
    CheckedImage(Arc<KernelCheckedSnapshot>),
    Definitions(Arc<KernelDemandedCheckSnapshot>),
}

impl KernelCheckProduct {
    pub fn materialized_definition_count(&self) -> usize {
        match self {
            Self::Diagnostics(_) => 0,
            Self::CheckedImage(snapshot) => snapshot.definitions.len(),
            Self::Definitions(snapshot) => snapshot.definitions.len(),
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
    solved: Option<CachedSolvedProject>,
    checks: BTreeMap<CheckDemand, CachedKernelCheck>,
}

impl KernelSession {
    pub fn new(project: KernelProjectInput) -> Self {
        Self {
            revision: KernelRevisionId(1),
            project: Arc::new(project),
            solved: None,
            checks: BTreeMap::new(),
        }
    }

    pub const fn revision(&self) -> KernelRevisionId {
        self.revision
    }

    pub fn project(&self) -> &KernelProjectInput {
        &self.project
    }

    pub fn replace_project(&mut self, project: KernelProjectInput) -> KernelRevisionId {
        self.revision = KernelRevisionId(
            self.revision
                .0
                .checked_add(1)
                .expect("kernel session revision counter exhausted"),
        );
        self.project = Arc::new(project);
        self.solved = None;
        self.checks.clear();
        self.revision
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

        let reused_solve = self.solved.is_some();
        if !reused_solve {
            let program = self.project.compile()?;
            let compile_work = program.compile_work();
            self.solved = Some(CachedSolvedProject {
                project: program.solve_graph()?,
                compile_work,
            });
        }
        let compile_work = self
            .solved
            .as_ref()
            .expect("kernel session installed its solved graph")
            .compile_work;
        let product = match &demand {
            CheckDemand::Diagnostics => KernelCheckProduct::Diagnostics(Arc::new(
                self.solved
                    .as_ref()
                    .expect("kernel diagnostics own a solved graph")
                    .project
                    .interface_snapshot(),
            )),
            CheckDemand::CheckedImage => KernelCheckProduct::CheckedImage(Arc::new(
                self.solved
                    .take()
                    .expect("kernel checked image owns a solved graph")
                    .project
                    .into_checked_snapshot()?,
            )),
            CheckDemand::Definitions(definitions) => {
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
                KernelCheckProduct::Definitions(Arc::new(
                    self.attach_stable_definition_keys(demanded)?,
                ))
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
                let public_results = snapshot
                    .definitions
                    .iter()
                    .map(|definition| definition.result.clone())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                let callable_formals = snapshot
                    .definitions
                    .iter()
                    .map(|definition| definition.formals.clone())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                let diagnostics = snapshot
                    .definitions
                    .iter()
                    .flat_map(|definition| definition.diagnostics.iter().cloned())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                KernelCheckProduct::Diagnostics(Arc::new(KernelInterfaceSnapshot {
                    public_results,
                    callable_formals,
                    diagnostics,
                    diagnostic_values: snapshot.diagnostic_values.clone(),
                    work: snapshot.work,
                }))
            }
            CheckDemand::Definitions(definitions) => {
                let definitions = definitions
                    .iter()
                    .map(|owner| {
                        let dense_owner = self
                            .project
                            .links()
                            .definition_id(owner)
                            .expect("definition demand was validated");
                        KernelDemandedCheckArtifact {
                            owner: owner.clone(),
                            dense_owner,
                            definition: snapshot.definitions[dense_owner.0 as usize].clone(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                KernelCheckProduct::Definitions(Arc::new(KernelDemandedCheckSnapshot {
                    definitions,
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
    ) -> Result<KernelDemandedCheckSnapshot, KernelCheckError> {
        let mut definitions = demanded
            .definitions
            .into_vec()
            .into_iter()
            .map(|definition| {
                let owner = self
                    .project
                    .links()
                    .definition_key(definition.owner)
                    .cloned()
                    .ok_or_else(|| {
                        KernelCheckError::invalid_demand(format!(
                            "kernel demanded artifact references missing dense owner {}",
                            definition.owner.0
                        ))
                    })?;
                Ok(KernelDemandedCheckArtifact {
                    owner,
                    dense_owner: definition.owner,
                    definition: definition.definition,
                })
            })
            .collect::<Result<Vec<_>, KernelCheckError>>()?;
        definitions.sort_by(|left, right| left.owner.cmp(&right.owner));
        Ok(KernelDemandedCheckSnapshot {
            definitions: definitions.into_boxed_slice(),
            work: demanded.work,
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
    fn project_input_rejects_cross_unit_definition_relocations() {
        let definition_unit = SourceUnitId::from_path("definition.bn").unwrap();
        let foreign_unit = SourceUnitId::from_path("foreign.bn").unwrap();
        let error = KernelProjectInput::new(
            KernelProjectProgramInput {
                owners: vec![value_owner(KernelOwnerNodeKind::Number)].into_boxed_slice(),
            },
            vec![KernelDefinitionFactsInput {
                relocations: crate::KernelDefinitionRelocations {
                    expressions: vec![crate::KernelExpressionRelocation::Authored(
                        boon_syntax::StableExpressionKey {
                            source_unit_id: foreign_unit,
                            route_digest_v1: [7; 32],
                        },
                    )]
                    .into_boxed_slice(),
                    statements: Box::new([]),
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
                .contains("expression relocation from source unit")
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
        assert_eq!(snapshot.public_results.len(), 3);
        assert_eq!(snapshot.callable_formals.len(), 3);
        assert!(
            snapshot
                .callable_formals
                .iter()
                .all(|formals| formals.is_empty())
        );
        assert_eq!(snapshot.public_results[2].ty, Type::Number);
        assert!(snapshot.diagnostics.is_empty());
        assert_eq!(result.product.materialized_definition_count(), 0);
        assert_eq!(result.product.sealed_definition_count(), 0);
        assert!(!result.reused);
        assert!(session.solved.is_some());

        let checked = session
            .check(CheckDemand::CheckedImage)
            .expect("checked image reuses the quiescent diagnostics graph");
        assert!(checked.reused, "the shared type graph must not solve twice");
        assert!(
            session.solved.is_none(),
            "a complete checked image must replace the pre-publication graph"
        );
        let KernelCheckProduct::CheckedImage(checked_snapshot) = &checked.product else {
            panic!("checked-image demand returned another product")
        };
        assert_eq!(
            snapshot.public_results.as_ref(),
            checked_snapshot
                .definitions
                .iter()
                .map(|definition| definition.result.clone())
                .collect::<Vec<_>>()
                .as_slice(),
            "diagnostics and checked-image demands must share one public interface authority"
        );
        assert_eq!(
            snapshot.callable_formals.as_ref(),
            checked_snapshot
                .definitions
                .iter()
                .map(|definition| definition.formals.clone())
                .collect::<Vec<_>>()
                .as_slice(),
            "diagnostics and checked-image demands must share callable formal authorities"
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
        let [diagnostic] = diagnostics.diagnostics.as_ref() else {
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
        assert_eq!(result.product.materialized_definition_count(), 0);
        assert_eq!(result.product.sealed_definition_count(), 0);

        let checked = session
            .check(CheckDemand::CheckedImage)
            .expect("checked image reuses diagnostic solve");
        assert!(checked.reused);
        let KernelCheckProduct::CheckedImage(checked) = checked.product else {
            panic!("checked demand returned another product")
        };
        assert_eq!(
            checked.definitions[1].diagnostics.as_ref(),
            diagnostics.diagnostics.as_ref(),
            "one graph evaluation owns both diagnostics-only and checked-image facts"
        );
    }

    #[test]
    fn demanded_definitions_materialize_only_the_canonical_requested_set() {
        let mut session = KernelSession::new(project(KernelOwnerNodeKind::Text));
        let first = session.project().links().definitions()[1].clone();
        let second = session.project().links().definitions()[2].clone();
        let result = session
            .check(CheckDemand::Definitions(
                vec![second.clone(), first.clone(), second].into_boxed_slice(),
            ))
            .expect("definition demand solves");
        let KernelCheckProduct::Definitions(snapshot) = &result.product else {
            panic!("definition demand returned another product")
        };
        assert_eq!(
            snapshot
                .definitions
                .iter()
                .map(|definition| definition.dense_owner)
                .collect::<Vec<_>>(),
            vec![KernelOwnerId(1), KernelOwnerId(2)]
        );
        assert_eq!(snapshot.definitions[0].owner, first);
        assert!(
            snapshot
                .definitions
                .iter()
                .all(|definition| definition.definition.result.ty == Type::Text)
        );
        assert_eq!(result.product.materialized_definition_count(), 2);
        assert_eq!(result.product.sealed_definition_count(), 0);
    }

    #[test]
    fn checked_image_satisfies_weaker_demands_without_another_solve() {
        let mut session = KernelSession::new(project(KernelOwnerNodeKind::Number));
        let checked = session
            .check(CheckDemand::CheckedImage)
            .expect("checked-image demand solves");
        assert_eq!(checked.product.materialized_definition_count(), 3);
        assert_eq!(checked.product.sealed_definition_count(), 3);
        assert!(!checked.reused);

        let demanded = session.project().links().definitions()[1].clone();
        let sparse = session
            .check(CheckDemand::Definitions(Box::new([demanded.clone()])))
            .expect("checked image satisfies sparse definition demand");
        assert!(sparse.reused);
        assert_eq!(sparse.product.materialized_definition_count(), 1);
        let KernelCheckProduct::Definitions(sparse_snapshot) = &sparse.product else {
            panic!("sparse demand returned another product")
        };
        assert_eq!(sparse_snapshot.definitions[0].owner, demanded);
        assert_eq!(sparse_snapshot.definitions[0].dense_owner, KernelOwnerId(1));

        let diagnostics = session
            .check(CheckDemand::Diagnostics)
            .expect("checked image satisfies diagnostics demand");
        assert!(diagnostics.reused);
        assert_eq!(diagnostics.product.materialized_definition_count(), 0);
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
        assert_eq!(snapshot.public_results[2].ty, Type::Text);
    }
}
