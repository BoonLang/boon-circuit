#[cfg(test)]
use crate::DefinitionArtifact;
use crate::{
    BytesTerm, DefinitionCodeRef, DefinitionCodeStore, KernelCallInputRoleRef, KernelCallTarget,
    KernelCallTargetRef, KernelDeclarationReference, KernelDefinitionFactsInput,
    KernelDefinitionRef, KernelDiagnosticArtifact, KernelDiagnosticKind, KernelExpressionId,
    KernelExpressionKindRef, KernelExternalExpression, KernelExternalTarget,
    KernelInterfaceSnapshot, KernelLexicalBindingTarget, KernelLexicalBindingTargetRef,
    KernelOwnerBuildError, KernelOwnerId, KernelOwnerNodeKind, KernelOwnerProgramInput,
    KernelProjectProgramInput, KernelSolveError, KernelStatePathRef, KernelStatementChildReference,
    KernelStatementReference, KernelValueReference, PackedDiagnosticMetadata,
    PackedDiagnosticTypes, PackedFlow, RichDefinitionArtifact, TypeTerm, TypeTermArena, TypeTermId,
    TypeVariableId, VariantTerm,
};
use boon_checked::{FlowType, ObjectShape, SharedObjectShape, Type, TypeVar, Variant};
use boon_effect_schema::{
    BarrierSpec, DeliveryCardinalityPolicySpec, ReplaySpec, ResultPolicySpec, host_effect_policy,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::ops::Range;

const KERNEL_DEFINITION_BASIS_DOMAIN_V14: &[u8] = b"boon.compiler-kernel.definition-basis.v14\0";
const KERNEL_PUBLIC_RESULT_DOMAIN_V1: &[u8] = b"boon.compiler-kernel.public-result.v1\0";
const KERNEL_EXPRESSION_SURFACE_DOMAIN_V1: &[u8] = b"boon.compiler-kernel.expression-surface.v1\0";
const KERNEL_DEFINITION_ARTIFACT_DOMAIN_V16: &[u8] =
    b"boon.compiler-kernel.definition-artifact.v16\0";
const KERNEL_DEPENDENCY_IMPORTS_DOMAIN_V2: &[u8] = b"boon.compiler-kernel.dependency-imports.v2\0";
const KERNEL_PACKED_DEPENDENCY_IMPORTS_DOMAIN_V3: &[u8] =
    b"boon.compiler-kernel.dependency-imports.v3.packed-resource-facts\0";
const KERNEL_DEFINITION_CURRENTNESS_DOMAIN_V16: &[u8] =
    b"boon.compiler-kernel.definition-currentness.v16\0";
const KERNEL_DEFINITION_ARTIFACT_DOMAIN_V17: &[u8] =
    b"boon.compiler-kernel.definition-artifact.v17.packed\0";
const KERNEL_EXPRESSION_SURFACE_DOMAIN_V2: &[u8] =
    b"boon.compiler-kernel.expression-surface.v2.packed\0";
const KERNEL_DEFINITION_CURRENTNESS_DOMAIN_V18: &[u8] =
    b"boon.compiler-kernel.definition-currentness.v18.packed-resource-facts\0";
const PARALLEL_DEFINITION_THRESHOLD: usize = 64;

struct DefinitionFingerprints {
    public_result: [u8; 32],
    artifact: [u8; 32],
    expressions: BTreeMap<KernelExpressionId, [u8; 32]>,
}

/// Exact definition-local origin of one dependency edge.
///
/// These rows are intentionally structural rather than diagnostic. They make
/// invalidation and retained-result currentness independent of source-tree
/// rediscovery and preserve multiple distinct uses of one provider.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelDependencySource {
    ExpressionInput {
        expression: KernelExpressionId,
        input: u32,
    },
    StatementValue {
        statement: crate::KernelStatementId,
    },
    StatementChild {
        statement: crate::KernelStatementId,
        child: u32,
    },
    DeclarationValue {
        declaration: crate::KernelDeclarationId,
    },
    LexicalDeclaration {
        expression: KernelExpressionId,
    },
    LexicalValue {
        expression: KernelExpressionId,
    },
    CallTarget {
        expression: KernelExpressionId,
    },
    CallInput {
        expression: KernelExpressionId,
        input: u32,
    },
    SourceDeclaration {
        source: crate::KernelSourceId,
    },
    SourceStatement {
        source: crate::KernelSourceId,
    },
    SourcePathAnchor {
        source: crate::KernelSourceId,
    },
    StateBindingDeclaration {
        state: crate::KernelStateId,
    },
    StateDeclaration {
        state: crate::KernelStateId,
    },
    StateStatement {
        state: crate::KernelStateId,
    },
    StateInitial {
        state: crate::KernelStateId,
    },
    StatePathAnchor {
        state: crate::KernelStateId,
    },
    ListDeclaration {
        list: crate::KernelListId,
    },
    ListStatement {
        list: crate::KernelListId,
    },
    ListPathAnchor {
        list: crate::KernelListId,
    },
    ResourceProjectionTarget {
        expression: KernelExpressionId,
    },
    ResourceProjectionOrigin {
        expression: KernelExpressionId,
        origin: u32,
    },
}

/// Exact authority imported from another dense definition.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelDependencyTarget {
    Definition(KernelOwnerId),
    PublicDeclaration(KernelOwnerId),
    Declaration {
        owner: KernelOwnerId,
        declaration: crate::KernelDeclarationId,
    },
    PublicStatement(KernelOwnerId),
    Expression {
        owner: KernelOwnerId,
        expression: KernelExpressionId,
    },
    Source {
        owner: KernelOwnerId,
        source: crate::KernelSourceId,
    },
    Result(KernelOwnerId),
}

impl KernelDependencyTarget {
    pub const fn owner(self) -> KernelOwnerId {
        match self {
            Self::Definition(owner)
            | Self::PublicDeclaration(owner)
            | Self::PublicStatement(owner)
            | Self::Expression { owner, .. }
            | Self::Source { owner, .. }
            | Self::Result(owner) => owner,
            Self::Declaration { owner, .. } => owner,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KernelDefinitionDependency {
    pub source: KernelDependencySource,
    pub target: KernelDependencyTarget,
}

/// Definition dependency rows plus a reverse-consumer CSR index.
///
/// `dependencies` preserves exact use sites. `consumers` is deduplicated by
/// definition so one provider mutation schedules each dependent definition at
/// most once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelDefinitionDependencyGraph {
    dependency_offsets: Box<[u32]>,
    dependencies: Box<[KernelDefinitionDependency]>,
    consumer_offsets: Box<[u32]>,
    consumers: Box<[KernelOwnerId]>,
}

impl KernelDefinitionDependencyGraph {
    pub fn definition_count(&self) -> usize {
        self.dependency_offsets.len().saturating_sub(1)
    }

    pub fn dependency_count(&self) -> usize {
        self.dependencies.len()
    }

    pub fn reverse_consumer_count(&self) -> usize {
        self.consumers.len()
    }

    pub fn dependencies(&self, definition: KernelOwnerId) -> Option<&[KernelDefinitionDependency]> {
        let index = definition.0 as usize;
        let start = *self.dependency_offsets.get(index)? as usize;
        let end = *self.dependency_offsets.get(index + 1)? as usize;
        self.dependencies.get(start..end)
    }

    pub fn consumers(&self, provider: KernelOwnerId) -> Option<&[KernelOwnerId]> {
        let index = provider.0 as usize;
        let start = *self.consumer_offsets.get(index)? as usize;
        let end = *self.consumer_offsets.get(index + 1)? as usize;
        self.consumers.get(start..end)
    }

    /// Sorted transitive reverse dependency cone, excluding `provider`.
    pub fn dependent_cone(&self, provider: KernelOwnerId) -> Option<Box<[KernelOwnerId]>> {
        self.consumers(provider)?;
        let mut seen = vec![false; self.definition_count()];
        seen[provider.0 as usize] = true;
        let mut pending = VecDeque::from([provider]);
        let mut cone = Vec::new();
        while let Some(current) = pending.pop_front() {
            for consumer in self
                .consumers(current)
                .expect("dependency graph consumer index is internally complete")
            {
                let index = consumer.0 as usize;
                if !seen[index] {
                    seen[index] = true;
                    cone.push(*consumer);
                    pending.push_back(*consumer);
                }
            }
        }
        cone.sort_unstable();
        Some(cone.into_boxed_slice())
    }
}

/// Separates a reusable semantic artifact from proof that this exact basis and
/// exact imported authority set produced it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelDefinitionCurrentnessReceipt {
    pub basis_fingerprint_v14: [u8; 32],
    pub public_result_fingerprint_v1: [u8; 32],
    pub artifact_fingerprint_v16: [u8; 32],
    pub dependency_fingerprint_v2: [u8; 32],
    pub fingerprint_v16: [u8; 32],
}

/// Exact currentness receipt for the structural definition plus its sealed
/// packed type code. The version differs deliberately from the retired rich
/// V16 hash domain; no field silently changes meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelPackedDefinitionCurrentnessReceipt {
    pub basis_fingerprint_v14: [u8; 32],
    pub public_result_fingerprint_v1: [u8; 32],
    pub artifact_fingerprint_v17: [u8; 32],
    pub dependency_fingerprint_v3: [u8; 32],
    pub fingerprint_v18: [u8; 32],
}

pub(crate) fn definition_basis_fingerprint(
    input: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
) -> Result<[u8; 32], KernelOwnerBuildError> {
    definition_basis_fingerprint_with_buffer(input, facts, &mut Vec::new())
}

pub(crate) fn definition_basis_fingerprint_with_buffer(
    input: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
    scratch: &mut Vec<u8>,
) -> Result<[u8; 32], KernelOwnerBuildError> {
    Ok(stable_fingerprint(
        KERNEL_DEFINITION_BASIS_DOMAIN_V14,
        &(input, facts),
        scratch,
    ))
}

pub(crate) fn build_snapshot_receipts(
    definitions: &mut [RichDefinitionArtifact],
    basis_fingerprints: &[[u8; 32]],
) -> Result<
    (
        KernelDefinitionDependencyGraph,
        Box<[KernelDefinitionCurrentnessReceipt]>,
    ),
    KernelSolveError,
> {
    for definition in definitions.iter_mut() {
        alpha_normalize_definition(definition);
    }
    build_normalized_snapshot_receipts(definitions, basis_fingerprints)
}

/// Seal a project whose definition artifacts have already been normalized in
/// their independent materialization workers.
pub(crate) fn build_normalized_snapshot_receipts(
    definitions: &[RichDefinitionArtifact],
    basis_fingerprints: &[[u8; 32]],
) -> Result<
    (
        KernelDefinitionDependencyGraph,
        Box<[KernelDefinitionCurrentnessReceipt]>,
    ),
    KernelSolveError,
> {
    if definitions.len() != basis_fingerprints.len() {
        return Err(KernelSolveError::new(format!(
            "kernel snapshot has {} definitions but {} basis fingerprints",
            definitions.len(),
            basis_fingerprints.len()
        )));
    }
    let dependency_graph = build_dependency_graph(definitions)?;
    let mut imported_expressions = vec![BTreeSet::new(); definitions.len()];
    for dependency in dependency_graph.dependencies.iter() {
        if let KernelDependencyTarget::Expression { owner, expression } = dependency.target {
            imported_expressions[owner.0 as usize].insert(expression);
        }
    }
    let fingerprints = collect_definition_ranges(definitions.len(), |range| {
        fingerprint_definition_range(range, definitions, &imported_expressions)
    })?;
    let receipts = collect_definition_ranges(definitions.len(), |range| {
        currentness_receipt_range(range, &dependency_graph, basis_fingerprints, &fingerprints)
    })?;
    Ok((dependency_graph, receipts.into_boxed_slice()))
}

#[cfg(test)]
pub(crate) fn build_packed_snapshot_receipts(
    definitions: &[DefinitionArtifact],
    definition_facts: &[KernelDefinitionFactsInput],
    code: &DefinitionCodeStore,
    interface: &KernelInterfaceSnapshot,
    basis_fingerprints: &[[u8; 32]],
) -> Result<
    (
        KernelDefinitionDependencyGraph,
        Box<[KernelPackedDefinitionCurrentnessReceipt]>,
    ),
    KernelSolveError,
> {
    if definitions.len() != definition_facts.len()
        || definitions.len() != basis_fingerprints.len()
        || definitions.len() != code.definition_count()
        || definitions.len() != interface.definition_count()
    {
        return Err(KernelSolveError::new(format!(
            "kernel packed snapshot has {} definitions, {} fact rows, {} code rows, {} interface rows, and {} basis fingerprints",
            definitions.len(),
            definition_facts.len(),
            code.definition_count(),
            interface.definition_count(),
            basis_fingerprints.len(),
        )));
    }
    validate_packed_interface_diagnostic_layout(code, interface)?;
    let dependency_graph = build_packed_dependency_graph(definitions, code, interface)?;
    let mut imported_expressions = vec![BTreeSet::new(); definitions.len()];
    for dependency in dependency_graph.dependencies.iter() {
        if let KernelDependencyTarget::Expression { owner, expression } = dependency.target {
            imported_expressions[owner.0 as usize].insert(expression);
        }
    }
    let fingerprints = collect_definition_ranges(definitions.len(), |range| {
        fingerprint_packed_definition_range(
            range,
            definitions,
            definition_facts,
            code,
            interface,
            &imported_expressions,
        )
    })?;
    let receipts = collect_definition_ranges(definitions.len(), |range| {
        packed_currentness_receipt_range(
            range,
            &dependency_graph,
            basis_fingerprints,
            &fingerprints,
        )
    })?;
    Ok((dependency_graph, receipts.into_boxed_slice()))
}

/// Seal receipts directly from the three immutable project authorities.
///
/// Unlike [`build_packed_snapshot_receipts`], this path never constructs the
/// owned compatibility `DefinitionArtifact` graph. The retained packed path is
/// kept temporarily as a differential oracle until every downstream consumer
/// has moved to the same borrowed definition view.
pub(crate) fn build_borrowed_snapshot_receipts(
    program: &KernelProjectProgramInput,
    definition_facts: &[KernelDefinitionFactsInput],
    code: &DefinitionCodeStore,
    interface: &KernelInterfaceSnapshot,
) -> Result<
    (
        KernelDefinitionDependencyGraph,
        Box<[KernelPackedDefinitionCurrentnessReceipt]>,
    ),
    KernelSolveError,
