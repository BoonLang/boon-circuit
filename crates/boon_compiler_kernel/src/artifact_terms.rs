use crate::{BytesTerm, KernelSolveError, TypeTerm, TypeTermArena, TypeTermId, TypeVariableId};
use boon_checked::FlowMode;
#[cfg(test)]
use boon_checked::{ArtifactFlowTermV1, ArtifactTypeModuleBuilderV1};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::collections::BTreeMap;
use std::fmt;
#[cfg(test)]
use std::hash::{Hash, Hasher};

#[cfg(test)]
const KERNEL_DEFINITION_FLOW_TERMS_DOMAIN_V1: &[u8] =
    b"boon.compiler-kernel.definition-flow-terms.v1\0";
const ARTIFACT_TYPE_TERM_DOMAIN_V1: &[u8] = b"boon.artifact-type-term.v1\0";
const ARTIFACT_FLOW_TERM_DOMAIN_V1: &[u8] = b"boon.artifact-flow-term.v1\0";
#[cfg(test)]
const ARTIFACT_TYPE_MODULE_DOMAIN_V1: &[u8] = b"boon.artifact-type-module.v1\0";

/// One definition-local flow root in the solved component type arena.
///
/// The dense coordinate is revision-local. Stable and runtime-erased digests
/// retain the existing artifact proof contract without building a second
/// string-owning type DAG for every definition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct KernelArtifactFlowTermV1 {
    pub(crate) mode: FlowMode,
    pub(crate) term: TypeTermId,
    pub(crate) stable_digest: [u8; 32],
    pub(crate) runtime_erased_digest: [u8; 32],
}

/// Internal proof sidecar for flow roots reached by the compatibility checked
/// projection.
///
/// A bare solver term ID is not a public type reference: it is meaningful only
/// with the exact frozen component store, and its variables still use the
/// component namespace. The permanent packed API will expose a store- and
/// definition-qualified reference after every type-bearing definition row has
/// moved to the shared authority.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KernelDefinitionFlowTermsV1 {
    pub(crate) module_stable_digest: [u8; 32],
    pub(crate) result: KernelArtifactFlowTermV1,
    pub(crate) formals: Box<[KernelArtifactFlowTermV1]>,
    pub(crate) expressions: Box<[KernelArtifactFlowTermV1]>,
    /// Component variable IDs in the alpha order induced only by the flow
    /// roots above. This is deliberately not the definition-wide alpha map:
    /// flush, known/source, call-substitution, resource, and diagnostic rows
    /// still belong to the rich compatibility projection.
    pub(crate) flow_variable_sources_by_ordinal: Box<[TypeVariableId]>,
    pub(crate) stable_digest: [u8; 32],
}

#[cfg(test)]
impl Hash for KernelDefinitionFlowTermsV1 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Bind the structural module and ordered roots once. Hashing its rich
        // nodes again would undo the purpose of the canonical term authority.
        self.stable_digest.hash(state);
    }
}

#[cfg(test)]
pub(crate) fn materialize_definition_flow_terms_v1(
    source: &TypeTermArena,
    formal_roots: &[(TypeTermId, FlowMode)],
    result_root: (TypeTermId, FlowMode),
    expression_roots: &[(TypeTermId, FlowMode)],
    scratch: &mut DefinitionTermProofScratch,
) -> Result<KernelDefinitionFlowTermsV1, KernelSolveError> {
    scratch.begin(source.len());
    let formals = formal_roots
        .iter()
        .map(|(term, mode)| scratch.import_flow(source, *term, *mode))
        .collect::<Result<Vec<_>, _>>()?;
    let result = scratch.import_flow(source, result_root.0, result_root.1)?;
    let expressions = expression_roots
        .iter()
        .map(|(term, mode)| scratch.import_flow(source, *term, *mode))
        .collect::<Result<Vec<_>, _>>()?;
    let module_stable_digest = scratch.module_digest();
    let stable_digest =
        definition_flow_terms_digest(module_stable_digest, result, &formals, &expressions);
    Ok(KernelDefinitionFlowTermsV1 {
        module_stable_digest,
        result,
        formals: formals.into_boxed_slice(),
        expressions: expressions.into_boxed_slice(),
        flow_variable_sources_by_ordinal: scratch
            .variable_sources_by_ordinal
            .clone()
            .into_boxed_slice(),
        stable_digest,
    })
}

#[derive(Clone, Copy, Debug, Default)]
struct TermProof {
    stable: [u8; 32],
    runtime_erased: [u8; 32],
}

/// Reusable generation-stamped proof scratch for one definition worker.
///
/// A previous dense importer cleared roughly 85 million slots on NovyWave;
/// allocating a hash/tree map per definition replaced that clear with many
/// small allocations. This scratch keeps O(component terms) storage once and
/// resets only the rows reached by the next definition.
#[derive(Debug, Default)]
pub(crate) struct DefinitionTermProofScratch {
    generation: u32,
    term_generations: Vec<u32>,
    term_proofs: Vec<TermProof>,
    variable_generations: Vec<u32>,
    variable_ordinals: Vec<u32>,
    variable_sources_by_ordinal: Vec<TypeVariableId>,
    next_variable: u32,
    module_term_digests: Vec<[u8; 32]>,
}

