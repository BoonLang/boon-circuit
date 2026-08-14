use crate::OutputId;
use boon_checked::FlowType;

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
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentArtifact {
    outputs: Box<[ArtifactOutput]>,
    pub work: KernelSolveWork,
}

impl ComponentArtifact {
    pub(crate) fn new(outputs: Box<[ArtifactOutput]>, work: KernelSolveWork) -> Self {
        Self { outputs, work }
    }

    pub fn outputs(&self) -> &[ArtifactOutput] {
        &self.outputs
    }

    pub fn output(&self, id: OutputId) -> Option<&ArtifactOutput> {
        self.outputs
            .get(id.0 as usize)
            .filter(|output| output.id == id)
    }
}