> {
    let definition_count = program.owners.len();
    if definition_count != definition_facts.len()
        || definition_count != code.definition_count()
        || definition_count != interface.definition_count()
    {
        return Err(KernelSolveError::new(format!(
            "kernel borrowed snapshot has {definition_count} program rows, {} fact rows, {} code rows, and {} interface rows",
            definition_facts.len(),
            code.definition_count(),
            interface.definition_count(),
        )));
    }
    validate_packed_interface_diagnostic_layout(code, interface)?;
    let dependency_graph =
        build_borrowed_dependency_graph(program, definition_facts, code, interface)?;
    let mut imported_expressions = vec![BTreeSet::new(); definition_count];
    for dependency in dependency_graph.dependencies.iter() {
        if let KernelDependencyTarget::Expression { owner, expression } = dependency.target {
            imported_expressions[owner.0 as usize].insert(expression);
        }
    }
    let fingerprints = collect_definition_ranges(definition_count, |range| {
        fingerprint_borrowed_definition_range(
            range,
            program,
            definition_facts,
            code,
            interface,
            &imported_expressions,
        )
    })?;
    let basis_fingerprints = code
        .definitions()
        .map(DefinitionCodeRef::basis_fingerprint_v14)
        .collect::<Vec<_>>();
    let receipts = collect_definition_ranges(definition_count, |range| {
        packed_currentness_receipt_range(
            range,
            &dependency_graph,
            &basis_fingerprints,
            &fingerprints,
        )
    })?;
    Ok((dependency_graph, receipts.into_boxed_slice()))
}

fn validate_packed_interface_diagnostic_layout(
    code: &DefinitionCodeStore,
    interface: &KernelInterfaceSnapshot,
) -> Result<(), KernelSolveError> {
    if interface.diagnostics.len() != interface.diagnostic_types.len() {
        return Err(KernelSolveError::new(format!(
            "kernel interface has {} diagnostic metadata rows but {} diagnostic type rows",
            interface.diagnostics.len(),
            interface.diagnostic_types.len()
        )));
    }
    let mut cursor = 0usize;
    for owner in 0..code.definition_count() {
        let owner_id = KernelOwnerId(
            u32::try_from(owner).expect("kernel definition count exceeds dense u32 namespace"),
        );
        code.definition(owner_id).ok_or_else(|| {
            KernelSolveError::new(format!("kernel definition code omits owner {owner}"))
        })?;
        let interface_definition = interface.definitions.get(owner).ok_or_else(|| {
            KernelSolveError::new(format!("kernel interface omits owner {owner}"))
        })?;
        let range = interface_definition.diagnostics.range();
        if range.start != cursor {
            return Err(KernelSolveError::new(format!(
                "kernel definition {owner} diagnostic span {range:?} does not continue at {cursor}",
            )));
        }
        let rows = interface.diagnostics.get(range.clone()).ok_or_else(|| {
            KernelSolveError::new(format!(
                "kernel definition {owner} diagnostic code exceeds its interface rows"
            ))
        })?;
        if rows.iter().any(|diagnostic| diagnostic.owner() != owner_id) {
            return Err(KernelSolveError::new(format!(
                "kernel definition {owner} diagnostic rows are not owner-contiguous"
            )));
        }
        for (metadata, types) in rows.iter().zip(&interface.diagnostic_types[range.clone()]) {
            let aligned = matches!(metadata, PackedDiagnosticMetadata::CallInputType { .. })
                == types.is_some();
            let plain_call_input = matches!(
                metadata,
                PackedDiagnosticMetadata::Plain(KernelDiagnosticArtifact {
                    kind: KernelDiagnosticKind::CallInputType { .. },
                    ..
                })
            );
            if !aligned || plain_call_input {
                return Err(KernelSolveError::new(format!(
                    "kernel definition {owner} diagnostic metadata and packed type rows differ"
                )));
            }
        }
        cursor = range.end;
    }
    if cursor != interface.diagnostics.len() {
        return Err(KernelSolveError::new(format!(
            "kernel definition code covers {cursor} of {} interface diagnostics",
            interface.diagnostics.len()
        )));
    }
    Ok(())
}

/// Reusable alpha-renaming state for direct hashes of packed checked types.
///
/// Public-result V1 starts from an empty scope and assigns variables in the
/// projected checked traversal order. Definition-artifact V17 starts from the
/// callable interface's already-established alpha scope so diagnostic types
/// receive the same ordinals as the former rich compatibility projection.
#[derive(Default)]
struct PackedAlphaHashScratch {
    variables: RefCell<Vec<(TypeVariableId, u32)>>,
    exact_scope: Cell<bool>,
}

impl PackedAlphaHashScratch {
    fn begin_fresh(&self) {
        self.variables.borrow_mut().clear();
        self.exact_scope.set(false);
    }

    fn begin_exact(&self, sources: &[TypeVariableId]) {
        let mut variables = self.variables.borrow_mut();
        variables.clear();
        variables.extend(
            sources
                .iter()
                .copied()
                .enumerate()
                .map(|(ordinal, source)| {
                    (
                        source,
                        u32::try_from(ordinal)
                            .expect("kernel interface alpha-variable count exceeds u32"),
                    )
                }),
        );
        self.exact_scope.set(true);
    }

    fn ordinal(&self, variable: TypeVariableId) -> u32 {
        let mut variables = self.variables.borrow_mut();
        if let Some((_, ordinal)) = variables.iter().find(|(source, _)| *source == variable) {
            return *ordinal;
        }
        assert!(
            !self.exact_scope.get(),
            "packed receipt type reaches variable {} outside its sealed callable alpha scope",
            variable.0
        );
        let ordinal = u32::try_from(variables.len())
            .expect("kernel packed receipt alpha-variable count exceeds u32");
        variables.push((variable, ordinal));
        ordinal
    }
}

#[allow(dead_code)]
#[derive(Hash)]
enum CheckedTypeHashTag {
    Text,
    Number,
    Bytes,
    Absent,
    VariantSet,
    Object,
    RenderContract,
    List,
    Function,
    UnresolvedShape,
    Var,
    Unknown,
    Union,
    Map,
    Set,
    Bits,
}

#[allow(dead_code)]
#[derive(Hash)]
enum CheckedBytesHashTag {
    Dynamic,
    Fixed,
}

#[allow(dead_code)]
#[derive(Hash)]
enum CheckedVariantHashTag {
    Tag,
    Tagged,
}

#[allow(dead_code)]
#[derive(Hash)]
enum KernelDiagnosticKindHashTag {
    InvalidExpression,
    InvalidPattern,
    InvalidNumberLiteral,
    InvalidBitsLiteral,
    ByteLiteralOutsideBytes,
    DuplicateRecordField,
    MissingPassedContext,
    UnresolvedValue,
    CallableUsedAsValue,
    AmbiguousValue,
    UnresolvedCallable,
    AmbiguousCallable,
    PipeWithoutValueInput,
    UnexpectedCallEntry,
    MisorderedCallEntry,
    MissingCallEntry,
    BareOrdinaryInput,
    PassOnAuthoritativeCallable,
    MissingPassContext,
    CallInputType,
}

struct PackedCheckedFlowHash<'a> {
    arena: &'a TypeTermArena,
    flow: PackedFlow,
    alpha: &'a PackedAlphaHashScratch,
}

impl Hash for PackedCheckedFlowHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.flow.mode.hash(state);
        hash_packed_checked_type(self.arena, self.flow.term, self.alpha, state);
    }
}

struct PackedDiagnosticsHash<'a> {
    arena: &'a TypeTermArena,
    metadata: &'a [PackedDiagnosticMetadata],
    types: &'a [Option<PackedDiagnosticTypes>],
    alpha: &'a PackedAlphaHashScratch,
}

impl Hash for PackedDiagnosticsHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        assert_eq!(
            self.metadata.len(),
            self.types.len(),
            "sealed packed diagnostic metadata and type columns differ"
        );
        self.metadata.len().hash(state);
        for (metadata, types) in self.metadata.iter().zip(self.types) {
            match metadata {
                PackedDiagnosticMetadata::Plain(diagnostic) => {
                    assert!(
                        types.is_none(),
                        "plain packed diagnostic unexpectedly retains type roots"
                    );
                    assert!(
                        !matches!(diagnostic.kind, KernelDiagnosticKind::CallInputType { .. }),
                        "call-input type diagnostic escaped the packed type column"
                    );
                    diagnostic.hash(state);
                }
                PackedDiagnosticMetadata::CallInputType {
                    owner,
                    severity,
                    site,
                    mismatch,
                } => {
                    let types = types.expect(
                        "packed call-input diagnostic retains actual and expected type roots",
                    );
                    owner.hash(state);
                    severity.hash(state);
                    site.hash(state);
                    KernelDiagnosticKindHashTag::CallInputType.hash(state);
                    hash_packed_checked_type(self.arena, types.actual, self.alpha, state);
                    hash_packed_checked_type(self.arena, types.expected, self.alpha, state);
                    mismatch.hash(state);
                }
            }
        }
    }
}

fn hash_packed_checked_type<H: Hasher>(
    arena: &TypeTermArena,
    term: TypeTermId,
    alpha: &PackedAlphaHashScratch,
    state: &mut H,
) {
    match arena.term(term) {
        TypeTerm::Text => CheckedTypeHashTag::Text.hash(state),
        TypeTerm::Number => CheckedTypeHashTag::Number.hash(state),
        TypeTerm::Bytes(bytes) => {
            CheckedTypeHashTag::Bytes.hash(state);
            match bytes {
                BytesTerm::Dynamic => CheckedBytesHashTag::Dynamic.hash(state),
                BytesTerm::Fixed(size) => {
                    CheckedBytesHashTag::Fixed.hash(state);
                    size.hash(state);
                }
            }
        }
        TypeTerm::Absent => CheckedTypeHashTag::Absent.hash(state),
        TypeTerm::VariantSet(variants) => {
            CheckedTypeHashTag::VariantSet.hash(state);
            variants.len().hash(state);
            for variant in variants {
                match variant {
                    VariantTerm::Tag(tag) => {
                        CheckedVariantHashTag::Tag.hash(state);
                        arena.name(*tag).hash(state);
                    }
                    VariantTerm::Tagged { tag, fields } => {
                        CheckedVariantHashTag::Tagged.hash(state);
                        arena.name(*tag).hash(state);
                        hash_packed_object_shape(arena, *fields, alpha, state);
                    }
                }
            }
        }
        TypeTerm::Object { fields, open } => {
            CheckedTypeHashTag::Object.hash(state);
            hash_packed_object_fields(arena, fields, open, alpha, state);
        }
        TypeTerm::OpenObjectPlaceholder => {
            CheckedTypeHashTag::Object.hash(state);
            0usize.hash(state);
            0usize.hash(state);
            true.hash(state);
        }
        TypeTerm::RenderContract => CheckedTypeHashTag::RenderContract.hash(state),
        TypeTerm::List(item) => {
            CheckedTypeHashTag::List.hash(state);
            hash_packed_checked_type(arena, item, alpha, state);
        }
        TypeTerm::Function {
            args,
            result_mode,
            result,
        } => {
            CheckedTypeHashTag::Function.hash(state);
            args.len().hash(state);
            for argument in args {
                hash_packed_checked_type(arena, *argument, alpha, state);
            }
            result_mode.hash(state);
            hash_packed_checked_type(arena, result, alpha, state);
        }
        TypeTerm::UnresolvedShape(reason) => {
            CheckedTypeHashTag::UnresolvedShape.hash(state);
            arena.diagnostic_text(reason).hash(state);
        }
        TypeTerm::Variable(variable) => {
            CheckedTypeHashTag::Var.hash(state);
            alpha.ordinal(variable).hash(state);
        }
        TypeTerm::Unknown => CheckedTypeHashTag::Unknown.hash(state),
        TypeTerm::Union(members) => {
            let members = crate::legacy_checked_union_order(arena, members);
            match members.as_slice() {
                [] => CheckedTypeHashTag::Absent.hash(state),
                [member] => hash_packed_checked_type(arena, *member, alpha, state),
                members => {
                    CheckedTypeHashTag::Union.hash(state);
                    members.len().hash(state);
                    for member in members {
                        hash_packed_checked_type(arena, *member, alpha, state);
                    }
                }
            }
        }
        TypeTerm::Map { key, value } => {
            CheckedTypeHashTag::Map.hash(state);
            hash_packed_checked_type(arena, key, alpha, state);
            hash_packed_checked_type(arena, value, alpha, state);
        }
        TypeTerm::Set(item) => {
            CheckedTypeHashTag::Set.hash(state);
            hash_packed_checked_type(arena, item, alpha, state);
        }
        TypeTerm::Bits(width) => {
            CheckedTypeHashTag::Bits.hash(state);
            width.hash(state);
        }
    }
}

fn hash_packed_object_shape<H: Hasher>(
    arena: &TypeTermArena,
    term: TypeTermId,
    alpha: &PackedAlphaHashScratch,
    state: &mut H,
) {
    match arena.term(term) {
        TypeTerm::Object { fields, open } => {
            hash_packed_object_fields(arena, fields, open, alpha, state)
        }
        TypeTerm::OpenObjectPlaceholder => {
            0usize.hash(state);
            0usize.hash(state);
            true.hash(state);
        }
        _ => unreachable!("kernel tagged payload is always an object"),
    }
}

fn hash_packed_object_fields<H: Hasher>(
    arena: &TypeTermArena,
    fields: crate::ObjectFields<'_>,
    open: bool,
    alpha: &PackedAlphaHashScratch,
    state: &mut H,
) {
    fields.len().hash(state);
    for field in fields.canonical_iter() {
        arena.name(field.name).hash(state);
        hash_packed_checked_type(arena, field.ty, alpha, state);
    }
    fields.len().hash(state);
    for field in fields {
        arena.name(field.name).hash(state);
    }
    open.hash(state);
}