impl DefinitionTermProofScratch {
    fn begin(&mut self, term_count: usize) {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.term_generations.fill(0);
            self.variable_generations.fill(0);
            self.generation = 1;
        }
        self.term_generations.resize(term_count, 0);
        self.term_proofs.resize(term_count, TermProof::default());
        self.next_variable = 0;
        self.variable_sources_by_ordinal.clear();
        self.module_term_digests.clear();
        // ArtifactTypeModuleBuilderV1 always installs Unknown before importing
        // roots, so preserve that exact stable module proof contract.
        self.module_term_digests.push(scalar_term_digest(11));
    }

    /// Begin one definition-wide alpha-order traversal.
    ///
    /// Unlike the flow-proof import above, callers visit every type-bearing
    /// definition row in compatibility order. Reusing this scratch keeps the
    /// traversal generation-stamped and allocation-free after its first
    /// component-sized growth.
    pub(crate) fn begin_alpha_variables(&mut self, source: &TypeTermArena) {
        self.begin(source.len());
    }

    pub(crate) fn visit_alpha_root(
        &mut self,
        source: &TypeTermArena,
        term: TypeTermId,
    ) -> Result<[u8; 32], KernelSolveError> {
        self.import(source, term).map(|proof| proof.stable)
    }

    pub(crate) fn visit_alpha_flow_root(
        &mut self,
        source: &TypeTermArena,
        term: TypeTermId,
        mode: FlowMode,
    ) -> Result<[u8; 32], KernelSolveError> {
        self.import(source, term)
            .map(|proof| flow_digest(mode, proof.stable))
    }

    pub(crate) fn materialize_alpha_flow_root(
        &mut self,
        source: &TypeTermArena,
        term: TypeTermId,
        mode: FlowMode,
    ) -> Result<KernelArtifactFlowTermV1, KernelSolveError> {
        self.import_flow(source, term, mode)
    }

    pub(crate) fn alpha_variable_sources(&self) -> &[TypeVariableId] {
        &self.variable_sources_by_ordinal
    }

    fn import_flow(
        &mut self,
        source: &TypeTermArena,
        term: TypeTermId,
        mode: FlowMode,
    ) -> Result<KernelArtifactFlowTermV1, KernelSolveError> {
        let proof = self.import(source, term)?;
        Ok(KernelArtifactFlowTermV1 {
            mode,
            term,
            stable_digest: flow_digest(mode, proof.stable),
            runtime_erased_digest: flow_digest(mode, proof.runtime_erased),
        })
    }

    fn import(
        &mut self,
        source: &TypeTermArena,
        source_id: TypeTermId,
    ) -> Result<TermProof, KernelSolveError> {
        let index = source_id.0 as usize;
        if self.term_generations.get(index).copied() == Some(self.generation) {
            return Ok(self.term_proofs[index]);
        }
        let projected_union_members = match source.term(source_id) {
            TypeTerm::Union(members) => Some(legacy_checked_union_order(source, members)),
            _ => None,
        };
        if let Some(members) = projected_union_members.as_deref()
            && members.len() <= 1
        {
            // The solver intentionally distinguishes inference-only terms that
            // the checked compatibility schema projects to the same rich type.
            // Canonical checked unions collapse that equivalence, including a
            // one-member union becoming the member itself.
            let projected = members.first().copied().unwrap_or_else(|| source.absent());
            let proof = self.import(source, projected)?;
            self.term_generations[index] = self.generation;
            self.term_proofs[index] = proof;
            return Ok(proof);
        }
        let mut stable = type_term_hasher();
        let mut runtime = type_term_hasher();
        match source.term(source_id) {
            TypeTerm::Text => update_same_tag(&mut stable, &mut runtime, 0),
            TypeTerm::Number => update_same_tag(&mut stable, &mut runtime, 1),
            TypeTerm::Bytes(BytesTerm::Dynamic) => {
                stable.update([2, 0]);
                runtime.update([2, 0]);
            }
            TypeTerm::Bytes(BytesTerm::Fixed(size)) => {
                let size = u64::try_from(size).map_err(|_| {
                    KernelSolveError::new("kernel fixed byte-list size exceeds u64")
                })?;
                stable.update([2, 1]);
                stable.update(size.to_be_bytes());
                runtime.update([2, 1]);
                runtime.update(size.to_be_bytes());
            }
            TypeTerm::Absent => update_same_tag(&mut stable, &mut runtime, 3),
            TypeTerm::VariantSet(variants) => {
                stable.update([4]);
                runtime.update([4]);
                update_len_pair(&mut stable, &mut runtime, variants.len());
                for variant in variants {
                    match variant {
                        crate::VariantTerm::Tag(tag) => {
                            stable.update([0]);
                            runtime.update([0]);
                            update_string_pair(&mut stable, &mut runtime, source.name(*tag));
                        }
                        crate::VariantTerm::Tagged { tag, fields } => {
                            stable.update([1]);
                            runtime.update([1]);
                            update_string_pair(&mut stable, &mut runtime, source.name(*tag));
                            let child = self.import(source, *fields)?;
                            stable.update(child.stable);
                            runtime.update(child.runtime_erased);
                        }
                    }
                }
            }
            TypeTerm::Object { fields, open } => {
                stable.update([5]);
                runtime.update([5]);
                update_len_pair(&mut stable, &mut runtime, fields.len());
                for field in fields.canonical_iter() {
                    update_string_pair(&mut stable, &mut runtime, source.name(field.name));
                    let child = self.import(source, field.ty)?;
                    stable.update(child.stable);
                    runtime.update(child.runtime_erased);
                }
                update_len_pair(&mut stable, &mut runtime, fields.len());
                for field in fields {
                    update_string_pair(&mut stable, &mut runtime, source.name(field.name));
                }
                stable.update([u8::from(open)]);
                runtime.update([u8::from(open)]);
            }
            TypeTerm::OpenObjectPlaceholder => {
                stable.update([5]);
                runtime.update([5]);
                update_len_pair(&mut stable, &mut runtime, 0);
                update_len_pair(&mut stable, &mut runtime, 0);
                stable.update([1]);
                runtime.update([1]);
            }
            TypeTerm::RenderContract => update_same_tag(&mut stable, &mut runtime, 6),
            TypeTerm::List(item) => {
                stable.update([7]);
                runtime.update([7]);
                let child = self.import(source, item)?;
                stable.update(child.stable);
                runtime.update(child.runtime_erased);
            }
            TypeTerm::Function {
                args,
                result_mode,
                result,
            } => {
                stable.update([8]);
                runtime.update([8]);
                update_len_pair(&mut stable, &mut runtime, args.len());
                for argument in args {
                    let child = self.import(source, *argument)?;
                    stable.update(child.stable);
                    runtime.update(child.runtime_erased);
                }
                let mode = flow_mode_tag(result_mode);
                stable.update([mode]);
                runtime.update([mode]);
                let child = self.import(source, result)?;
                stable.update(child.stable);
                runtime.update(child.runtime_erased);
            }
            TypeTerm::UnresolvedShape(reason) => {
                stable.update([9]);
                runtime.update([9]);
                update_string_pair(&mut stable, &mut runtime, source.diagnostic_text(reason));
            }
            TypeTerm::Variable(variable) => {
                let ordinal = self.variable_ordinal(variable)?;
                stable.update([10]);
                stable.update(ordinal.to_be_bytes());
                runtime.update([11]);
            }
            TypeTerm::Unknown => update_same_tag(&mut stable, &mut runtime, 11),
            TypeTerm::Union(_) => {
                stable.update([12]);
                runtime.update([12]);
                let members = projected_union_members
                    .as_deref()
                    .expect("kernel union projection was prepared");
                update_len_pair(&mut stable, &mut runtime, members.len());
                for member in members {
                    let child = self.import(source, *member)?;
                    stable.update(child.stable);
                    runtime.update(child.runtime_erased);
                }
            }
            TypeTerm::Map { key, value } => {
                stable.update([13]);
                runtime.update([13]);
                for child in [key, value] {
                    let child = self.import(source, child)?;
                    stable.update(child.stable);
                    runtime.update(child.runtime_erased);
                }
            }
            TypeTerm::Set(item) => {
                stable.update([14]);
                runtime.update([14]);
                let child = self.import(source, item)?;
                stable.update(child.stable);
                runtime.update(child.runtime_erased);
            }
            TypeTerm::Bits(width) => {
                stable.update([15]);
                stable.update(width.to_be_bytes());
                runtime.update([15]);
                runtime.update(width.to_be_bytes());
            }
        }
        let proof = TermProof {
            stable: stable.finalize().into(),
            runtime_erased: runtime.finalize().into(),
        };
        self.term_generations[index] = self.generation;
        self.term_proofs[index] = proof;
        self.module_term_digests.push(proof.stable);
        if proof.runtime_erased != proof.stable {
            self.module_term_digests.push(proof.runtime_erased);
        }
        Ok(proof)
    }

    fn variable_ordinal(&mut self, variable: TypeVariableId) -> Result<u32, KernelSolveError> {
        let index = variable.0 as usize;
        if self.variable_generations.len() <= index {
            self.variable_generations.resize(index + 1, 0);
            self.variable_ordinals.resize(index + 1, 0);
        }
        if self.variable_generations[index] == self.generation {
            return Ok(self.variable_ordinals[index]);
        }
        let ordinal = self.next_variable;
        self.next_variable = self.next_variable.checked_add(1).ok_or_else(|| {
            KernelSolveError::new("kernel artifact type-variable count exceeds u32")
        })?;
        self.variable_generations[index] = self.generation;
        self.variable_ordinals[index] = ordinal;
        self.variable_sources_by_ordinal.push(variable);
        Ok(ordinal)
    }

    #[cfg(test)]
    fn module_digest(&mut self) -> [u8; 32] {
        self.module_term_digests.sort_unstable();
        self.module_term_digests.dedup();
        let mut hasher = Sha256::new();
        hasher.update(ARTIFACT_TYPE_MODULE_DOMAIN_V1);
        update_len(&mut hasher, self.module_term_digests.len());
        for digest in &self.module_term_digests {
            hasher.update(digest);
        }
        hasher.finalize().into()
    }
}

