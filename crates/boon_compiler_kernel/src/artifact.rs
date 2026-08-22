use crate::{FrozenTypeStore, FrozenTypeStoreLayout, KernelFlowRef, OutputId, TypeTermArena};
use boon_checked::FlowType;
use std::sync::Arc;

pub const KERNEL_SUMMARY_DEFINITION_RANKING_LEN: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelSummaryDefinitionWork {
    pub definition: u32,
    pub program_evaluations: u64,
    pub node_evaluations: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelSolveWork {
    pub variables: u64,
    /// Coarse scheduler entries. Fully acyclic residual definition frames
    /// contribute one entry regardless of their immutable instruction count.
    pub scheduled_work_items: u64,
    pub operations: u64,
    pub activations: u64,
    pub unify_activations: u64,
    pub publish_activations: u64,
    pub projection_activations: u64,
    pub select_activations: u64,
    pub record_activations: u64,
    /// Immutable definition-summary bytecode nodes actually demanded.
    pub summary_node_evaluations: u64,
    pub summary_definition_ranking:
        [KernelSummaryDefinitionWork; KERNEL_SUMMARY_DEFINITION_RANKING_LEN],
    pub summary_call_activations: u64,
    pub mutations: u64,
    pub union_operations: u64,
    /// Available solver outputs recursively exported to rich compatibility
    /// `FlowType`s. This is an export-event count, not a recursive node count.
    pub rich_output_flow_exports: u64,
    pub term_intern_requests: u64,
    pub term_intern_hits: u64,
    pub term_intern_requests_by_kind: [u64; 8],
    pub term_intern_hits_by_kind: [u64; 8],
    pub nonempty_object_intern_requests: u64,
    pub scratch_vector_misses: u64,
    pub scratch_vector_reuses: u64,
    pub scratch_max_pool_depth: u64,
    pub scratch_retained_capacity_bytes: u64,
    pub structural_widen_requests: u64,
    pub structural_widen_hits: u64,
    pub dynamic_dependency_edges: u64,
    /// Layout of the frozen type authority retained by the published product.
    /// Diagnostics retain an exact reachable-root store; checked demand shares
    /// the complete definition-code store.
    pub frozen_type_store: FrozenTypeStoreLayout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactOutput {
    pub id: OutputId,
    /// Resolved flow qualified by the exact frozen store retained by the
    /// component artifact. No recursive checked type is allocated here.
    pub flow: KernelFlowRef,
    /// Whether this exact runtime occurrence contains a value constructed by
    /// selecting one singleton, invocation-parameter-derived syntax branch.
    pub syntax_selected: bool,
    /// Whether this exact output cell is itself the authored SELECT that
    /// chose a singleton, parameter-derived branch. Unlike `syntax_selected`,
    /// this bit does not propagate through aliases, records, or call inputs.
    pub syntax_selected_here: bool,
    /// Whether the directional writer for this output is a user-definition
    /// summary call whose own result construction selected syntax. This is the
    /// narrow authority for checked-call metadata; ordinary forwarded value
    /// provenance must not relabel a call site.
    pub call_syntax_selected: bool,
}

impl ArtifactOutput {
    pub(crate) const fn term(&self) -> crate::TypeTermId {
        self.flow.term()
    }
}

#[derive(Clone, Debug)]
pub struct ComponentArtifact {
    outputs: Box<[Option<ArtifactOutput>]>,
    // The solved project supports multiple sparse/full materializations from
    // one quiescent graph. Share the frozen solver arena across those cheap
    // snapshot clones; never give its mutable construction caches semantic
    // equality or receipt authority.
    terms: Arc<FrozenTypeStore>,
    pub work: KernelSolveWork,
}

/// Quiescent solved component whose construction indexes remain available for
/// definition finalization.
///
/// FLUSH, call-substitution, resource, and diagnostic roots must be interned
/// here before the one permanent type-store freeze. This value is never
/// shared or retained by a compiler session.
#[derive(Debug)]
pub(crate) struct UnsealedComponentArtifact {
    outputs: Box<[Option<ArtifactOutput>]>,
    terms: TypeTermArena,
    work: KernelSolveWork,
}

/// Phase-local packed view over demanded outputs in the live solver arena.
///
/// Diagnostics may add derived call-instantiation roots while this borrow is
/// active, but the arena remains owned by the staged solve session.  The
/// final interface projector copies only retained roots into its immutable
/// snapshot, so diagnostics-to-checked promotion neither freezes nor clones
/// the live solver graph.
#[derive(Debug)]
pub(crate) struct ComponentOutputSnapshot<'a> {
    outputs: Box<[Option<ArtifactOutput>]>,
    terms: &'a mut TypeTermArena,
    pub work: KernelSolveWork,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ArtifactOutputFlags {
    pub(crate) syntax_selected_here: bool,
    pub(crate) call_syntax_selected: bool,
}

impl<'a> ComponentOutputSnapshot<'a> {
    pub(crate) fn new(
        outputs: Box<[Option<ArtifactOutput>]>,
        terms: &'a mut TypeTermArena,
        work: KernelSolveWork,
    ) -> Self {
        Self {
            outputs,
            terms,
            work,
        }
    }

    /// Borrow a sparse packed view while the complete artifact remains
    /// mutable. Derived roots are interned through [`Self::terms_mut`] before
    /// the one permanent freeze.
    pub(crate) fn project_unsealed(artifact: &'a mut UnsealedComponentArtifact) -> Self {
        // A complete solve has already made every output available and later
        // definition-code publication consumes all of them. Borrow the flat
        // rows directly; a second sparse-demand calculation would omit
        // occurrence-only syntax flags without reducing the boxed column.
        let outputs = artifact.outputs.clone();
        let work = artifact.work;
        Self::new(outputs, &mut artifact.terms, work)
    }

    #[cfg(test)]
    pub(crate) fn available_output_count(&self) -> usize {
        self.outputs
            .iter()
            .filter(|output| output.is_some())
            .count()
    }

    pub(crate) fn flow(&self, id: OutputId) -> Option<crate::PackedFlow> {
        self.outputs
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .filter(|output| output.id == id)
            .map(|output| crate::PackedFlow {
                mode: output.flow.mode(),
                term: output.term(),
            })
    }

    pub(crate) fn term(&self, id: OutputId) -> Option<crate::TypeTermId> {
        self.flow(id).map(|flow| flow.term)
    }

    pub(crate) fn output_flags(&self, id: OutputId) -> Option<ArtifactOutputFlags> {
        self.outputs
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .filter(|output| output.id == id)
            .map(|output| ArtifactOutputFlags {
                syntax_selected_here: output.syntax_selected_here,
                call_syntax_selected: output.call_syntax_selected,
            })
    }

    pub(crate) const fn work(&self) -> KernelSolveWork {
        self.work
    }

    pub(crate) fn terms(&self) -> &TypeTermArena {
        self.terms
    }

    pub(crate) fn terms_mut(&mut self) -> &mut TypeTermArena {
        self.terms
    }
}

impl ComponentArtifact {
    pub fn outputs(&self) -> impl Iterator<Item = &ArtifactOutput> {
        self.outputs.iter().filter_map(Option::as_ref)
    }

    pub fn output(&self, id: OutputId) -> Option<&ArtifactOutput> {
        let index = id.0 as usize;
        self.outputs
            .get(index)
            .and_then(Option::as_ref)
            .filter(|output| output.id == id)
    }

    /// Explicit rich projection for compatibility consumers and tests.
    pub fn output_flow(&self, id: OutputId) -> Option<FlowType> {
        let output = self.output(id)?;
        self.terms.materialize_flow(output.flow)
    }

    pub fn available_output_count(&self) -> usize {
        self.outputs
            .iter()
            .filter(|output| output.is_some())
            .count()
    }

    /// Share the one immutable type/symbol store retained by this solved
    /// component. Snapshot publication clones only this `Arc`.
    pub(crate) fn type_store(&self) -> Arc<FrozenTypeStore> {
        Arc::clone(&self.terms)
    }
}

impl UnsealedComponentArtifact {
    pub(crate) fn new(
        outputs: Box<[Option<ArtifactOutput>]>,
        terms: TypeTermArena,
        work: KernelSolveWork,
    ) -> Self {
        Self {
            outputs,
            terms,
            work,
        }
    }

    pub(crate) fn output(&self, id: OutputId) -> Option<&ArtifactOutput> {
        self.outputs
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .filter(|output| output.id == id)
    }

    pub(crate) fn terms(&self) -> &TypeTermArena {
        &self.terms
    }

    pub(crate) fn terms_mut(&mut self) -> &mut TypeTermArena {
        &mut self.terms
    }

    pub(crate) fn seal(mut self) -> ComponentArtifact {
        let terms = Arc::new(self.terms.freeze());
        self.work.frozen_type_store = terms.layout();
        ComponentArtifact {
            outputs: self.outputs,
            terms,
            work: self.work,
        }
    }
}