#[cfg(test)]
fn fingerprint_packed_definition_range(
    range: Range<usize>,
    definitions: &[DefinitionArtifact],
    definition_facts: &[KernelDefinitionFactsInput],
    code: &DefinitionCodeStore,
    interface: &KernelInterfaceSnapshot,
    imported_expressions: &[BTreeSet<KernelExpressionId>],
) -> Result<Vec<DefinitionFingerprints>, KernelSolveError> {
    let mut fingerprints = Vec::with_capacity(range.len());
    let mut hash_scratch = Vec::new();
    let alpha_scratch = PackedAlphaHashScratch::default();
    let type_arena = interface.types.as_arena();
    for definition_index in range {
        let owner = KernelOwnerId(
            u32::try_from(definition_index)
                .expect("kernel definition count exceeds dense u32 namespace"),
        );
        let definition = &definitions[definition_index];
        let facts = &definition_facts[definition_index];
        let code = code.definition(owner).ok_or_else(|| {
            KernelSolveError::new(format!(
                "kernel definition code omits owner {definition_index}"
            ))
        })?;
        let interface_definition =
            interface.definitions.get(definition_index).ok_or_else(|| {
                KernelSolveError::new(format!(
                    "kernel interface omits definition {definition_index}"
                ))
            })?;
        alpha_scratch.begin_fresh();
        let public_result = stable_fingerprint(
            KERNEL_PUBLIC_RESULT_DOMAIN_V1,
            &PackedCheckedFlowHash {
                arena: type_arena,
                flow: interface_definition.result,
                alpha: &alpha_scratch,
            },
            &mut hash_scratch,
        );
        let rich_public_result = hash_normalized_flow_type(
            KERNEL_PUBLIC_RESULT_DOMAIN_V1,
            &interface.materialize_result_flow(owner).ok_or_else(|| {
                KernelSolveError::new(format!(
                    "kernel interface omits public result for definition {definition_index}"
                ))
            })?,
            &mut hash_scratch,
        )?;
        assert_eq!(
            public_result, rich_public_result,
            "packed V1 public-result hash must match the rich checked projection for definition {definition_index}",
        );
        alpha_scratch
            .begin_exact(&interface.alpha_variables[interface_definition.alpha_variables.range()]);
        let diagnostic_range = interface_definition.diagnostics.range();
        let artifact = stable_fingerprint(
            KERNEL_DEFINITION_ARTIFACT_DOMAIN_V17,
            &(
                PackedDefinitionHash { definition, facts },
                code.stable_digest(),
                PackedDiagnosticsHash {
                    arena: type_arena,
                    metadata: &interface.diagnostics[diagnostic_range.clone()],
                    types: &interface.diagnostic_types[diagnostic_range],
                    alpha: &alpha_scratch,
                },
            ),
            &mut hash_scratch,
        );
        let rich_diagnostics = interface.diagnostics_for(owner).ok_or_else(|| {
            KernelSolveError::new(format!(
                "kernel interface omits diagnostics for definition {definition_index}"
            ))
        })?;
        let rich_artifact = stable_fingerprint(
            KERNEL_DEFINITION_ARTIFACT_DOMAIN_V17,
            &(
                PackedDefinitionHash { definition, facts },
                code.stable_digest(),
                rich_diagnostics.as_ref(),
            ),
            &mut hash_scratch,
        );
        assert_eq!(
            artifact, rich_artifact,
            "packed V17 artifact hash must match the rich checked projection for definition {definition_index}",
        );
        let mut expressions = BTreeMap::new();
        for expression in imported_expressions[definition_index].iter().copied() {
            let digest = code
                .expression_surface_digest(expression.0 as usize)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition {definition_index} omits imported expression {}",
                        expression.0
                    ))
                })?;
            expressions.insert(
                expression,
                stable_fingerprint(
                    KERNEL_EXPRESSION_SURFACE_DOMAIN_V2,
                    &digest,
                    &mut hash_scratch,
                ),
            );
        }
        fingerprints.push(DefinitionFingerprints {
            public_result,
            artifact,
            expressions,
        });
    }
    Ok(fingerprints)
}

fn fingerprint_borrowed_definition_range(
    range: Range<usize>,
    program: &KernelProjectProgramInput,
    definition_facts: &[KernelDefinitionFactsInput],
    code: &DefinitionCodeStore,
    interface: &KernelInterfaceSnapshot,
    imported_expressions: &[BTreeSet<KernelExpressionId>],
) -> Result<Vec<DefinitionFingerprints>, KernelSolveError> {
    let mut fingerprints = Vec::with_capacity(range.len());
    let mut hash_scratch = Vec::new();
    let alpha_scratch = PackedAlphaHashScratch::default();
    let type_arena = interface.types.as_arena();
    for definition_index in range {
        let owner = KernelOwnerId(
            u32::try_from(definition_index)
                .expect("kernel definition count exceeds dense u32 namespace"),
        );
        let definition =
            KernelDefinitionRef::from_authorities(program, definition_facts, code, owner)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel borrowed authorities omit owner {definition_index}"
                    ))
                })?;
        let interface_definition =
            interface.definitions.get(definition_index).ok_or_else(|| {
                KernelSolveError::new(format!(
                    "kernel interface omits definition {definition_index}"
                ))
            })?;
        alpha_scratch.begin_fresh();
        let public_result = stable_fingerprint(
            KERNEL_PUBLIC_RESULT_DOMAIN_V1,
            &PackedCheckedFlowHash {
                arena: type_arena,
                flow: interface_definition.result,
                alpha: &alpha_scratch,
            },
            &mut hash_scratch,
        );
        alpha_scratch
            .begin_exact(&interface.alpha_variables[interface_definition.alpha_variables.range()]);
        let diagnostic_range = interface_definition.diagnostics.range();
        let artifact = stable_fingerprint(
            KERNEL_DEFINITION_ARTIFACT_DOMAIN_V17,
            &(
                BorrowedDefinitionHash { definition },
                definition.code().stable_digest(),
                PackedDiagnosticsHash {
                    arena: type_arena,
                    metadata: &interface.diagnostics[diagnostic_range.clone()],
                    types: &interface.diagnostic_types[diagnostic_range],
                    alpha: &alpha_scratch,
                },
            ),
            &mut hash_scratch,
        );
        let mut expressions = BTreeMap::new();
        for expression in imported_expressions[definition_index].iter().copied() {
            let digest = definition
                .code()
                .expression_surface_digest(expression.0 as usize)
                .ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition {definition_index} omits imported expression {}",
                        expression.0
                    ))
                })?;
            expressions.insert(
                expression,
                stable_fingerprint(
                    KERNEL_EXPRESSION_SURFACE_DOMAIN_V2,
                    &digest,
                    &mut hash_scratch,
                ),
            );
        }
        fingerprints.push(DefinitionFingerprints {
            public_result,
            artifact,
            expressions,
        });
    }
    Ok(fingerprints)
}

/// Hashes the shared immutable facts and solve-derived rows in the exact field
/// order of the former monolithic `DefinitionArtifact`. This preserves the V17
/// fingerprint contract while deleting its deep relocation/presentation/
/// literal clones.
#[cfg(test)]
struct PackedDefinitionHash<'a> {
    definition: &'a DefinitionArtifact,
    facts: &'a KernelDefinitionFactsInput,
}

/// Streams the exact former `DefinitionArtifact` V17 event sequence from the
/// immutable program/facts and the packed solve store. Every adapter below is
/// stack-only; no string, path, row, or recursive type is cloned merely to
/// establish currentness.
struct BorrowedDefinitionHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedDefinitionHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let definition = self.definition;
        let facts = definition.facts();
        definition.linkage().hash(state);
        facts.relocations.hash(state);
        facts.presentation.hash(state);
        facts.expression_payloads.hash(state);
        BorrowedCallSyntaxHash { definition }.hash(state);
        BorrowedExecutionShapesHash { definition }.hash(state);
        BorrowedExpressionsHash { definition }.hash(state);
        BorrowedStatementsHash { definition }.hash(state);
        BorrowedDeclarationsHash { definition }.hash(state);
        BorrowedLexicalBindingsHash { definition }.hash(state);
        BorrowedCallsHash { definition }.hash(state);
        BorrowedEffectsHash { definition }.hash(state);
        BorrowedSourcesHash { definition }.hash(state);
        BorrowedStatesHash { definition }.hash(state);
        BorrowedListsHash { definition }.hash(state);
    }
}

struct BorrowedCallSyntaxHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedCallSyntaxHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let calls = &self.definition.facts().call_syntax;
        calls.len().hash(state);
        for call in calls {
            let consumer = call.expression.0 as usize;
            call.expression.hash(state);
            call.occurrence.hash(state);
            call.function.hash(state);
            call.pipe_input
                .map(|value| resolve_hash_value(self.definition, value, consumer))
                .hash(state);
            call.arguments.len().hash(state);
            for argument in &call.arguments {
                argument.ordinal.hash(state);
                argument.kind.hash(state);
                argument.name.hash(state);
                resolve_hash_value(self.definition, argument.value, consumer).hash(state);
                argument.span.hash(state);
            }
            call.pass
                .map(|pass| BorrowedCallPassHash {
                    definition: self.definition,
                    consumer,
                    pass,
                })
                .hash(state);
        }
    }
}

#[derive(Clone, Copy)]
struct BorrowedCallPassHash<'a> {
    definition: KernelDefinitionRef<'a>,
    consumer: usize,
    pass: crate::KernelCallPassInput,
}

impl Hash for BorrowedCallPassHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        resolve_hash_value(self.definition, self.pass.value, self.consumer).hash(state);
        self.pass.final_clause.hash(state);
        self.pass.span.hash(state);
    }
}

struct BorrowedExecutionShapesHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedExecutionShapesHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let shapes = &self.definition.facts().execution_shapes;
        shapes.len().hash(state);
        for shape in shapes {
            let borrowed = match shape {
                crate::KernelExecutionShapeInput::Conditional { expression, kind } => {
                    BorrowedExecutionShapeHash::Conditional {
                        expression: *expression,
                        kind: *kind,
                    }
                }
                crate::KernelExecutionShapeInput::Record { expression, fields } => {
                    BorrowedExecutionShapeHash::Record {
                        expression: *expression,
                        fields: BorrowedExecutionRecordFieldsHash {
                            definition: self.definition,
                            consumer: expression.0 as usize,
                            fields,
                        },
                    }
                }
                crate::KernelExecutionShapeInput::Block {
                    expression,
                    bindings,
                    result,
                } => BorrowedExecutionShapeHash::Block {
                    expression: *expression,
                    bindings: BorrowedExecutionBlockBindingsHash {
                        definition: self.definition,
                        consumer: expression.0 as usize,
                        bindings,
                    },
                    result: result.map(|value| {
                        resolve_hash_value(self.definition, value, expression.0 as usize)
                    }),
                },
                crate::KernelExecutionShapeInput::MatchArm {
                    expression,
                    selector,
                    bindings,
                } => BorrowedExecutionShapeHash::MatchArm {
                    expression: *expression,
                    selector: resolve_hash_value(self.definition, *selector, expression.0 as usize),
                    bindings,
                },
            };
            borrowed.hash(state);
        }
    }
}

#[derive(Hash)]
enum BorrowedExecutionShapeHash<'a> {
    Conditional {
        expression: KernelExpressionId,
        kind: crate::KernelConditionalKind,
    },
    Record {
        expression: KernelExpressionId,
        fields: BorrowedExecutionRecordFieldsHash<'a>,
    },
    Block {
        expression: KernelExpressionId,
        bindings: BorrowedExecutionBlockBindingsHash<'a>,
        result: Option<KernelValueReference>,
    },
    MatchArm {
        expression: KernelExpressionId,
        selector: KernelValueReference,
        bindings: &'a [crate::KernelDeclarationId],
    },
}

#[derive(Clone, Copy)]
struct BorrowedExecutionRecordFieldsHash<'a> {
    definition: KernelDefinitionRef<'a>,
    consumer: usize,
    fields: &'a [crate::KernelExecutionRecordFieldInput],
}

impl Hash for BorrowedExecutionRecordFieldsHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fields.len().hash(state);
        for field in self.fields {
            field.ordinal.hash(state);
            field
                .declaration
                .map(|declaration| {
                    self.definition
                        .resolve_structural_declaration(declaration, field.value, self.consumer)
                        .expect("validated structural declaration remains resolvable")
                })
                .hash(state);
            field.name.hash(state);
            resolve_hash_value(self.definition, field.value, self.consumer).hash(state);
            field.spread.hash(state);
            field.span.hash(state);
        }
    }
}

#[derive(Clone, Copy)]
struct BorrowedExecutionBlockBindingsHash<'a> {
    definition: KernelDefinitionRef<'a>,
    consumer: usize,
    bindings: &'a [crate::KernelExecutionBlockBindingInput],
}

impl Hash for BorrowedExecutionBlockBindingsHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bindings.len().hash(state);
        for binding in self.bindings {
            binding.ordinal.hash(state);
            self.definition
                .resolve_structural_declaration(binding.declaration, binding.value, self.consumer)
                .expect("validated block declaration remains resolvable")
                .hash(state);
            resolve_hash_value(self.definition, binding.value, self.consumer).hash(state);
            binding.span.hash(state);
        }
    }
}

struct BorrowedExpressionsHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedExpressionsHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let expressions = &self.definition.input().nodes;
        expressions.len().hash(state);
        for (index, expression) in expressions.iter().enumerate() {
            let id = KernelExpressionId(
                u32::try_from(index).expect("kernel expression count exceeds u32"),
            );
            id.hash(state);
            KernelExpressionKindRef::from(&expression.kind).hash(state);
            expression.inputs.len().hash(state);
            for input in &expression.inputs {
                input.role.hash(state);
                resolve_hash_value(self.definition, input.expression, index).hash(state);
            }
            self.definition
                .expression_effect(id)
                .expect("validated expression retains an effect summary")
                .hash(state);
        }
    }
}

struct BorrowedStatementsHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedStatementsHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let statements = &self.definition.facts().statements;
        statements.len().hash(state);
        for statement in statements {
            statement.id.hash(state);
            statement.kind.hash(state);
            statement
                .value
                .map(|value| resolve_hash_value(self.definition, value, statement.id.0 as usize))
                .hash(state);
            statement.value_use.hash(state);
            statement.children.hash(state);
        }
    }
}

struct BorrowedDeclarationsHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedDeclarationsHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let declarations = &self.definition.facts().declarations;
        declarations.len().hash(state);
        for declaration in declarations {
            declaration.id.hash(state);
            declaration.origin.hash(state);
            declaration.name.hash(state);
            declaration.kind.hash(state);
            declaration
                .value
                .map(|value| resolve_hash_value(self.definition, value, declaration.id.0 as usize))
                .hash(state);
        }
    }
}

struct BorrowedLexicalBindingsHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedLexicalBindingsHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let bindings = &self.definition.facts().lexical_bindings;
        bindings.len().hash(state);
        for binding in bindings {
            binding.expression.hash(state);
            self.definition
                .resolve_lexical_target(binding)
                .expect("validated lexical target remains resolvable")
                .hash(state);
            binding.projection.hash(state);
            binding.access.hash(state);
        }
    }
}

struct BorrowedCallsHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedCallsHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.definition.call_count().hash(state);
        for ordinal in 0..self.definition.call_count() {
            let call = self
                .definition
                .call(ordinal)
                .expect("validated packed call coordinate remains resolvable");
            call.expression().hash(state);
            call.target().hash(state);
            call.inputs().len().hash(state);
            for input in call.inputs() {
                call.input_role(input)
                    .expect("validated call input role remains resolvable")
                    .hash(state);
                call.input_value(input)
                    .expect("validated call input value remains resolvable")
                    .hash(state);
            }
        }
    }
}

struct BorrowedEffectsHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedEffectsHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let effect_count = self
            .definition
            .input()
            .nodes
            .iter()
            .filter(|node| matches!(node.kind, KernelOwnerNodeKind::HostEffect { .. }))
            .count();
        effect_count.hash(state);
        for (index, node) in self.definition.input().nodes.iter().enumerate() {
            let KernelOwnerNodeKind::HostEffect { operation } = &node.kind else {
                continue;
            };
            let spec = host_effect_policy(operation)
                .expect("validated host-effect operation remains in the ABI registry");
            KernelExpressionId(u32::try_from(index).expect("kernel expression count exceeds u32"))
                .hash(state);
            spec.operation.hash(state);
            hash_replay(spec.replay, state);
            hash_barrier(spec.barrier, state);
            hash_result_policy(spec.result_policy, state);
            hash_delivery_policy(spec.delivery, state);
        }
    }
}

struct BorrowedSourcesHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedSourcesHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let sources = &self.definition.facts().sources;
        sources.len().hash(state);
        for source in sources {
            source.id.hash(state);
            source.declaration.hash(state);
            source.statement.hash(state);
            source.expression.hash(state);
            source.declaration.hash(state);
            source.projection.hash(state);
            source.interval_ms.hash(state);
        }
    }
}

struct BorrowedStatesHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedStatesHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.definition.state_count().hash(state);
        for published in self.definition.states() {
            let input = published.input();
            published.id().hash(state);
            input.binding_declaration.hash(state);
            input.declaration.hash(state);
            input.statement.hash(state);
            input.expression.hash(state);
            published
                .initial()
                .expect("validated state initial remains resolvable")
                .hash(state);
            input.declaration.hash(state);
            match published.path() {
                KernelStatePathRef::Authored(projection) => projection.hash(state),
                KernelStatePathRef::Synthetic(ordinal) => {
                    1usize.hash(state);
                    hash_synthetic_state_name(ordinal, state);
                }
            }
            input.kind.hash(state);
        }
    }
}

struct BorrowedListsHash<'a> {
    definition: KernelDefinitionRef<'a>,
}

impl Hash for BorrowedListsHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let lists = &self.definition.facts().lists;
        lists.len().hash(state);
        for list in lists {
            list.id.hash(state);
            list.declaration.hash(state);
            list.statement.hash(state);
            list.producer.hash(state);
            list.declaration.hash(state);
            list.projection.hash(state);
            list.capacity.hash(state);
            list.key_policy.hash(state);
        }
    }
}

fn resolve_hash_value(
    definition: KernelDefinitionRef<'_>,
    value: KernelExpressionId,
    consumer: usize,
) -> KernelValueReference {
    definition
        .resolve_value(value, consumer)
        .expect("validated expression reference remains resolvable")
}

fn hash_replay<H: Hasher>(value: ReplaySpec, state: &mut H) {
    match value {
        ReplaySpec::ReadOnly => 0_u8,
        ReplaySpec::ProcessScoped => 1,
        ReplaySpec::IdempotentBytesKey => 2,
        ReplaySpec::NonReplayable => 3,
    }
    .hash(state);
}

fn hash_barrier<H: Hasher>(value: BarrierSpec, state: &mut H) {
    match value {
        BarrierSpec::None => 0_u8,
        BarrierSpec::Before => 1,
        BarrierSpec::BeforeAndAfter => 2,
    }
    .hash(state);
}

fn hash_result_policy<H: Hasher>(value: ResultPolicySpec, state: &mut H) {
    match value {
        ResultPolicySpec::ReturnValue => 0_u8,
        ResultPolicySpec::Acknowledgement => 1,
        ResultPolicySpec::Discarded => 2,
    }
    .hash(state);
}

fn hash_delivery_policy<H: Hasher>(value: DeliveryCardinalityPolicySpec, state: &mut H) {
    match value {
        DeliveryCardinalityPolicySpec::Single => 0_u8.hash(state),
        DeliveryCardinalityPolicySpec::Stream {
            initial_credits,
            max_in_flight,
            credit_result_tags,
            terminal_result_tags,
        } => {
            1_u8.hash(state);
            initial_credits.hash(state);
            max_in_flight.hash(state);
            credit_result_tags.hash(state);
            terminal_result_tags.hash(state);
        }
    }
}

fn hash_synthetic_state_name<H: Hasher>(ordinal: u32, state: &mut H) {
    const PREFIX: &[u8] = b"state_";
    let mut bytes = [0_u8; PREFIX.len() + 10];
    bytes[..PREFIX.len()].copy_from_slice(PREFIX);
    let mut digits = [0_u8; 10];
    let mut value = ordinal;
    let mut count = 0usize;
    loop {
        digits[count] = b'0' + (value % 10) as u8;
        count += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for index in 0..count {
        bytes[PREFIX.len() + index] = digits[count - index - 1];
    }
    std::str::from_utf8(&bytes[..PREFIX.len() + count])
        .expect("synthetic state name is ASCII")
        .hash(state);
}

#[cfg(test)]
impl Hash for PackedDefinitionHash<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.definition.linkage.hash(state);
        self.facts.relocations.hash(state);
        self.facts.presentation.hash(state);
        self.facts.expression_payloads.hash(state);
        self.definition.call_syntax.hash(state);
        self.definition.execution_shapes.hash(state);
        self.definition.expressions.hash(state);
        self.definition.statements.hash(state);
        self.definition.declarations.hash(state);
        self.definition.lexical_bindings.hash(state);
        self.definition.calls.hash(state);
        self.definition.effects.hash(state);
        self.definition.sources.hash(state);
        self.definition.states.hash(state);
        self.definition.lists.hash(state);
    }
}

fn packed_currentness_receipt_range(
    range: Range<usize>,
    dependency_graph: &KernelDefinitionDependencyGraph,
    basis_fingerprints: &[[u8; 32]],
    fingerprints: &[DefinitionFingerprints],
) -> Result<Vec<KernelPackedDefinitionCurrentnessReceipt>, KernelSolveError> {
    let mut receipts = Vec::with_capacity(range.len());
    let mut hash_scratch = Vec::new();
    for definition_index in range {
        let owner = KernelOwnerId(
            u32::try_from(definition_index)
                .expect("kernel definition count exceeds dense u32 namespace"),
        );
        let dependencies = dependency_graph
            .dependencies(owner)
            .expect("kernel dependency graph contains every definition");
        let imported_authorities = dependencies
            .iter()
            .map(|dependency| {
                let target = dependency.target;
                let provider = target.owner().0 as usize;
                match target {
                    KernelDependencyTarget::Expression { expression, .. } => fingerprints[provider]
                        .expressions
                        .get(&expression)
                        .copied()
                        .ok_or_else(|| {
                            KernelSolveError::new(format!(
                                "kernel dependency targets missing expression {} in definition {}",
                                expression.0, provider
                            ))
                        }),
                    KernelDependencyTarget::Declaration { .. }
                    | KernelDependencyTarget::Source { .. } => Ok(fingerprints[provider].artifact),
                    KernelDependencyTarget::Definition(_)
                    | KernelDependencyTarget::PublicDeclaration(_)
                    | KernelDependencyTarget::PublicStatement(_)
                    | KernelDependencyTarget::Result(_) => Ok(fingerprints[provider].public_result),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let dependency_fingerprint_v3 = stable_fingerprint(
            KERNEL_PACKED_DEPENDENCY_IMPORTS_DOMAIN_V3,
            &(dependencies, imported_authorities),
            &mut hash_scratch,
        );
        let basis_fingerprint_v14 = basis_fingerprints[definition_index];
        let public_result_fingerprint_v1 = fingerprints[definition_index].public_result;
        let artifact_fingerprint_v17 = fingerprints[definition_index].artifact;
        let fingerprint_v18 = stable_fingerprint(
            KERNEL_DEFINITION_CURRENTNESS_DOMAIN_V18,
            &(
                basis_fingerprint_v14,
                artifact_fingerprint_v17,
                dependency_fingerprint_v3,
            ),
            &mut hash_scratch,
        );
        receipts.push(KernelPackedDefinitionCurrentnessReceipt {
            basis_fingerprint_v14,
            public_result_fingerprint_v1,
            artifact_fingerprint_v17,
            dependency_fingerprint_v3,
            fingerprint_v18,
        });
    }
    Ok(receipts)
}

fn collect_definition_ranges<T, F>(len: usize, build: F) -> Result<Vec<T>, KernelSolveError>
where
    T: Send,
    F: Fn(Range<usize>) -> Result<Vec<T>, KernelSolveError> + Sync,
{
    #[cfg(not(target_family = "wasm"))]
    if crate::experimental_parallel_projection_enabled()
        && len >= PARALLEL_DEFINITION_THRESHOLD
        && std::thread::available_parallelism().is_ok_and(|parallelism| parallelism.get() >= 2)
    {
        let split = len / 2;
        return std::thread::scope(|scope| {
            let right_worker = scope.spawn(|| build(split..len));
            let left = build(0..split);
            let right = right_worker.join();
            let mut values = left?;
            values.extend(right.map_err(|_| {
                KernelSolveError::new("kernel definition receipt worker panicked")
            })??);
            Ok(values)
        });
    }

    build(0..len)
}

fn fingerprint_definition_range(
    range: Range<usize>,
    definitions: &[RichDefinitionArtifact],
    imported_expressions: &[BTreeSet<KernelExpressionId>],
) -> Result<Vec<DefinitionFingerprints>, KernelSolveError> {
    let mut fingerprints = Vec::with_capacity(range.len());
    let mut hash_scratch = Vec::new();
    for definition_index in range {
        let definition = &definitions[definition_index];
        let public_result = hash_normalized_flow_type(
            KERNEL_PUBLIC_RESULT_DOMAIN_V1,
            &definition.result,
            &mut hash_scratch,
        )?;
        let artifact = stable_fingerprint(
            KERNEL_DEFINITION_ARTIFACT_DOMAIN_V16,
            definition,
            &mut hash_scratch,
        );
        let mut expressions = BTreeMap::new();
        for expression in definition
            .expressions
            .iter()
            .filter(|expression| imported_expressions[definition_index].contains(&expression.id))
        {
            expressions.insert(
                expression.id,
                hash_normalized_flow_type(
                    KERNEL_EXPRESSION_SURFACE_DOMAIN_V1,
                    &expression.flow_type,
                    &mut hash_scratch,
                )?,
            );
        }
        fingerprints.push(DefinitionFingerprints {
            public_result,
            artifact,
            expressions,
        });
    }
    Ok(fingerprints)
}

fn currentness_receipt_range(
    range: Range<usize>,
    dependency_graph: &KernelDefinitionDependencyGraph,
    basis_fingerprints: &[[u8; 32]],
    fingerprints: &[DefinitionFingerprints],
) -> Result<Vec<KernelDefinitionCurrentnessReceipt>, KernelSolveError> {
    let mut receipts = Vec::with_capacity(range.len());
    let mut hash_scratch = Vec::new();
    for definition_index in range {
        let owner = KernelOwnerId(
            u32::try_from(definition_index)
                .expect("kernel definition count exceeds the dense u32 namespace"),
        );
        let dependencies = dependency_graph
            .dependencies(owner)
            .expect("kernel dependency graph contains every definition");
        let imported_authorities = dependencies
            .iter()
            .map(|dependency| {
                let target = dependency.target;
                let provider = target.owner().0 as usize;
                match target {
                    KernelDependencyTarget::Expression { expression, .. } => fingerprints[provider]
                        .expressions
                        .get(&expression)
                        .copied()
                        .ok_or_else(|| {
                            KernelSolveError::new(format!(
                                "kernel dependency targets missing expression {} in definition {}",
                                expression.0, provider
                            ))
                        }),
                    KernelDependencyTarget::Declaration { .. }
                    | KernelDependencyTarget::Source { .. } => Ok(fingerprints[provider].artifact),
                    KernelDependencyTarget::Definition(_)
                    | KernelDependencyTarget::PublicDeclaration(_)
                    | KernelDependencyTarget::PublicStatement(_)
                    | KernelDependencyTarget::Result(_) => Ok(fingerprints[provider].public_result),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let dependency_fingerprint_v2 = stable_fingerprint(
            KERNEL_DEPENDENCY_IMPORTS_DOMAIN_V2,
            &(dependencies, imported_authorities),
            &mut hash_scratch,
        );
        let basis_fingerprint_v14 = basis_fingerprints[definition_index];
        let public_result_fingerprint_v1 = fingerprints[definition_index].public_result;
        let artifact_fingerprint_v16 = fingerprints[definition_index].artifact;
        let fingerprint_v16 = stable_fingerprint(
            KERNEL_DEFINITION_CURRENTNESS_DOMAIN_V16,
            &(
                basis_fingerprint_v14,
                artifact_fingerprint_v16,
                dependency_fingerprint_v2,
            ),
            &mut hash_scratch,
        );
        receipts.push(KernelDefinitionCurrentnessReceipt {
            basis_fingerprint_v14,
            public_result_fingerprint_v1,
            artifact_fingerprint_v16,
            dependency_fingerprint_v2,
            fingerprint_v16,
        });
    }
    Ok(receipts)
}

fn build_dependency_graph(
    definitions: &[RichDefinitionArtifact],
) -> Result<KernelDefinitionDependencyGraph, KernelSolveError> {
    let mut rows = Vec::with_capacity(definitions.len());
    for (definition_index, definition) in definitions.iter().enumerate() {
        validate_definition_diagnostics(definitions, definition_index, definition)?;
        let mut local = definition_dependencies(definition);
        local.sort_unstable();
        local.dedup();
        for dependency in &local {
            validate_dependency_target(definitions, definition_index, dependency.target)?;
        }
        rows.push(local);
    }
    seal_dependency_graph(rows)
}

fn seal_dependency_graph(
    rows: Vec<Vec<KernelDefinitionDependency>>,
) -> Result<KernelDefinitionDependencyGraph, KernelSolveError> {
    let mut dependency_offsets = Vec::with_capacity(rows.len() + 1);
    let mut dependencies = Vec::new();
    dependency_offsets.push(0);
    for local in rows {
        dependencies.extend(local);
        dependency_offsets.push(checked_u32(
            dependencies.len(),
            "kernel definition dependency count",
        )?);
    }

    let mut reverse = vec![BTreeSet::new(); dependency_offsets.len().saturating_sub(1)];
    for (consumer_index, range) in dependency_offsets.windows(2).enumerate() {
        let consumer = KernelOwnerId(
            u32::try_from(consumer_index)
                .expect("kernel definition count exceeds the dense u32 namespace"),
        );
        for dependency in &dependencies[range[0] as usize..range[1] as usize] {
            let provider = dependency.target.owner();
            if provider != consumer {
                reverse[provider.0 as usize].insert(consumer);
            }
        }
    }
    let mut consumer_offsets = Vec::with_capacity(dependency_offsets.len());
    let mut consumers = Vec::new();
    consumer_offsets.push(0);
    for provider_consumers in reverse {
        consumers.extend(provider_consumers);
        consumer_offsets.push(checked_u32(
            consumers.len(),
            "kernel reverse dependency consumer count",
        )?);
    }
    Ok(KernelDefinitionDependencyGraph {
        dependency_offsets: dependency_offsets.into_boxed_slice(),
        dependencies: dependencies.into_boxed_slice(),
        consumer_offsets: consumer_offsets.into_boxed_slice(),
        consumers: consumers.into_boxed_slice(),
    })
}

#[cfg(test)]
fn build_packed_dependency_graph(
    definitions: &[DefinitionArtifact],
    code: &DefinitionCodeStore,
    interface: &KernelInterfaceSnapshot,
) -> Result<KernelDefinitionDependencyGraph, KernelSolveError> {
    let mut rows = Vec::with_capacity(definitions.len());
    for (definition_index, definition) in definitions.iter().enumerate() {
        validate_packed_definition_diagnostics(
            definitions,
            code,
            interface,
            definition_index,
            definition,
        )?;
        let owner = KernelOwnerId(
            u32::try_from(definition_index)
                .expect("kernel definition count exceeds the dense u32 namespace"),
        );
        let definition_code = code.definition(owner).ok_or_else(|| {
            KernelSolveError::new(format!(
                "kernel definition code omits owner {definition_index}"
            ))
        })?;
        let mut local = packed_definition_dependencies(owner, definition, definition_code);
        local.sort_unstable();
        local.dedup();
        for dependency in &local {
            validate_packed_dependency_target(definitions, definition_index, dependency.target)?;
        }
        rows.push(local);
    }
    seal_dependency_graph(rows)
}

fn build_borrowed_dependency_graph(
    program: &KernelProjectProgramInput,
    facts: &[KernelDefinitionFactsInput],
    code: &DefinitionCodeStore,
    interface: &KernelInterfaceSnapshot,
) -> Result<KernelDefinitionDependencyGraph, KernelSolveError> {
    let mut rows = Vec::with_capacity(program.owners.len());
    for definition_index in 0..program.owners.len() {
        let owner = KernelOwnerId(
            u32::try_from(definition_index)
                .expect("kernel definition count exceeds the dense u32 namespace"),
        );
        let definition = KernelDefinitionRef::from_authorities(program, facts, code, owner)
            .ok_or_else(|| {
                KernelSolveError::new(format!(
                    "kernel borrowed authorities omit owner {definition_index}"
                ))
            })?;
        validate_borrowed_definition_diagnostics(
            program,
            facts,
            code,
            interface,
            definition_index,
            definition,
        )?;
        let mut local = borrowed_definition_dependencies(definition)?;
        local.sort_unstable();
        local.dedup();
        for dependency in &local {
            validate_borrowed_dependency_target(
                program,
                facts,
                code,
                definition_index,
                dependency.target,
            )?;
        }
        rows.push(local);
    }
    seal_dependency_graph(rows)
}

fn validate_borrowed_definition_diagnostics(
    program: &KernelProjectProgramInput,
    facts: &[KernelDefinitionFactsInput],
    code: &DefinitionCodeStore,
    interface: &KernelInterfaceSnapshot,
    owner_index: usize,
    definition: KernelDefinitionRef<'_>,
) -> Result<(), KernelSolveError> {
    let diagnostic_range = interface.definitions[owner_index].diagnostics.range();
    for diagnostic in &interface.diagnostics[diagnostic_range.clone()] {
        match *diagnostic.site() {
            crate::KernelDiagnosticSite::Expression { expression }
            | crate::KernelDiagnosticSite::CallArgument {
                call: expression, ..
            }
            | crate::KernelDiagnosticSite::CallPass {
                call: expression, ..
            } => {
                if expression.0 as usize >= definition.input().nodes.len() {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing expression {}",
                        expression.0
                    )));
                }
            }
            crate::KernelDiagnosticSite::CallInput {
                call,
                target,
                formal_ordinal,
            } => {
                if target.0 as usize >= program.owners.len() || target.0 as usize >= facts.len() {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic targets missing definition {}",
                        target.0
                    )));
                }
                let target_code = code.definition(target).ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic target {} has no code",
                        target.0
                    ))
                })?;
                if target_code.formals().get(formal_ordinal as usize).is_none() {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic targets missing formal {formal_ordinal} in definition {}",
                        target.0
                    )));
                }
                let mut call_matches = false;
                for ordinal in 0..definition.call_count() {
                    let Some(candidate) = definition.call(ordinal) else {
                        continue;
                    };
                    if candidate.expression() != call
                        || !matches!(
                            candidate.target(),
                            KernelCallTargetRef::User {
                                target: candidate_target,
                                ..
                            } if candidate_target == target
                        )
                    {
                        continue;
                    }
                    for edge in candidate.inputs() {
                        if matches!(
                            candidate.input_role(edge),
                            Ok(KernelCallInputRoleRef::Formal { ordinal })
                                if ordinal == formal_ordinal
                        ) {
                            call_matches = true;
                            break;
                        }
                    }
                    if call_matches {
                        break;
                    }
                }
                if !call_matches {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing call input {} formal {formal_ordinal} targeting definition {}",
                        call.0, target.0
                    )));
                }
            }
        }
    }
    Ok(())
}