/// Preserve the V1 checked-artifact union contract during the packed cut.
///
/// `boon_checked::canonical_union_type` historically ordered members by their
/// derived `Debug` text and then removed equal rich types. Solver terms use a
/// cheaper structural order, and inference-only placeholders can project to
/// the same rich type. Replaying that legacy boundary only for reachable union
/// roots keeps existing receipts exact without rebuilding a rich type module
/// for every definition. A future artifact schema can replace this textual
/// compatibility order with one shared structural comparator.
pub(crate) fn legacy_checked_union_order(
    source: &TypeTermArena,
    members: &[TypeTermId],
) -> Vec<TypeTermId> {
    let mut keyed = members
        .iter()
        .copied()
        .map(|member| {
            (
                format!("{:?}", CheckedProjectionDebug::new(source, member)),
                member,
            )
        })
        .collect::<Vec<_>>();
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    // Derived Debug is a lossless structural rendering for the checked Type
    // model. Equal checked projections therefore have the same key, including
    // the inference-only open-object placeholder and an actual open empty
    // object. This removes the former recursive rich Type construction from
    // every reachable union while retaining the V1 ordering contract.
    keyed.dedup_by(|left, right| left.0 == right.0);
    keyed.into_iter().map(|(_, member)| member).collect()
}

#[derive(Clone, Copy)]
struct CheckedProjectionDebug<'a> {
    source: &'a TypeTermArena,
    term: TypeTermId,
}

