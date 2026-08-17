use crate::{OutputId, TypeTermArena, TypeTermId};
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
    pub term_materializations: u64,
    pub term_intern_requests: u64,
    pub term_intern_hits: u64,
    pub term_intern_requests_by_kind: [u64; 8],
    pub term_intern_hits_by_kind: [u64; 8],
    pub structural_widen_requests: u64,
    pub structural_widen_hits: u64,
    pub dynamic_dependency_edges: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactOutput {
    pub id: OutputId,
    /// Resolved solver-arena term retained until definition finalization.
    /// This ID is meaningful only together with `ComponentArtifact::terms`;
    /// it is never a stable receipt identity.
    pub term: TypeTermId,
    pub flow_type: FlowType,
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

#[derive(Clone, Debug)]
pub struct ComponentArtifact {
    outputs: Box<[Option<ArtifactOutput>]>,
    // The solved project supports multiple sparse/full materializations from
    // one quiescent graph. Share the frozen solver arena across those cheap
    // snapshot clones; never give its mutable construction caches semantic
    // equality or receipt authority.
    terms: Arc<TypeTermArena>,
    pub work: KernelSolveWork,
}

/// Lean output-only view used by diagnostics before checked definition terms
/// are demanded. It intentionally owns no clone of the solved type arena.
#[derive(Clone, Debug)]
pub(crate) struct ComponentOutputSnapshot {
    outputs: Box<[Option<ArtifactOutput>]>,
    pub work: KernelSolveWork,
}

pub(crate) trait ComponentOutputs {
    fn output(&self, id: OutputId) -> Option<&ArtifactOutput>;
    fn work(&self) -> KernelSolveWork;
}

impl ComponentOutputSnapshot {
    pub(crate) fn new(outputs: Box<[Option<ArtifactOutput>]>, work: KernelSolveWork) -> Self {
        Self { outputs, work }
    }

    #[cfg(test)]
    pub(crate) fn available_output_count(&self) -> usize {
        self.outputs
            .iter()
            .filter(|output| output.is_some())
            .count()
    }
}

impl ComponentOutputs for ComponentOutputSnapshot {
    fn output(&self, id: OutputId) -> Option<&ArtifactOutput> {
        self.outputs
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .filter(|output| output.id == id)
    }

    fn work(&self) -> KernelSolveWork {
        self.work
    }
}

impl ComponentOutputs for ComponentArtifact {
    fn output(&self, id: OutputId) -> Option<&ArtifactOutput> {
        Self::output(self, id)
    }

    fn work(&self) -> KernelSolveWork {
        self.work
    }
}

impl ComponentArtifact {
    pub(crate) fn new(
        outputs: Box<[Option<ArtifactOutput>]>,
        terms: TypeTermArena,
        work: KernelSolveWork,
    ) -> Self {
        Self {
            outputs,
            terms: Arc::new(terms),
            work,
        }
    }

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

    pub fn available_output_count(&self) -> usize {
        self.outputs
            .iter()
            .filter(|output| output.is_some())
            .count()
    }

    pub fn terms(&self) -> &TypeTermArena {
        self.terms.as_ref()
    }
}