fn borrowed_definition_dependencies(
    definition: KernelDefinitionRef<'_>,
) -> Result<Vec<KernelDefinitionDependency>, KernelSolveError> {
    let mut dependencies = Vec::new();
    for (expression_index, expression) in definition.input().nodes.iter().enumerate() {
        let expression_id = KernelExpressionId(
            u32::try_from(expression_index).expect("kernel expression count exceeds u32"),
        );
        for (input, edge) in expression.inputs.iter().enumerate() {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::ExpressionInput {
                    expression: expression_id,
                    input: dense_index(input),
                },
                definition
                    .resolve_value(edge.expression, expression_index)
                    .map_err(owner_build_error)?,
            );
        }
    }
    for statement in &definition.facts().statements {
        if let Some(value) = statement.value {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::StatementValue {
                    statement: statement.id,
                },
                definition
                    .resolve_value(value, statement.id.0 as usize)
                    .map_err(owner_build_error)?,
            );
        }
        for (child, reference) in statement.children.iter().enumerate() {
            if let KernelStatementChildReference::Owner(owner) = reference {
                dependencies.push(KernelDefinitionDependency {
                    source: KernelDependencySource::StatementChild {
                        statement: statement.id,
                        child: dense_index(child),
                    },
                    target: KernelDependencyTarget::Definition(*owner),
                });
            }
        }
    }
    for declaration in &definition.facts().declarations {
        if let Some(value) = declaration.value {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::DeclarationValue {
                    declaration: declaration.id,
                },
                definition
                    .resolve_value(value, declaration.id.0 as usize)
                    .map_err(owner_build_error)?,
            );
        }
    }
    for binding in &definition.facts().lexical_bindings {
        match definition
            .resolve_lexical_target(binding)
            .map_err(owner_build_error)?
        {
            KernelLexicalBindingTargetRef::Declaration(reference) => {
                push_declaration_dependency(
                    &mut dependencies,
                    KernelDependencySource::LexicalDeclaration {
                        expression: binding.expression,
                    },
                    reference,
                );
            }
            KernelLexicalBindingTargetRef::Value { provider } => push_value_dependency(
                &mut dependencies,
                KernelDependencySource::LexicalValue {
                    expression: binding.expression,
                },
                provider,
            ),
            KernelLexicalBindingTargetRef::ContextFormal { .. }
            | KernelLexicalBindingTargetRef::RuntimeContext => {}
        }
    }
    for ordinal in 0..definition.call_count() {
        let call = definition.call(ordinal).ok_or_else(|| {
            KernelSolveError::new(format!(
                "kernel definition {} omits packed call {ordinal}",
                definition.owner().0
            ))
        })?;
        if let KernelCallTargetRef::User { target, .. } = call.target() {
            dependencies.push(KernelDefinitionDependency {
                source: KernelDependencySource::CallTarget {
                    expression: call.expression(),
                },
                target: KernelDependencyTarget::Definition(target),
            });
        }
        for (input, edge) in call.inputs().iter().enumerate() {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::CallInput {
                    expression: call.expression(),
                    input: dense_index(input),
                },
                call.input_value(edge).map_err(owner_build_error)?,
            );
        }
    }
    for source in &definition.facts().sources {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::SourceDeclaration { source: source.id },
            source.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::SourceStatement { source: source.id },
            source.statement,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::SourcePathAnchor { source: source.id },
            source.declaration,
        );
    }
    for state in definition.states() {
        let input = state.input();
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StateBindingDeclaration { state: state.id() },
            input.binding_declaration,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StateDeclaration { state: state.id() },
            input.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::StateStatement { state: state.id() },
            input.statement,
        );
        push_value_dependency(
            &mut dependencies,
            KernelDependencySource::StateInitial { state: state.id() },
            state.initial().map_err(owner_build_error)?,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StatePathAnchor { state: state.id() },
            input.declaration,
        );
    }
    for list in &definition.facts().lists {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::ListDeclaration { list: list.id },
            list.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::ListStatement { list: list.id },
            list.statement,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::ListPathAnchor { list: list.id },
            list.declaration,
        );
    }
    for requirement in definition.code().resource_projection_requirements() {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::ResourceProjectionTarget {
                expression: requirement.expression(),
            },
            requirement.target(),
        );
        for (origin, source) in definition
            .code()
            .resource_projection_origins(requirement)
            .iter()
            .copied()
            .enumerate()
        {
            if source.owner() == definition.owner() {
                continue;
            }
            dependencies.push(KernelDefinitionDependency {
                source: KernelDependencySource::ResourceProjectionOrigin {
                    expression: requirement.expression(),
                    origin: dense_index(origin),
                },
                target: KernelDependencyTarget::Source {
                    owner: source.owner(),
                    source: source.source(),
                },
            });
        }
    }
    Ok(dependencies)
}

fn validate_borrowed_dependency_target(
    program: &KernelProjectProgramInput,
    facts: &[KernelDefinitionFactsInput],
    code: &DefinitionCodeStore,
    consumer: usize,
    target: KernelDependencyTarget,
) -> Result<(), KernelSolveError> {
    let provider = target.owner().0 as usize;
    let definition = KernelDefinitionRef::from_authorities(program, facts, code, target.owner())
        .ok_or_else(|| {
            KernelSolveError::new(format!(
                "kernel definition {consumer} depends on missing definition {provider}"
            ))
        })?;
    match target {
        KernelDependencyTarget::Expression { expression, .. }
            if expression.0 as usize >= definition.input().nodes.len() =>
        {
            return Err(KernelSolveError::new(format!(
                "kernel definition {consumer} depends on missing expression {} in definition {provider}",
                expression.0
            )));
        }
        KernelDependencyTarget::Source { source, .. }
            if definition
                .facts()
                .sources
                .get(source.0 as usize)
                .is_none_or(|candidate| candidate.id != source) =>
        {
            return Err(KernelSolveError::new(format!(
                "kernel definition {consumer} depends on missing SOURCE {} in definition {provider}",
                source.0
            )));
        }
        _ => {}
    }
    Ok(())
}

fn owner_build_error(error: KernelOwnerBuildError) -> KernelSolveError {
    KernelSolveError::new(error.to_string())
}

#[cfg(test)]
fn validate_packed_definition_diagnostics(
    definitions: &[DefinitionArtifact],
    code: &DefinitionCodeStore,
    interface: &KernelInterfaceSnapshot,
    owner_index: usize,
    definition: &DefinitionArtifact,
) -> Result<(), KernelSolveError> {
    let diagnostic_range = interface.definitions[owner_index].diagnostics.range();
    for diagnostic in &interface.diagnostics[diagnostic_range.clone()] {
        match *diagnostic.site() {
            crate::KernelDiagnosticSite::Expression { expression } => {
                if definition
                    .expressions
                    .get(expression.0 as usize)
                    .is_none_or(|candidate| candidate.id != expression)
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing expression {}",
                        expression.0
                    )));
                }
            }
            crate::KernelDiagnosticSite::CallArgument { call, .. }
            | crate::KernelDiagnosticSite::CallPass { call, .. } => {
                if definition
                    .expressions
                    .get(call.0 as usize)
                    .is_none_or(|candidate| candidate.id != call)
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing call expression {}",
                        call.0
                    )));
                }
            }
            crate::KernelDiagnosticSite::CallInput {
                call,
                target,
                formal_ordinal,
            } => {
                if definitions.get(target.0 as usize).is_none() {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic targets missing definition {}",
                        target.0
                    )));
                }
                let target_code = code.definition(target).ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic target {} has no code",
                        target.0
                    ))
                })?;
                if target_code.formals().get(formal_ordinal as usize).is_none() {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic targets missing formal {formal_ordinal} in definition {}",
                        target.0
                    )));
                }
                let call_matches = definition.calls.iter().any(|candidate| {
                    candidate.expression == call
                        && matches!(
                            candidate.target,
                            KernelCallTarget::User {
                                target: candidate_target,
                                ..
                            } if candidate_target == target
                        )
                        && candidate.inputs.iter().any(|input| {
                            matches!(
                                input.role,
                                crate::KernelCallInputRole::Formal { ordinal }
                                    if ordinal == formal_ordinal
                            )
                        })
                });
                if !call_matches {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing call input {} formal {formal_ordinal} targeting definition {}",
                        call.0, target.0
                    )));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