impl<'a> CheckedProjectionDebug<'a> {
    const fn new(source: &'a TypeTermArena, term: TypeTermId) -> Self {
        Self { source, term }
    }
}

impl fmt::Debug for CheckedProjectionDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.source.term(self.term) {
            TypeTerm::Text => formatter.write_str("Text"),
            TypeTerm::Number => formatter.write_str("Number"),
            TypeTerm::Bytes(bytes) => formatter
                .debug_tuple("Bytes")
                .field(&CheckedBytesDebug(bytes))
                .finish(),
            TypeTerm::Absent => formatter.write_str("Absent"),
            TypeTerm::VariantSet(variants) => formatter
                .debug_tuple("VariantSet")
                .field(&CheckedVariantSetDebug {
                    source: self.source,
                    variants,
                })
                .finish(),
            TypeTerm::Object { fields, open } => formatter
                .debug_tuple("Object")
                .field(&CheckedSharedObjectDebug {
                    source: self.source,
                    fields,
                    open,
                })
                .finish(),
            TypeTerm::OpenObjectPlaceholder => formatter
                .debug_tuple("Object")
                .field(&CheckedOpenObjectDebug)
                .finish(),
            TypeTerm::RenderContract => formatter.write_str("RenderContract"),
            TypeTerm::List(item) => formatter
                .debug_tuple("List")
                .field(&CheckedSharedTypeDebug::new(self.source, item))
                .finish(),
            TypeTerm::Function {
                args,
                result_mode,
                result,
            } => formatter
                .debug_struct("Function")
                .field(
                    "args",
                    &CheckedTypeListDebug {
                        source: self.source,
                        terms: args,
                    },
                )
                .field(
                    "result",
                    &CheckedFlowDebug {
                        source: self.source,
                        mode: result_mode,
                        term: result,
                    },
                )
                .finish(),
            TypeTerm::UnresolvedShape(reason) => formatter
                .debug_struct("UnresolvedShape")
                .field("reason", &self.source.diagnostic_text(reason))
                .finish(),
            TypeTerm::Variable(variable) => formatter
                .debug_tuple("Var")
                .field(&CheckedTypeVarDebug(variable))
                .finish(),
            TypeTerm::Unknown => formatter.write_str("Unknown"),
            TypeTerm::Union(members) => {
                let members = legacy_checked_union_order(self.source, members);
                match members.as_slice() {
                    [] => formatter.write_str("Absent"),
                    [member] => Self::new(self.source, *member).fmt(formatter),
                    members => formatter
                        .debug_tuple("Union")
                        .field(&CheckedTypeListDebug {
                            source: self.source,
                            terms: members,
                        })
                        .finish(),
                }
            }
            TypeTerm::Map { key, value } => formatter
                .debug_struct("Map")
                .field("key", &Self::new(self.source, key))
                .field("value", &Self::new(self.source, value))
                .finish(),
            TypeTerm::Set(item) => formatter
                .debug_tuple("Set")
                .field(&CheckedSharedTypeDebug::new(self.source, item))
                .finish(),
            TypeTerm::Bits(width) => formatter
                .debug_struct("Bits")
                .field("width", &width)
                .finish(),
        }
    }
}

struct CheckedBytesDebug(BytesTerm);

impl fmt::Debug for CheckedBytesDebug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            BytesTerm::Dynamic => formatter.write_str("Dynamic"),
            BytesTerm::Fixed(size) => formatter.debug_tuple("Fixed").field(&size).finish(),
        }
    }
}

struct CheckedTypeVarDebug(TypeVariableId);

impl fmt::Debug for CheckedTypeVarDebug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("TypeVar").field(&self.0.0).finish()
    }
}

struct CheckedSharedTypeDebug<'a>(CheckedProjectionDebug<'a>);

impl<'a> CheckedSharedTypeDebug<'a> {
    const fn new(source: &'a TypeTermArena, term: TypeTermId) -> Self {
        Self(CheckedProjectionDebug::new(source, term))
    }
}

impl fmt::Debug for CheckedSharedTypeDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("SharedType").field(&self.0).finish()
    }
}

struct CheckedFlowDebug<'a> {
    source: &'a TypeTermArena,
    mode: boon_checked::FlowMode,
    term: TypeTermId,
}

impl fmt::Debug for CheckedFlowDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FlowType")
            .field("mode", &self.mode)
            .field("ty", &CheckedProjectionDebug::new(self.source, self.term))
            .finish()
    }
}

struct CheckedTypeListDebug<'a, T: AsRef<[TypeTermId]>> {
    source: &'a TypeTermArena,
    terms: T,
}

impl<T: AsRef<[TypeTermId]>> fmt::Debug for CheckedTypeListDebug<'_, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(
                self.terms
                    .as_ref()
                    .iter()
                    .copied()
                    .map(|term| CheckedProjectionDebug::new(self.source, term)),
            )
            .finish()
    }
}

struct CheckedVariantSetDebug<'a> {
    source: &'a TypeTermArena,
    variants: &'a [crate::VariantTerm],
}

impl fmt::Debug for CheckedVariantSetDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SharedVariantSet")
            .field(&CheckedVariantListDebug {
                source: self.source,
                variants: self.variants,
            })
            .finish()
    }
}

struct CheckedVariantListDebug<'a> {
    source: &'a TypeTermArena,
    variants: &'a [crate::VariantTerm],
}

impl fmt::Debug for CheckedVariantListDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.variants.iter().map(|variant| CheckedVariantDebug {
                source: self.source,
                variant,
            }))
            .finish()
    }
}

struct CheckedVariantDebug<'a> {
    source: &'a TypeTermArena,
    variant: &'a crate::VariantTerm,
}

impl fmt::Debug for CheckedVariantDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.variant {
            crate::VariantTerm::Tag(tag) => formatter
                .debug_tuple("Tag")
                .field(&self.source.name(*tag))
                .finish(),
            crate::VariantTerm::Tagged { tag, fields } => formatter
                .debug_struct("Tagged")
                .field("tag", &self.source.name(*tag))
                .field(
                    "fields",
                    &CheckedObjectFromTermDebug {
                        source: self.source,
                        term: *fields,
                    },
                )
                .finish(),
        }
    }
}

struct CheckedObjectFromTermDebug<'a> {
    source: &'a TypeTermArena,
    term: TypeTermId,
}

impl fmt::Debug for CheckedObjectFromTermDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.source.term(self.term) {
            TypeTerm::Object { fields, open } => CheckedSharedObjectDebug {
                source: self.source,
                fields,
                open,
            }
            .fmt(formatter),
            TypeTerm::OpenObjectPlaceholder => CheckedOpenObjectDebug.fmt(formatter),
            _ => unreachable!("kernel tagged payload is always an object"),
        }
    }
}

struct CheckedSharedObjectDebug<'a> {
    source: &'a TypeTermArena,
    fields: crate::ObjectFields<'a>,
    open: bool,
}

impl fmt::Debug for CheckedSharedObjectDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SharedObjectShape")
            .field(&CheckedObjectShapeDebug {
                source: self.source,
                fields: self.fields,
                open: self.open,
            })
            .finish()
    }
}

struct CheckedOpenObjectDebug;

impl fmt::Debug for CheckedOpenObjectDebug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SharedObjectShape")
            .field(&CheckedEmptyObjectShapeDebug)
            .finish()
    }
}

struct CheckedObjectShapeDebug<'a> {
    source: &'a TypeTermArena,
    fields: crate::ObjectFields<'a>,
    open: bool,
}

impl fmt::Debug for CheckedObjectShapeDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObjectShape")
            .field(
                "fields",
                &CheckedObjectFieldMapDebug {
                    source: self.source,
                    fields: self.fields,
                },
            )
            .field(
                "field_order",
                &CheckedObjectFieldOrderDebug {
                    source: self.source,
                    fields: self.fields,
                },
            )
            .field("open", &self.open)
            .finish()
    }
}

struct CheckedEmptyObjectShapeDebug;

impl fmt::Debug for CheckedEmptyObjectShapeDebug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObjectShape")
            .field("fields", &CheckedEmptyMapDebug)
            .field("field_order", &CheckedEmptyListDebug)
            .field("open", &true)
            .finish()
    }
}

struct CheckedObjectFieldMapDebug<'a> {
    source: &'a TypeTermArena,
    fields: crate::ObjectFields<'a>,
}