fn packed_definition_dependencies(
    owner: KernelOwnerId,
    definition: &DefinitionArtifact,
    code: DefinitionCodeRef<'_>,
) -> Vec<KernelDefinitionDependency> {
    let mut dependencies = Vec::new();
    for expression in &definition.expressions {
        for (input, edge) in expression.inputs.iter().enumerate() {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::ExpressionInput {
                    expression: expression.id,
                    input: dense_index(input),
                },
                edge.value,
            );
        }
    }
    for statement in &definition.statements {
        if let Some(value) = statement.value {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::StatementValue {
                    statement: statement.id,
                },
                value,
            );
        }
        for (child, reference) in statement.children.iter().enumerate() {
            if let KernelStatementChildReference::Owner(owner) = reference {
                dependencies.push(KernelDefinitionDependency {
                    source: KernelDependencySource::StatementChild {
                        statement: statement.id,
                        child: dense_index(child),
                    },
                    target: KernelDependencyTarget::Definition(*owner),
                });
            }
        }
    }
    for declaration in &definition.declarations {
        if let Some(value) = declaration.value {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::DeclarationValue {
                    declaration: declaration.id,
                },
                value,
            );
        }
    }
    for binding in &definition.lexical_bindings {
        match binding.target {
            KernelLexicalBindingTarget::Declaration(reference) => push_declaration_dependency(
                &mut dependencies,
                KernelDependencySource::LexicalDeclaration {
                    expression: binding.expression,
                },
                reference,
            ),
            KernelLexicalBindingTarget::Value { provider } => push_value_dependency(
                &mut dependencies,
                KernelDependencySource::LexicalValue {
                    expression: binding.expression,
                },
                provider,
            ),
            KernelLexicalBindingTarget::ContextFormal { .. }
            | KernelLexicalBindingTarget::RuntimeContext => {}
        }
    }
    for call in &definition.calls {
        if let KernelCallTarget::User { target, .. } = call.target {
            dependencies.push(KernelDefinitionDependency {
                source: KernelDependencySource::CallTarget {
                    expression: call.expression,
                },
                target: KernelDependencyTarget::Definition(target),
            });
        }
        for (input, argument) in call.inputs.iter().enumerate() {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::CallInput {
                    expression: call.expression,
                    input: dense_index(input),
                },
                argument.value,
            );
        }
    }
    for source in &definition.sources {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::SourceDeclaration { source: source.id },
            source.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::SourceStatement { source: source.id },
            source.statement,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::SourcePathAnchor { source: source.id },
            source.path.anchor,
        );
    }
    for state in &definition.states {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StateBindingDeclaration { state: state.id },
            state.binding_declaration,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StateDeclaration { state: state.id },
            state.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::StateStatement { state: state.id },
            state.statement,
        );
        push_value_dependency(
            &mut dependencies,
            KernelDependencySource::StateInitial { state: state.id },
            state.initial,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StatePathAnchor { state: state.id },
            state.path.anchor,
        );
    }
    for list in &definition.lists {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::ListDeclaration { list: list.id },
            list.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::ListStatement { list: list.id },
            list.statement,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::ListPathAnchor { list: list.id },
            list.path.anchor,
        );
    }
    for requirement in code.resource_projection_requirements() {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::ResourceProjectionTarget {
                expression: requirement.expression(),
            },
            requirement.target(),
        );
        for (origin, source) in code
            .resource_projection_origins(requirement)
            .iter()
            .copied()
            .enumerate()
        {
            if source.owner() == owner {
                continue;
            }
            dependencies.push(KernelDefinitionDependency {
                source: KernelDependencySource::ResourceProjectionOrigin {
                    expression: requirement.expression(),
                    origin: dense_index(origin),
                },
                target: KernelDependencyTarget::Source {
                    owner: source.owner(),
                    source: source.source(),
                },
            });
        }
    }
    dependencies
}

#[cfg(test)]
fn validate_packed_dependency_target(
    definitions: &[DefinitionArtifact],
    consumer: usize,
    target: KernelDependencyTarget,
) -> Result<(), KernelSolveError> {
    let provider = target.owner().0 as usize;
    let Some(definition) = definitions.get(provider) else {
        return Err(KernelSolveError::new(format!(
            "kernel definition {consumer} depends on missing definition {provider}"
        )));
    };
    match target {
        KernelDependencyTarget::Expression { expression, .. }
            if definition
                .expressions
                .get(expression.0 as usize)
                .is_none_or(|candidate| candidate.id != expression) =>
        {
            return Err(KernelSolveError::new(format!(
                "kernel definition {consumer} depends on missing expression {} in definition {provider}",
                expression.0
            )));
        }
        KernelDependencyTarget::Source { source, .. }
            if definition
                .sources
                .get(source.0 as usize)
                .is_none_or(|candidate| candidate.id != source) =>
        {
            return Err(KernelSolveError::new(format!(
                "kernel definition {consumer} depends on missing SOURCE {} in definition {provider}",
                source.0
            )));
        }
        _ => {}
    }
    Ok(())
}

fn validate_definition_diagnostics(
    definitions: &[RichDefinitionArtifact],
    owner_index: usize,
    definition: &RichDefinitionArtifact,
) -> Result<(), KernelSolveError> {
    let owner = KernelOwnerId(
        u32::try_from(owner_index)
            .expect("kernel definition count exceeds the dense u32 namespace"),
    );
    for diagnostic in &definition.diagnostics {
        if diagnostic.owner != owner {
            return Err(KernelSolveError::new(format!(
                "kernel definition {owner_index} contains diagnostic owned by definition {}",
                diagnostic.owner.0
            )));
        }
        match diagnostic.site {
            crate::KernelDiagnosticSite::Expression { expression } => {
                if definition
                    .expressions
                    .get(expression.0 as usize)
                    .is_none_or(|candidate| candidate.id != expression)
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing expression {}",
                        expression.0
                    )));
                }
            }
            crate::KernelDiagnosticSite::CallArgument { call, .. }
            | crate::KernelDiagnosticSite::CallPass { call, .. } => {
                if definition
                    .expressions
                    .get(call.0 as usize)
                    .is_none_or(|candidate| candidate.id != call)
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing call expression {}",
                        call.0
                    )));
                }
            }
            crate::KernelDiagnosticSite::CallInput {
                call,
                target,
                formal_ordinal,
            } => {
                let Some(target_definition) = definitions.get(target.0 as usize) else {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic targets missing definition {}",
                        target.0
                    )));
                };
                if target_definition
                    .formals
                    .get(formal_ordinal as usize)
                    .is_none()
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic targets missing formal {formal_ordinal} in definition {}",
                        target.0
                    )));
                }
                let call_matches = definition.calls.iter().any(|candidate| {
                    candidate.expression == call
                        && matches!(
                            candidate.target,
                            KernelCallTarget::User {
                                target: candidate_target,
                                ..
                            } if candidate_target == target
                        )
                        && candidate.inputs.iter().any(|input| {
                            matches!(
                                input.role,
                                crate::KernelCallInputRole::Formal { ordinal }
                                    if ordinal == formal_ordinal
                            )
                        })
                });
                if !call_matches {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing call input {} formal {formal_ordinal} targeting definition {}",
                        call.0, target.0
                    )));
                }
            }
        }
    }
    Ok(())
}

fn definition_dependencies(definition: &RichDefinitionArtifact) -> Vec<KernelDefinitionDependency> {
    let mut dependencies = Vec::new();
    for expression in &definition.expressions {
        for (input, edge) in expression.inputs.iter().enumerate() {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::ExpressionInput {
                    expression: expression.id,
                    input: dense_index(input),
                },
                edge.value,
            );
        }
    }
    for statement in &definition.statements {
        if let Some(value) = statement.value {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::StatementValue {
                    statement: statement.id,
                },
                value,
            );
        }
        for (child, reference) in statement.children.iter().enumerate() {
            if let KernelStatementChildReference::Owner(owner) = reference {
                dependencies.push(KernelDefinitionDependency {
                    source: KernelDependencySource::StatementChild {
                        statement: statement.id,
                        child: dense_index(child),
                    },
                    target: KernelDependencyTarget::Definition(*owner),
                });
            }
        }
    }
    for declaration in &definition.declarations {
        if let Some(value) = declaration.value {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::DeclarationValue {
                    declaration: declaration.id,
                },
                value,
            );
        }
    }
    for binding in &definition.lexical_bindings {
        match binding.target {
            KernelLexicalBindingTarget::Declaration(reference) => push_declaration_dependency(
                &mut dependencies,
                KernelDependencySource::LexicalDeclaration {
                    expression: binding.expression,
                },
                reference,
            ),
            KernelLexicalBindingTarget::Value { provider } => push_value_dependency(
                &mut dependencies,
                KernelDependencySource::LexicalValue {
                    expression: binding.expression,
                },
                provider,
            ),
            KernelLexicalBindingTarget::ContextFormal { .. }
            | KernelLexicalBindingTarget::RuntimeContext => {}
        }
    }
    for call in &definition.calls {
        if let KernelCallTarget::User { target, .. } = call.target {
            dependencies.push(KernelDefinitionDependency {
                source: KernelDependencySource::CallTarget {
                    expression: call.expression,
                },
                target: KernelDependencyTarget::Definition(target),
            });
        }
        for (input, argument) in call.inputs.iter().enumerate() {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::CallInput {
                    expression: call.expression,
                    input: dense_index(input),
                },
                argument.value,
            );
        }
    }
    for source in &definition.sources {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::SourceDeclaration { source: source.id },
            source.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::SourceStatement { source: source.id },
            source.statement,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::SourcePathAnchor { source: source.id },
            source.path.anchor,
        );
    }
    for state in &definition.states {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StateBindingDeclaration { state: state.id },
            state.binding_declaration,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StateDeclaration { state: state.id },
            state.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::StateStatement { state: state.id },
            state.statement,
        );
        push_value_dependency(
            &mut dependencies,
            KernelDependencySource::StateInitial { state: state.id },
            state.initial,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StatePathAnchor { state: state.id },
            state.path.anchor,
        );
    }
    for list in &definition.lists {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::ListDeclaration { list: list.id },
            list.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::ListStatement { list: list.id },
            list.statement,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::ListPathAnchor { list: list.id },
            list.path.anchor,
        );
    }
    dependencies
}

fn push_value_dependency(
    dependencies: &mut Vec<KernelDefinitionDependency>,
    source: KernelDependencySource,
    value: KernelValueReference,
) {
    let KernelValueReference::External(external) = value else {
        return;
    };
    dependencies.push(KernelDefinitionDependency {
        source,
        target: external_target(external),
    });
}

fn external_target(external: KernelExternalExpression) -> KernelDependencyTarget {
    match external.target {
        KernelExternalTarget::Expression(expression) => KernelDependencyTarget::Expression {
            owner: external.owner,
            expression,
        },
        KernelExternalTarget::Result => KernelDependencyTarget::Result(external.owner),
    }
}

fn push_declaration_dependency(
    dependencies: &mut Vec<KernelDefinitionDependency>,
    source: KernelDependencySource,
    reference: KernelDeclarationReference,
) {
    match reference {
        KernelDeclarationReference::Local(_) => {}
        KernelDeclarationReference::OwnerPublic(owner) => {
            dependencies.push(KernelDefinitionDependency {
                source,
                target: KernelDependencyTarget::PublicDeclaration(owner),
            });
        }
        KernelDeclarationReference::OwnerDeclaration { owner, declaration } => {
            dependencies.push(KernelDefinitionDependency {
                source,
                target: KernelDependencyTarget::Declaration { owner, declaration },
            });
        }
    }
}

fn push_statement_dependency(
    dependencies: &mut Vec<KernelDefinitionDependency>,
    source: KernelDependencySource,
    reference: KernelStatementReference,
) {
    if let KernelStatementReference::OwnerPublic(owner) = reference {
        dependencies.push(KernelDefinitionDependency {
            source,
            target: KernelDependencyTarget::PublicStatement(owner),
        });
    }
}

fn validate_dependency_target(
    definitions: &[RichDefinitionArtifact],
    consumer: usize,
    target: KernelDependencyTarget,
) -> Result<(), KernelSolveError> {
    let provider = target.owner().0 as usize;
    let Some(definition) = definitions.get(provider) else {
        return Err(KernelSolveError::new(format!(
            "kernel definition {consumer} depends on missing definition {provider}"
        )));
    };
    if let KernelDependencyTarget::Expression { expression, .. } = target
        && definition
            .expressions
            .get(expression.0 as usize)
            .is_none_or(|candidate| candidate.id != expression)
    {
        return Err(KernelSolveError::new(format!(
            "kernel definition {consumer} depends on missing expression {} in definition {provider}",
            expression.0
        )));
    }
    Ok(())
}

pub(crate) fn alpha_normalize_definition(normalized: &mut RichDefinitionArtifact) {
    let mut variables = BTreeMap::new();
    let mut next = 0;
    for formal in &mut normalized.formals {
        *formal = alpha_normalize_flow_type(formal, &mut variables, &mut next);
    }
    normalized.result = alpha_normalize_flow_type(&normalized.result, &mut variables, &mut next);
    for expression in &mut normalized.expressions {
        expression.flow_type =
            alpha_normalize_flow_type(&expression.flow_type, &mut variables, &mut next);
        if let Some(flush_type) = &mut expression.flush_type {
            *flush_type = alpha_normalize_type(flush_type, &mut variables, &mut next);
        }
        match &mut expression.kind {
            KernelOwnerNodeKind::Known(ty) | KernelOwnerNodeKind::Source(ty) => {
                *ty = alpha_normalize_type(ty, &mut variables, &mut next);
            }
            _ => {}
        }
    }
    for declaration in &mut normalized.declarations {
        if let Some(flow_type) = &mut declaration.declared_flow_type {
            *flow_type = alpha_normalize_flow_type(flow_type, &mut variables, &mut next);
        }
    }
    for call in &mut normalized.calls {
        call.result = alpha_normalize_flow_type(&call.result, &mut variables, &mut next);
        for substitution in &mut call.type_substitutions {
            substitution.value =
                alpha_normalize_type(&substitution.value, &mut variables, &mut next);
        }
    }
    for source in &mut normalized.sources {
        source.payload_type = alpha_normalize_type(&source.payload_type, &mut variables, &mut next);
    }
    for state in &mut normalized.states {
        state.flow_type = alpha_normalize_flow_type(&state.flow_type, &mut variables, &mut next);
    }
    for list in &mut normalized.lists {
        list.item_type = alpha_normalize_type(&list.item_type, &mut variables, &mut next);
    }
    alpha_normalize_diagnostics(&mut normalized.diagnostics, &mut variables, &mut next);
}