impl fmt::Debug for CheckedObjectFieldMapDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = formatter.debug_map();
        for field in self.fields.canonical_iter() {
            map.entry(
                &self.source.name(field.name),
                &CheckedProjectionDebug::new(self.source, field.ty),
            );
        }
        map.finish()
    }
}

struct CheckedObjectFieldOrderDebug<'a> {
    source: &'a TypeTermArena,
    fields: crate::ObjectFields<'a>,
}

impl fmt::Debug for CheckedObjectFieldOrderDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.fields.iter().map(|field| self.source.name(field.name)))
            .finish()
    }
}

struct CheckedEmptyMapDebug;

impl fmt::Debug for CheckedEmptyMapDebug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_map().finish()
    }
}

struct CheckedEmptyListDebug;

impl fmt::Debug for CheckedEmptyListDebug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().finish()
    }
}

#[cfg(test)]
fn definition_flow_terms_digest(
    module_stable_digest: [u8; 32],
    result: KernelArtifactFlowTermV1,
    formals: &[KernelArtifactFlowTermV1],
    expressions: &[KernelArtifactFlowTermV1],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(KERNEL_DEFINITION_FLOW_TERMS_DOMAIN_V1);
    hasher.update(module_stable_digest);
    hasher.update(result.stable_digest);
    update_flow_roots(&mut hasher, formals);
    update_flow_roots(&mut hasher, expressions);
    hasher.finalize().into()
}

#[cfg(test)]
fn update_flow_roots(hasher: &mut Sha256, roots: &[KernelArtifactFlowTermV1]) {
    hasher.update(
        u64::try_from(roots.len())
            .expect("kernel artifact flow root count exceeds u64")
            .to_be_bytes(),
    );
    for root in roots {
        hasher.update(root.stable_digest);
    }
}

fn type_term_hasher() -> Sha256 {
    let mut hasher = Sha256::new();
    hasher.update(ARTIFACT_TYPE_TERM_DOMAIN_V1);
    hasher
}

fn scalar_term_digest(tag: u8) -> [u8; 32] {
    let mut hasher = type_term_hasher();
    hasher.update([tag]);
    hasher.finalize().into()
}

fn flow_digest(mode: FlowMode, term_digest: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(ARTIFACT_FLOW_TERM_DOMAIN_V1);
    hasher.update([flow_mode_tag(mode)]);
    hasher.update(term_digest);
    hasher.finalize().into()
}

fn flow_mode_tag(mode: FlowMode) -> u8 {
    match mode {
        FlowMode::Continuous => 0,
        FlowMode::TickPresent => 1,
        FlowMode::PresentOrAbsent => 2,
        FlowMode::Absent => 3,
    }
}

fn update_same_tag(stable: &mut Sha256, runtime: &mut Sha256, tag: u8) {
    stable.update([tag]);
    runtime.update([tag]);
}

fn update_len_pair(stable: &mut Sha256, runtime: &mut Sha256, value: usize) {
    let value = u64::try_from(value)
        .expect("kernel artifact type collection length exceeds u64")
        .to_be_bytes();
    stable.update(value);
    runtime.update(value);
}

fn update_string_pair(stable: &mut Sha256, runtime: &mut Sha256, value: &str) {
    update_len_pair(stable, runtime, value.len());
    stable.update(value.as_bytes());
    runtime.update(value.as_bytes());
}

#[cfg(test)]
fn update_len(hasher: &mut Sha256, value: usize) {
    hasher.update(
        u64::try_from(value)
            .expect("kernel artifact type collection length exceeds u64")
            .to_be_bytes(),
    );
}

#[cfg(test)]
pub(crate) fn materialize_checked_definition_flow_terms_for_test_v1(
    formals: &[boon_checked::FlowType],
    result: &boon_checked::FlowType,
    expressions: &[boon_checked::FlowType],
) -> KernelDefinitionFlowTermsV1 {
    let mut variables = BTreeMap::new();
    let mut next = 0;
    let formals = formals
        .iter()
        .map(|flow| crate::alpha_normalize_flow_type(flow, &mut variables, &mut next))
        .collect::<Vec<_>>();
    let result = crate::alpha_normalize_flow_type(result, &mut variables, &mut next);
    let expressions = expressions
        .iter()
        .map(|flow| crate::alpha_normalize_flow_type(flow, &mut variables, &mut next))
        .collect::<Vec<_>>();

    let mut builder = ArtifactTypeModuleBuilderV1::new();
    let formals = formals
        .iter()
        .map(|flow| builder.intern_flow(flow))
        .collect::<Result<Vec<_>, _>>()
        .expect("checked test formals produce an artifact type module");
    let result = builder
        .intern_flow(&result)
        .expect("checked test result produces an artifact type module");
    let expressions = expressions
        .iter()
        .map(|flow| builder.intern_flow(flow))
        .collect::<Result<Vec<_>, _>>()
        .expect("checked test expressions produce an artifact type module");
    let module = builder.finish();
    let pack = |flow: ArtifactFlowTermV1| KernelArtifactFlowTermV1 {
        mode: flow.mode,
        term: TypeTermId(flow.term.0),
        stable_digest: flow.stable_digest,
        runtime_erased_digest: flow.runtime_erased_digest,
    };
    let result = pack(result);
    let formals = formals.into_iter().map(pack).collect::<Vec<_>>();
    let expressions = expressions.into_iter().map(pack).collect::<Vec<_>>();
    let stable_digest =
        definition_flow_terms_digest(module.stable_digest, result, &formals, &expressions);
    KernelDefinitionFlowTermsV1 {
        module_stable_digest: module.stable_digest,
        result,
        formals: formals.into_boxed_slice(),
        expressions: expressions.into_boxed_slice(),
        flow_variable_sources_by_ordinal: (0..next).map(TypeVariableId).collect(),
        stable_digest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TypeTermArena, TypeVariableId};
    use boon_checked::{FlowType, ObjectShape, Type, TypeVar};

    #[test]
    fn solver_roots_materialize_exactly_and_alpha_rebase_variables() {
        let mut source = TypeTermArena::new();
        let value_name = source.intern_name("value");
        let variable = source.variable(TypeVariableId(37));
        let record = source.object([(value_name, variable)], true);
        let list = source.list(record);
        let expected_record = Type::object(ObjectShape::from_ordered_fields(
            [("value".to_owned(), Type::Var(TypeVar(0)))],
            true,
        ));

        let mut scratch = DefinitionTermProofScratch::default();
        let artifact = materialize_definition_flow_terms_v1(
            &source,
            &[(variable, FlowMode::Continuous)],
            (list, FlowMode::TickPresent),
            &[
                (record, FlowMode::Continuous),
                (list, FlowMode::TickPresent),
            ],
            &mut scratch,
        )
        .unwrap();

        let mut expected = ArtifactTypeModuleBuilderV1::new();
        let expected_formal = expected
            .intern_flow(&FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Var(TypeVar(0)),
            })
            .unwrap();
        let expected_record_flow = expected
            .intern_flow(&FlowType {
                mode: FlowMode::Continuous,
                ty: expected_record.clone(),
            })
            .unwrap();
        let expected_result = expected
            .intern_flow(&FlowType {
                mode: FlowMode::TickPresent,
                ty: Type::List(Type::shared(expected_record)),
            })
            .unwrap();
        let expected_module = expected.finish();
        assert_eq!(
            artifact.formals[0].stable_digest,
            expected_formal.stable_digest
        );
        assert_eq!(
            artifact.expressions[0].stable_digest,
            expected_record_flow.stable_digest
        );
        assert_eq!(artifact.result.stable_digest, expected_result.stable_digest);
        assert_eq!(artifact.module_stable_digest, expected_module.stable_digest);
        assert_eq!(artifact.result, artifact.expressions[1]);
        assert_eq!(
            artifact.flow_variable_sources_by_ordinal.as_ref(),
            [TypeVariableId(37)]
        );
    }

    #[test]
    fn stable_term_identity_ignores_solver_insertion_order() {
        let build = |insert_unused_first: bool| {
            let mut source = TypeTermArena::new();
            if insert_unused_first {
                let unused = source.intern_name("unused");
                let number = source.number();
                let _ = source.object([(unused, number)], false);
            }
            let value = source.intern_name("value");
            let text = source.text();
            let root = source.object([(value, text)], false);
            let mut scratch = DefinitionTermProofScratch::default();
            let artifact = materialize_definition_flow_terms_v1(
                &source,
                &[],
                (root, FlowMode::Continuous),
                &[(root, FlowMode::Continuous)],
                &mut scratch,
            )
            .unwrap();
            (root, artifact)
        };

        let first = build(false);
        let shifted = build(true);
        assert_ne!(first.0, shifted.0);
        assert_eq!(first.1.result.stable_digest, shifted.1.result.stable_digest);
        assert_eq!(first.1.stable_digest, shifted.1.stable_digest);
    }

    #[test]
    fn object_runtime_digest_matches_rich_projection_with_nonlexical_authored_order() {
        let mut source = TypeTermArena::new();
        let z = source.intern_name("z");
        let a = source.intern_name("a");
        let number = source.number();
        let text = source.text();
        let root = source.object([(z, number), (a, text)], false);
        let mut scratch = DefinitionTermProofScratch::default();
        let direct = materialize_definition_flow_terms_v1(
            &source,
            &[],
            (root, FlowMode::Continuous),
            &[(root, FlowMode::Continuous)],
            &mut scratch,
        )
        .unwrap();

        let rich = FlowType {
            mode: FlowMode::Continuous,
            ty: Type::object(ObjectShape::from_ordered_fields(
                [("z".to_owned(), Type::Number), ("a".to_owned(), Type::Text)],
                false,
            )),
        };
        let mut rich_builder = ArtifactTypeModuleBuilderV1::new();
        let rich_term = rich_builder.intern_flow(&rich).unwrap();
        let rich_module = rich_builder.finish();
        assert_eq!(
            direct.result.runtime_erased_digest, rich_term.runtime_erased_digest,
            "direct kernel and rich checked object projections must publish one runtime identity",
        );
        assert_eq!(direct.module_stable_digest, rich_module.stable_digest);
    }

    #[test]
    fn direct_proofs_match_rich_oracle_for_every_kernel_term_kind() {
        let mut source = TypeTermArena::new();
        let text = source.text();
        let number = source.number();
        let dynamic_bytes = source.bytes(BytesTerm::Dynamic);
        let fixed_bytes = source.bytes(BytesTerm::Fixed(17));
        let absent = source.absent();
        let render_contract = source.render_contract();
        let open_object = source.open_object();
        let empty_open_object = source.object([], true);
        let unknown = source.unknown();
        let variable = source.variable(TypeVariableId(41));
        let bits = source.bits(37);
        let unresolved = source.unresolved_shape("proof-only unresolved shape");

        let z = source.intern_name("z");
        let a = source.intern_name("a");
        let record = source.object([(z, variable), (a, fixed_bytes)], true);
        let tagged_fields = source.object([(a, text), (z, bits)], false);
        let idle = source.variant_tag("Idle");
        let ready = source.tagged_variant("Ready", tagged_fields);
        let variants = source.variant_set_preserving_order([ready, idle]);
        let list = source.list(record);
        let set = source.set(variants);
        let map = source.map(dynamic_bytes, list);
        let union = source.union([number, text, variable, variants]);
        let projection_equivalent_union = source.union([open_object, empty_open_object]);
        let function = source.function([record, set, union], FlowMode::PresentOrAbsent, map);

        let roots = [
            text,
            number,
            dynamic_bytes,
            fixed_bytes,
            absent,
            variants,
            record,
            open_object,
            empty_open_object,
            render_contract,
            list,
            unresolved,
            variable,
            unknown,
            union,
            projection_equivalent_union,
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
        let expression_roots = roots
            .iter()
            .enumerate()
            .map(|(index, term)| (*term, modes[index % modes.len()]))
            .collect::<Vec<_>>();
        for term in roots {
            assert_eq!(
                format!("{:?}", CheckedProjectionDebug::new(&source, term)),
                format!("{:?}", source.export_checked_type(term)),
                "packed checked Debug projection differs for term {term:?}",
            );
        }
        let expression_flows = expression_roots
            .iter()
            .map(|(term, mode)| FlowType {
                mode: *mode,
                ty: source.export_checked_type(*term),
            })
            .collect::<Vec<_>>();
        let formal_roots = [
            (variable, FlowMode::Continuous),
            (record, FlowMode::TickPresent),
        ];
        let formal_flows = formal_roots
            .iter()
            .map(|(term, mode)| FlowType {
                mode: *mode,
                ty: source.export_checked_type(*term),
            })
            .collect::<Vec<_>>();
        let result_root = (function, FlowMode::Absent);
        let result_flow = FlowType {
            mode: result_root.1,
            ty: source.export_checked_type(result_root.0),
        };

        let mut scratch = DefinitionTermProofScratch::default();
        let direct = materialize_definition_flow_terms_v1(
            &source,
            &formal_roots,
            result_root,
            &expression_roots,
            &mut scratch,
        )
        .unwrap();
        let rich = materialize_checked_definition_flow_terms_for_test_v1(
            &formal_flows,
            &result_flow,
            &expression_flows,
        );
        assert_eq!(direct.formals.len(), rich.formals.len());
        assert_eq!(direct.expressions.len(), rich.expressions.len());
        for (index, (direct, rich)) in direct
            .formals
            .iter()
            .zip(rich.formals.iter())
            .chain(direct.expressions.iter().zip(rich.expressions.iter()))
            .enumerate()
        {
            assert_eq!(direct.mode, rich.mode);
            assert_eq!(
                direct.stable_digest, rich.stable_digest,
                "stable root digest {index}"
            );
            assert_eq!(
                direct.runtime_erased_digest, rich.runtime_erased_digest,
                "runtime-erased root digest {index}"
            );
        }
        assert_eq!(direct.result.stable_digest, rich.result.stable_digest);
        assert_eq!(
            direct.result.runtime_erased_digest,
            rich.result.runtime_erased_digest
        );
        assert_eq!(direct.module_stable_digest, rich.module_stable_digest);
        assert_eq!(direct.stable_digest, rich.stable_digest);
    }

    #[test]
    fn packed_checked_debug_preserves_legacy_raw_variable_union_order() {
        let mut source = TypeTermArena::new();
        let variable_2 = source.variable(TypeVariableId(2));
        let variable_10 = source.variable(TypeVariableId(10));
        let open_placeholder = source.open_object();
        let open_object = source.object([], true);
        let variables = source.union([variable_2, variable_10]);
        let projected_duplicates = source.union([open_placeholder, open_object, variables]);

        for term in [variables, projected_duplicates] {
            assert_eq!(
                format!("{:?}", CheckedProjectionDebug::new(&source, term)),
                format!("{:?}", source.export_checked_type(term)),
            );
        }
        assert_eq!(
            legacy_checked_union_order(&source, &[open_placeholder, open_object]).len(),
            1,
            "inference-only and real open-empty objects have one checked projection",
        );
    }
}