pub(crate) fn alpha_normalize_public_flow(flow_type: &FlowType) -> FlowType {
    alpha_normalize_flow_type(flow_type, &mut BTreeMap::new(), &mut 0)
}

fn alpha_normalize_diagnostics(
    diagnostics: &mut [KernelDiagnosticArtifact],
    variables: &mut BTreeMap<TypeVar, TypeVar>,
    next: &mut u32,
) {
    for diagnostic in diagnostics {
        match &mut diagnostic.kind {
            KernelDiagnosticKind::CallInputType {
                actual, expected, ..
            } => {
                *actual = alpha_normalize_type(actual, variables, next);
                *expected = alpha_normalize_type(expected, variables, next);
            }
            KernelDiagnosticKind::InvalidExpression { .. }
            | KernelDiagnosticKind::InvalidPattern
            | KernelDiagnosticKind::InvalidNumberLiteral { .. }
            | KernelDiagnosticKind::InvalidBitsLiteral { .. }
            | KernelDiagnosticKind::ByteLiteralOutsideBytes
            | KernelDiagnosticKind::DuplicateRecordField { .. }
            | KernelDiagnosticKind::MissingPassedContext
            | KernelDiagnosticKind::UnresolvedValue { .. }
            | KernelDiagnosticKind::CallableUsedAsValue { .. }
            | KernelDiagnosticKind::AmbiguousValue { .. }
            | KernelDiagnosticKind::UnresolvedCallable { .. }
            | KernelDiagnosticKind::AmbiguousCallable { .. }
            | KernelDiagnosticKind::PipeWithoutValueInput { .. }
            | KernelDiagnosticKind::UnexpectedCallEntry { .. }
            | KernelDiagnosticKind::MisorderedCallEntry { .. }
            | KernelDiagnosticKind::MissingCallEntry { .. }
            | KernelDiagnosticKind::BareOrdinaryInput { .. }
            | KernelDiagnosticKind::PassOnAuthoritativeCallable { .. }
            | KernelDiagnosticKind::MissingPassContext { .. } => {}
        }
    }
}

fn hash_normalized_flow_type(
    domain: &[u8],
    flow_type: &FlowType,
    scratch: &mut Vec<u8>,
) -> Result<[u8; 32], KernelSolveError> {
    let normalized = alpha_normalize_public_flow(flow_type);
    Ok(stable_fingerprint(domain, &normalized, scratch))
}

pub(crate) fn alpha_normalize_flow_type(
    flow_type: &FlowType,
    variables: &mut BTreeMap<TypeVar, TypeVar>,
    next: &mut u32,
) -> FlowType {
    FlowType {
        mode: flow_type.mode,
        ty: alpha_normalize_type(&flow_type.ty, variables, next),
    }
}

fn alpha_normalize_type(
    ty: &Type,
    variables: &mut BTreeMap<TypeVar, TypeVar>,
    next: &mut u32,
) -> Type {
    alpha_normalize_type_inner(ty, variables, next).unwrap_or_else(|| ty.clone())
}

/// Copy-on-change alpha normalization.
///
/// Most checked types are closed immutable `Arc` subtrees. Rebuilding every
/// object map, field name, field-order vector, collection edge, and variant
/// vector merely to discover that it contains no variable was one of the
/// checked-publication allocation multipliers. `None` means the original
/// subtree is already valid and can be shared verbatim.
fn alpha_normalize_type_inner(
    ty: &Type,
    variables: &mut BTreeMap<TypeVar, TypeVar>,
    next: &mut u32,
) -> Option<Type> {
    match ty {
        Type::Var(variable) => {
            let normalized = *variables.entry(*variable).or_insert_with(|| {
                let normalized = TypeVar(*next);
                *next = next.saturating_add(1);
                normalized
            });
            (normalized != *variable).then_some(Type::Var(normalized))
        }
        Type::Object(shape) => alpha_normalize_object_shape(shape, variables, next)
            .map(|shape| Type::Object(shape.into())),
        Type::List(item) => alpha_normalize_type_inner(item, variables, next)
            .map(|item| Type::List(Type::shared(item))),
        Type::Set(item) => alpha_normalize_type_inner(item, variables, next)
            .map(|item| Type::Set(Type::shared(item))),
        Type::Map { key, value } => {
            let normalized_key = alpha_normalize_type_inner(key, variables, next);
            let normalized_value = alpha_normalize_type_inner(value, variables, next);
            (normalized_key.is_some() || normalized_value.is_some()).then(|| Type::Map {
                key: Box::new(normalized_key.unwrap_or_else(|| key.as_ref().clone())),
                value: Box::new(normalized_value.unwrap_or_else(|| value.as_ref().clone())),
            })
        }
        Type::Function { args, result } => {
            let mut normalized_args = None;
            for (index, argument) in args.iter().enumerate() {
                if let Some(argument) = alpha_normalize_type_inner(argument, variables, next) {
                    normalized_args.get_or_insert_with(|| args.clone())[index] = argument;
                }
            }
            let normalized_result = alpha_normalize_type_inner(&result.ty, variables, next);
            (normalized_args.is_some() || normalized_result.is_some()).then(|| Type::Function {
                args: normalized_args.unwrap_or_else(|| args.clone()),
                result: Box::new(FlowType {
                    mode: result.mode,
                    ty: normalized_result.unwrap_or_else(|| result.ty.clone()),
                }),
            })
        }
        Type::VariantSet(variants) => {
            let mut normalized_variants = None;
            for (index, variant) in variants.iter().enumerate() {
                let Variant::Tagged { tag, fields } = variant else {
                    continue;
                };
                if let Some(fields) = alpha_normalize_object_shape(fields, variables, next) {
                    normalized_variants
                        .get_or_insert_with(|| variants.iter().cloned().collect::<Vec<_>>())
                        [index] = Variant::Tagged {
                        tag: tag.clone(),
                        fields: fields.into(),
                    };
                }
            }
            normalized_variants.map(|variants| Type::VariantSet(variants.into()))
        }
        Type::Union(members) => {
            let mut normalized_members = None;
            for (index, member) in members.iter().enumerate() {
                if let Some(member) = alpha_normalize_type_inner(member, variables, next) {
                    normalized_members.get_or_insert_with(|| members.clone())[index] = member;
                }
            }
            normalized_members.map(Type::Union)
        }
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown
        | Type::Bits { .. } => None,
    }
}

fn alpha_normalize_object_shape(
    shape: &SharedObjectShape,
    variables: &mut BTreeMap<TypeVar, TypeVar>,
    next: &mut u32,
) -> Option<ObjectShape> {
    let mut normalized_fields = None;
    for (name, ty) in &shape.fields {
        if let Some(ty) = alpha_normalize_type_inner(ty, variables, next) {
            normalized_fields
                .get_or_insert_with(|| shape.fields.clone())
                .insert(name.clone(), ty);
        }
    }
    normalized_fields.map(|fields| ObjectShape {
        fields,
        field_order: shape.field_order.clone(),
        open: shape.open,
    })
}

fn checked_u32(value: usize, context: &str) -> Result<u32, KernelSolveError> {
    u32::try_from(value).map_err(|_| KernelSolveError::new(format!("{context} exceeds u32")))
}

fn dense_index(index: usize) -> u32 {
    u32::try_from(index).expect("kernel local artifact row count exceeds u32")
}

/// Direct deterministic structural hashing for hot kernel inventories.
///
/// Rust's derived `Hash` walk gives us the compact typed event stream; this
/// hasher fixes integer encoding to big endian and widens pointer-sized values
/// to 64 bits so fingerprints are process- and target-width-independent. A
/// schema/domain revision is required when a hashed DTO or enum order changes.
struct StableSha256Hasher<'a>(&'a mut Vec<u8>);

impl<'a> StableSha256Hasher<'a> {
    fn new(domain: &[u8], bytes: &'a mut Vec<u8>) -> Self {
        bytes.clear();
        bytes.extend_from_slice(&(domain.len() as u64).to_be_bytes());
        bytes.extend_from_slice(domain);
        Self(bytes)
    }

    fn finalize(self) -> [u8; 32] {
        Sha256::digest(self.0.as_slice()).into()
    }
}

impl Hasher for StableSha256Hasher<'_> {
    fn finish(&self) -> u64 {
        let digest = Sha256::digest(self.0.as_slice());
        u64::from_be_bytes(
            digest[..8]
                .try_into()
                .expect("SHA-256 digest always contains eight prefix bytes"),
        )
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn write_u8(&mut self, value: u8) {
        self.write(&value.to_be_bytes());
    }

    fn write_u16(&mut self, value: u16) {
        self.write(&value.to_be_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.write(&value.to_be_bytes());
    }

    fn write_u64(&mut self, value: u64) {
        self.write(&value.to_be_bytes());
    }

    fn write_u128(&mut self, value: u128) {
        self.write(&value.to_be_bytes());
    }

    fn write_usize(&mut self, value: usize) {
        self.write(&(value as u64).to_be_bytes());
    }

    fn write_i8(&mut self, value: i8) {
        self.write(&value.to_be_bytes());
    }

    fn write_i16(&mut self, value: i16) {
        self.write(&value.to_be_bytes());
    }

    fn write_i32(&mut self, value: i32) {
        self.write(&value.to_be_bytes());
    }

    fn write_i64(&mut self, value: i64) {
        self.write(&value.to_be_bytes());
    }

    fn write_i128(&mut self, value: i128) {
        self.write(&value.to_be_bytes());
    }

    fn write_isize(&mut self, value: isize) {
        self.write(&(value as i64).to_be_bytes());
    }
}

fn stable_fingerprint<T: Hash + ?Sized>(
    domain: &[u8],
    value: &T,
    scratch: &mut Vec<u8>,
) -> [u8; 32] {
    let mut hasher = StableSha256Hasher::new(domain, scratch);
    value.hash(&mut hasher);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DefinitionAdditionalTypeRoots, DefinitionCodeBuilder, KernelArtifactFlowTermV1,
        KernelDefinitionFactsInput, KernelDiagnosticInput, KernelDiagnosticSeverity,
        KernelDiagnosticSite, KernelOwnerEdgeRole, KernelOwnerInputEdge, KernelOwnerNode,
        KernelProjectProgramInput, PackedResourceProjectionRequirementInput, TypeTermArena,
        compile_owner_program_with_definition_facts, compile_project_program_with_definition_facts,
    };
    use boon_checked::FlowMode;
    use std::sync::Arc;

    #[test]
    fn alpha_normalization_shares_closed_immutable_subtrees() {
        let closed = Type::object(ObjectShape::from_ordered_fields(
            [("name".to_owned(), Type::Text)],
            false,
        ));
        let Type::Object(closed_shape) = &closed else {
            unreachable!()
        };
        let unchanged = alpha_normalize_type(&closed, &mut BTreeMap::new(), &mut 0);
        let Type::Object(unchanged_shape) = &unchanged else {
            unreachable!()
        };
        assert!(boon_checked::SharedObjectShape::ptr_eq(
            closed_shape,
            unchanged_shape
        ));

        let generic = Type::object(ObjectShape::from_ordered_fields(
            [
                ("closed".to_owned(), closed.clone()),
                ("value".to_owned(), Type::Var(TypeVar(29))),
            ],
            false,
        ));
        let normalized = alpha_normalize_type(&generic, &mut BTreeMap::new(), &mut 0);
        let Type::Object(normalized) = normalized else {
            unreachable!()
        };
        assert_eq!(normalized.fields["value"], Type::Var(TypeVar(0)));
        let Type::Object(normalized_closed) = &normalized.fields["closed"] else {
            unreachable!()
        };
        assert!(boon_checked::SharedObjectShape::ptr_eq(
            closed_shape,
            normalized_closed
        ));
    }

    #[test]
    fn packed_checked_hash_stream_matches_rich_projection_for_every_type_kind() {
        let mut arena = TypeTermArena::new();
        let text = arena.text();
        let number = arena.number();
        let dynamic_bytes = arena.bytes(BytesTerm::Dynamic);
        let fixed_bytes = arena.bytes(BytesTerm::Fixed(17));
        let absent = arena.absent();
        let render_contract = arena.render_contract();
        let open_placeholder = arena.open_object();
        let open_object = arena.object([], true);
        let unknown = arena.unknown();
        let variable_2 = arena.variable(TypeVariableId(2));
        let variable_10 = arena.variable(TypeVariableId(10));
        let bits = arena.bits(37);
        let unresolved = arena.unresolved_shape("receipt-only unresolved shape");

        let z = arena.intern_name("z");
        let a = arena.intern_name("a");
        let record = arena.object([(z, variable_2), (a, fixed_bytes)], true);
        let tagged_fields = arena.object([(a, text), (z, bits)], false);
        let idle = arena.variant_tag("Idle");
        let ready = arena.tagged_variant("Ready", tagged_fields);
        let variants = arena.variant_set_preserving_order([ready, idle]);
        let list = arena.list(record);
        let set = arena.set(variants);
        let map = arena.map(dynamic_bytes, list);
        let union = arena.union([number, text, variable_2, variable_10, variants]);
        let projected_duplicate_union = arena.union([open_placeholder, open_object]);
        let function = arena.function([record, set, union], FlowMode::PresentOrAbsent, map);
        let roots = [
            text,
            number,
            dynamic_bytes,
            fixed_bytes,
            absent,
            variants,
            record,
            open_placeholder,
            open_object,
            render_contract,
            list,
            unresolved,
            variable_2,
            variable_10,
            unknown,
            union,
            projected_duplicate_union,
            map,
            set,
            bits,
            function,
        ];
        let modes = [
            FlowMode::Continuous,
            FlowMode::TickPresent,
            FlowMode::PresentOrAbsent,
            FlowMode::Absent,
        ];
        let alpha = PackedAlphaHashScratch::default();
        for (ordinal, term) in roots.into_iter().enumerate() {
            let mode = modes[ordinal % modes.len()];
            alpha.begin_fresh();
            let mut packed_bytes = Vec::new();
            let packed = stable_fingerprint(
                b"packed-checked-hash-parity\0",
                &PackedCheckedFlowHash {
                    arena: &arena,
                    flow: PackedFlow { mode, term },
                    alpha: &alpha,
                },
                &mut packed_bytes,
            );
            let rich = alpha_normalize_public_flow(&FlowType {
                mode,
                ty: arena.export_checked_type(term),
            });
            let mut rich_bytes = Vec::new();
            let rich = stable_fingerprint(b"packed-checked-hash-parity\0", &rich, &mut rich_bytes);
            assert_eq!(
                packed_bytes, rich_bytes,
                "hash event stream for term {term:?}"
            );
            assert_eq!(packed, rich, "hash digest for term {term:?}");
        }
    }

    fn value_owner(nodes: Vec<KernelOwnerNode>) -> KernelOwnerProgramInput {
        KernelOwnerProgramInput {
            nodes: nodes.into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        }
    }

    fn external_result_owner(provider: u32) -> KernelOwnerProgramInput {
        KernelOwnerProgramInput {
            nodes: Box::new([KernelOwnerNode {
                kind: KernelOwnerNodeKind::ValueRead {
                    fields: Box::new([]),
                    mode_narrowing: None,
                },
                inputs: Box::new([KernelOwnerInputEdge {
                    role: KernelOwnerEdgeRole::ReadProvider,
                    expression: KernelExpressionId(1),
                }]),
                mode: FlowMode::Continuous,
            }]),
            formal_count: 0,
            external_expressions: Box::new([KernelExternalExpression {
                owner: KernelOwnerId(provider),
                target: KernelExternalTarget::Result,
            }]),
            result: KernelExpressionId(0),
        }
    }

    fn external_expression_owner(provider: u32) -> KernelOwnerProgramInput {
        KernelOwnerProgramInput {
            nodes: Box::new([KernelOwnerNode {
                kind: KernelOwnerNodeKind::ValueRead {
                    fields: Box::new([]),
                    mode_narrowing: None,
                },
                inputs: Box::new([KernelOwnerInputEdge {
                    role: KernelOwnerEdgeRole::ReadProvider,
                    expression: KernelExpressionId(1),
                }]),
                mode: FlowMode::Continuous,
            }]),
            formal_count: 0,
            external_expressions: Box::new([KernelExternalExpression {
                owner: KernelOwnerId(provider),
                target: KernelExternalTarget::Expression(KernelExpressionId(0)),
            }]),
            result: KernelExpressionId(0),
        }
    }

    fn solve_project(owners: Vec<KernelOwnerProgramInput>) -> crate::KernelCheckedSnapshot {
        let facts = vec![KernelDefinitionFactsInput::default(); owners.len()].into_boxed_slice();
        compile_project_program_with_definition_facts(
            &KernelProjectProgramInput {
                owners: owners.into_boxed_slice(),
            },
            &facts,
        )
        .expect("dependency fixture compiles")
        .solve()
        .expect("dependency fixture solves")
    }

    #[test]
    fn dependency_graph_keeps_exact_edges_and_reverse_definition_cones() {
        let snapshot = solve_project(vec![
            value_owner(vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::Number,
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]),
            external_result_owner(0),
            external_result_owner(1),
        ]);

        assert_eq!(
            snapshot.dependencies.dependencies(KernelOwnerId(0)),
            Some(&[][..])
        );
        assert_eq!(
            snapshot.dependencies.dependencies(KernelOwnerId(1)),
            Some(
                &[KernelDefinitionDependency {
                    source: KernelDependencySource::ExpressionInput {
                        expression: KernelExpressionId(0),
                        input: 0,
                    },
                    target: KernelDependencyTarget::Result(KernelOwnerId(0)),
                }][..]
            )
        );
        assert_eq!(
            snapshot.dependencies.consumers(KernelOwnerId(0)),
            Some(&[KernelOwnerId(1)][..])
        );
        assert_eq!(
            snapshot.dependencies.dependent_cone(KernelOwnerId(0)),
            Some(vec![KernelOwnerId(1), KernelOwnerId(2)].into_boxed_slice())
        );
    }

    #[test]
    fn semantic_backdating_is_separate_from_exact_definition_currentness() {
        let first = solve_project(vec![
            value_owner(vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
            ]),
            external_result_owner(0),
        ]);
        let second = solve_project(vec![
            value_owner(vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Absent,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
            ]),
            external_result_owner(0),
        ]);

        assert_eq!(
            first.currentness[0].public_result_fingerprint_v1,
            second.currentness[0].public_result_fingerprint_v1,
            "an unused implementation edit must preserve the public type identity"
        );
        assert_ne!(
            first.currentness[0].artifact_fingerprint_v17,
            second.currentness[0].artifact_fingerprint_v17
        );
        assert_ne!(
            first.currentness[0].fingerprint_v18, second.currentness[0].fingerprint_v18,
            "the edited definition must not claim the old exact evaluation receipt"
        );
        assert_eq!(
            first.currentness[1], second.currentness[1],
            "a dependent definition can backdate when its imported public authority is unchanged"
        );
    }

    #[test]
    fn external_expression_currentness_uses_sparse_published_flow() {
        let snapshot = solve_project(vec![
            value_owner(vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::Unknown,
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]),
            external_expression_owner(0),
        ]);
        let build_code = |publish_text: bool| {
            let arena = TypeTermArena::new();
            let unknown = arena.unknown();
            let required = if publish_text {
                arena.text()
            } else {
                arena.number()
            };
            let base = KernelArtifactFlowTermV1 {
                mode: FlowMode::Continuous,
                term: unknown,
                stable_digest: [11; 32],
                runtime_erased_digest: [12; 32],
            };
            let published = KernelArtifactFlowTermV1 {
                mode: FlowMode::Continuous,
                term: required,
                stable_digest: if publish_text { [21; 32] } else { [22; 32] },
                runtime_erased_digest: [23; 32],
            };
            let requirement = [PackedResourceProjectionRequirementInput {
                expression: KernelExpressionId(0),
                target: KernelDeclarationReference::Local(crate::KernelDeclarationId(0)),
                projection_start: 0,
                projection_len: 0,
                origin_start: 0,
                origin_len: 0,
                required_term: required,
                published_expression: Some(published),
            }];
            let empty_roots = |stable_digest| DefinitionAdditionalTypeRoots {
                effect_summary: crate::KernelEffectSummary::default(),
                basis_fingerprint_v14: stable_digest,
                expression_flush_types: &[None],
                expression_kind_types: &[None],
                declaration_flows: &[],
                calls: &[],
                call_substitutions: &[],
                source_payload_types: &[],
                state_input_count: 0,
                states: &[],
                list_item_types: &[],
                resource_projection_requirements: &[],
                resource_projection_origins: &[],
                resource_projection_symbols: &[],
                alpha_variables: &[],
                callable_type_parameters: &[],
                stable_digest,
            };
            let mut builder = DefinitionCodeBuilder::with_capacity(2, 2, 0);
            let mut provider_roots = empty_roots([31; 32]);
            provider_roots.resource_projection_requirements = &requirement;
            builder
                .push(KernelOwnerId(0), base, &[], &[base], provider_roots)
                .unwrap();
            builder
                .push(KernelOwnerId(1), base, &[], &[base], empty_roots([32; 32]))
                .unwrap();
            builder.finish(Arc::new(arena.freeze())).unwrap()
        };

        let number_code = build_code(false);
        let text_code = build_code(true);
        let basis = [[41; 32], [42; 32]];
        let (_, number_receipts) = build_packed_snapshot_receipts(
            &snapshot.definitions,
            &snapshot.definition_facts,
            &number_code,
            &snapshot.interface,
            &basis,
        )
        .unwrap();
        let (_, text_receipts) = build_packed_snapshot_receipts(
            &snapshot.definitions,
            &snapshot.definition_facts,
            &text_code,
            &snapshot.interface,
            &basis,
        )
        .unwrap();

        assert_eq!(
            number_receipts[1].artifact_fingerprint_v17, text_receipts[1].artifact_fingerprint_v17,
            "the consumer's own packed artifact is unchanged",
        );
        assert_ne!(
            number_receipts[1].dependency_fingerprint_v3,
            text_receipts[1].dependency_fingerprint_v3,
            "the imported expression authority must use the corrected published flow",
        );
        assert_ne!(
            number_receipts[1].fingerprint_v18,
            text_receipts[1].fingerprint_v18,
        );
    }

    #[test]
    fn semantic_artifact_fingerprints_alpha_normalize_type_variables() {
        let definition = |variable| {
            let result = FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Var(TypeVar(variable)),
            };
            let expression = crate::RichKernelExpressionArtifact {
                id: KernelExpressionId(0),
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 0,
                    fields: Box::new([]),
                },
                inputs: Box::new([]),
                flow_type: FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Var(TypeVar(variable)),
                },
                flush_type: None,
                effect: crate::KernelEffectSummary::default(),
            };
            RichDefinitionArtifact {
                result,
                formals: Box::new([]),
                linkage: crate::KernelDefinitionLinkage::default(),
                relocations: crate::KernelDefinitionRelocations::default(),
                presentation: crate::KernelDefinitionPresentation::default(),
                expression_payloads: Box::new([]),
                call_syntax: Box::new([]),
                execution_shapes: Box::new([]),
                expressions: Box::new([expression]),
                statements: Box::new([]),
                declarations: Box::new([]),
                lexical_bindings: Box::new([]),
                calls: Box::new([]),
                effects: Box::new([]),
                sources: Box::new([]),
                states: Box::new([]),
                lists: Box::new([]),
                diagnostics: Box::new([]),
            }
        };
        let mut first_definition = [definition(7)];
        let mut second_definition = [definition(91)];
        let (_, first) = build_snapshot_receipts(&mut first_definition, &[[7; 32]])
            .expect("first generic artifact fingerprints");
        let (_, second) = build_snapshot_receipts(&mut second_definition, &[[91; 32]])
            .expect("second generic artifact fingerprints");

        assert_eq!(
            first[0].public_result_fingerprint_v1,
            second[0].public_result_fingerprint_v1
        );
        assert_eq!(
            first[0].artifact_fingerprint_v16,
            second[0].artifact_fingerprint_v16
        );
        assert_eq!(
            first[0].artifact_fingerprint_v16,
            [
                21, 19, 130, 76, 180, 103, 146, 59, 118, 186, 174, 194, 7, 164, 208, 76, 87, 149,
                115, 211, 134, 6, 42, 188, 61, 116, 29, 214, 91, 151, 135, 112,
            ],
            "the V16 artifact fingerprint byte contract changed"
        );
        assert_ne!(
            first[0].basis_fingerprint_v14,
            second[0].basis_fingerprint_v14
        );
        assert_ne!(first[0].fingerprint_v16, second[0].fingerprint_v16);
    }

    #[test]
    fn diagnostic_facts_change_exact_artifact_currentness_but_not_public_interfaces() {
        let project = KernelProjectProgramInput {
            owners: Box::new([value_owner(vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::Number,
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }])]),
        };
        let clean = compile_project_program_with_definition_facts(
            &project,
            &[KernelDefinitionFactsInput::default()],
        )
        .unwrap()
        .solve()
        .unwrap();
        let diagnosed = compile_project_program_with_definition_facts(
            &project,
            &[KernelDefinitionFactsInput {
                diagnostics: Box::new([KernelDiagnosticInput {
                    severity: KernelDiagnosticSeverity::Error,
                    site: KernelDiagnosticSite::Expression {
                        expression: KernelExpressionId(0),
                    },
                    kind: KernelDiagnosticKind::UnresolvedCallable {
                        function: "missing".into(),
                    },
                }]),
                ..KernelDefinitionFactsInput::default()
            }],
        )
        .unwrap()
        .solve()
        .unwrap();
        assert!(clean.diagnostics_for(KernelOwnerId(0)).unwrap().is_empty());
        assert_eq!(
            diagnosed.diagnostics_for(KernelOwnerId(0)).unwrap().len(),
            1
        );

        assert_eq!(
            clean.currentness[0].public_result_fingerprint_v1,
            diagnosed.currentness[0].public_result_fingerprint_v1
        );
        assert_ne!(
            clean.currentness[0].artifact_fingerprint_v17,
            diagnosed.currentness[0].artifact_fingerprint_v17
        );
        assert_ne!(
            clean.currentness[0].fingerprint_v18,
            diagnosed.currentness[0].fingerprint_v18
        );
    }

    #[test]
    fn typed_call_diagnostics_change_basis_and_exact_currentness_not_public_type() {
        let input = value_owner(vec![KernelOwnerNode {
            kind: KernelOwnerNodeKind::Unknown,
            inputs: Box::new([]),
            mode: FlowMode::Continuous,
        }]);
        let facts = |kind| KernelDefinitionFactsInput {
            diagnostics: vec![KernelDiagnosticInput {
                severity: KernelDiagnosticSeverity::Error,
                site: KernelDiagnosticSite::Expression {
                    expression: KernelExpressionId(0),
                },
                kind,
            }]
            .into_boxed_slice(),
            ..KernelDefinitionFactsInput::default()
        };
        let first = compile_owner_program_with_definition_facts(
            &input,
            &facts(KernelDiagnosticKind::UnresolvedCallable {
                function: "first".into(),
            }),
        )
        .expect("first call diagnostic compiles")
        .solve()
        .expect("first call diagnostic solves");
        let second = compile_owner_program_with_definition_facts(
            &input,
            &facts(KernelDiagnosticKind::MissingCallEntry {
                function: "second".into(),
                name: "value".into(),
            }),
        )
        .expect("second call diagnostic compiles")
        .solve()
        .expect("second call diagnostic solves");

        assert_eq!(first.definition.result, second.definition.result);
        assert_eq!(
            first.currentness.public_result_fingerprint_v1,
            second.currentness.public_result_fingerprint_v1
        );
        assert_ne!(
            first.currentness.basis_fingerprint_v14,
            second.currentness.basis_fingerprint_v14
        );
        assert_ne!(
            first.currentness.artifact_fingerprint_v16,
            second.currentness.artifact_fingerprint_v16
        );
        assert_ne!(
            first.currentness.fingerprint_v16,
            second.currentness.fingerprint_v16
        );
    }
}
