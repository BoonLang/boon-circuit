//! Compile-time elaboration of checked `OUT` bindings.
//!
//! The resolved graph is owned by [`crate::SemanticProgram`]. It retains
//! checked provenance and the complete static-owner forest, but it is neither
//! executable IR nor a runtime value.

use crate::ProducerMaterializationMode;
use boon_checked::{
    CheckedCall, CheckedCallEntry, CheckedCallId, CheckedCallableKind, CheckedCallableSignature,
    CheckedContextBinding, CheckedDeclaration, CheckedDeclarationKind, CheckedEvaluationScope,
    CheckedExprId, CheckedExpressionKind, CheckedMatchPattern, CheckedPassedAccess,
    CheckedPatternBinding, CheckedProgramFields, CheckedScopeKind, CheckedTypeSubstitution,
    CheckedTypeSubstitutionFrameLookup, ContextFormalId, DeclId, FlowType, LexicalScopeId, Type,
    TypeVar, apply_checked_type_substitution_frames, apply_checked_type_substitutions_once,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

macro_rules! typed_out_id {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(
                Clone,
                Copy,
                Debug,
                Eq,
                Hash,
                Ord,
                PartialEq,
                PartialOrd,
                Serialize,
                Deserialize,
            )]
            #[serde(transparent)]
            pub struct $name(pub usize);

            impl $name {
                pub const fn as_usize(self) -> usize {
                    self.0
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.0.fmt(formatter)
                }
            }
        )+
    };
}

typed_out_id!(
    OutCallInstanceId,
    OutPortId,
    OutNetId,
    ProducerFunctionId,
    ProducerResultStatementId,
    StaticOwnerId,
);

impl OutCallInstanceId {
    pub const fn from_usize(value: usize) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ProducerParameterId {
    pub function: ProducerFunctionId,
    pub ordinal: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StaticOwnerDef {
    pub id: StaticOwnerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<StaticOwnerId>,
    pub child_ordinal: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum DistributedCallOccurrenceRoot {
    Program,
    Producer([u8; 32]),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProducerRootParameter {
    pub formal: DeclId,
    pub parameter: ProducerParameterId,
    pub name: String,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProducerRootSpec {
    pub identity: [u8; 32],
    pub mode: ProducerMaterializationMode,
    pub callable: DeclId,
    pub function: ProducerFunctionId,
    pub function_name: String,
    pub result_statement: ProducerResultStatementId,
    pub result_declaration: DeclId,
    pub result_path: String,
    pub result_type: FlowType,
    pub parameters: Vec<ProducerRootParameter>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProducerRoot {
    pub spec: ProducerRootSpec,
    pub call: OutCallInstanceId,
}

/// Stable checked-program coordinates for one static call site.
///
/// The contextual binding is deliberately excluded: `PASS` is context for
/// expansion, not part of executable ownership.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OutCallProvenance {
    pub call_id: Option<CheckedCallId>,
    pub expression: CheckedExprId,
    pub owner_callable: Option<DeclId>,
    pub callable: DeclId,
}

impl From<&CheckedCall> for OutCallProvenance {
    fn from(call: &CheckedCall) -> Self {
        Self {
            call_id: Some(call.id),
            expression: call.expression,
            owner_callable: call.owner_callable,
            callable: call.callable,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OutCallInstance {
    pub id: OutCallInstanceId,
    pub parent: Option<OutCallInstanceId>,
    pub provenance: OutCallProvenance,
    pub parent_output: Option<DeclId>,
    parent_output_node: Option<usize>,
    pub inputs: Vec<OutInputBinding>,
    pub passed: Option<PassedBinding>,
    pub ports: Vec<OutPortId>,
    /// Substitutions introduced by this checked call only. Inherited entries
    /// remain owned by `parent`; retaining their full structural types in
    /// every descendant makes deep generic call graphs quadratic in memory.
    local_type_substitutions: Vec<CheckedTypeSubstitution>,
    /// Local keys with values resolved once through their parent invocation.
    /// This remains proportional to local call facts and never copies an
    /// ancestor key set into a descendant frame.
    #[serde(skip)]
    resolved_type_substitutions: Vec<CheckedTypeSubstitution>,
    #[serde(skip)]
    type_substitution_count: usize,
    pub result: FlowType,
    /// This frame carries an exact value-level result occurrence selected by
    /// syntax in this call or inherited through the callable's result path.
    pub result_is_exact_occurrence: bool,
    /// Present only when this concrete user call directly allocates runtime
    /// resources. Pure forwarding wrappers deliberately have no owner.
    pub owner: Option<StaticOwnerId>,
}

impl OutCallInstance {
    pub fn local_type_substitutions(&self) -> &[CheckedTypeSubstitution] {
        &self.local_type_substitutions
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PassedBinding {
    pub formal: ContextFormalId,
    pub value: ScopedCheckedExpr,
    pub evaluation_call: OutCallInstanceId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ScopedCheckedExpr {
    pub expression: CheckedExprId,
    /// The concrete user-call frame in which this expression was written.
    pub frame: Option<OutCallInstanceId>,
    /// A call-local output formal under which this argument is evaluated.
    pub evaluation_port: Option<OutPortId>,
    /// A standalone pure-function binding frame used outside a concrete call site.
    pub value_frame: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OutInputBinding {
    pub formal: DeclId,
    pub value: OutInputValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum OutInputValue {
    Checked(ScopedCheckedExpr),
    ProducerParameter {
        parameter: ProducerParameterId,
        flow_type: FlowType,
    },
}

impl OutInputBinding {
    pub fn checked_value(&self) -> Option<ScopedCheckedExpr> {
        match &self.value {
            OutInputValue::Checked(value) => Some(*value),
            OutInputValue::ProducerParameter { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum OutPortBinding {
    Fresh {
        output: DeclId,
        scope_id: LexicalScopeId,
    },
    Forward {
        target: DeclId,
    },
}

/// One output formal instantiated at one concrete call site.
///
/// `Contract` is currently `()` for the public checked schema. Keeping it on
/// the port lets type/shape/scope/role/generation/correlation facts be attached
/// without changing graph identity or the unification algorithm.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OutPort<Contract = ()> {
    pub id: OutPortId,
    pub call: OutCallInstanceId,
    pub entry_ordinal: usize,
    pub formal: DeclId,
    pub name: String,
    pub binding: OutPortBinding,
    pub contract: Contract,
    pub net: OutNetId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StructuralProducer {
    pub port: OutPortId,
    pub call: OutCallInstanceId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnifiedOutNet {
    pub id: OutNetId,
    pub ports: Vec<OutPortId>,
    pub producers: Vec<StructuralProducer>,
    pub owner: Option<StaticOwnerId>,
    pub owner_anchor: Option<OutPortId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OutNet<Contract = ()> {
    pub call_instances: Vec<OutCallInstance>,
    pub ports: Vec<OutPort<Contract>>,
    pub nets: Vec<UnifiedOutNet>,
    pub static_owners: Vec<StaticOwnerDef>,
    call_instance_by_checked_frame:
        BTreeMap<(CheckedCallId, Option<OutCallInstanceId>), Option<OutCallInstanceId>>,
    #[serde(skip)]
    intentionally_elided_calls: BTreeSet<(CheckedCallId, Option<OutCallInstanceId>)>,
    #[serde(skip)]
    intentionally_elided_frames: BTreeSet<OutCallInstanceId>,
    output_net_by_frame_target: BTreeMap<(Option<OutCallInstanceId>, DeclId), Option<OutNetId>>,
    concrete_producers_by_checked: BTreeMap<CheckedCallId, Vec<ConcreteOutProducer>>,
    producer_roots: Vec<ProducerRoot>,
    producer_root_by_identity: BTreeMap<[u8; 32], OutCallInstanceId>,
    producer_root_calls: BTreeSet<OutCallInstanceId>,
}

struct OutTypeSubstitutionFrames<'a>(&'a [OutCallInstance]);

impl CheckedTypeSubstitutionFrameLookup for OutTypeSubstitutionFrames<'_> {
    type Frame = OutCallInstanceId;

    fn parent(&self, frame: Self::Frame) -> Option<Self::Frame> {
        self.0
            .get(frame.as_usize())
            .filter(|instance| instance.id == frame)
            .and_then(|instance| instance.parent)
    }

    fn substitutions(&self, frame: Self::Frame) -> &[CheckedTypeSubstitution] {
        self.0
            .get(frame.as_usize())
            .filter(|instance| instance.id == frame)
            .map_or(&[], |instance| instance.local_type_substitutions())
    }

    fn frame_count(&self) -> usize {
        self.0.len()
    }
}

fn apply_type_substitution_frames(
    call_instances: &[OutCallInstance],
    frame: Option<OutCallInstanceId>,
    ty: &Type,
) -> Type {
    frame.map_or_else(
        || ty.clone(),
        |frame| {
            apply_checked_type_substitution_frames(
                ty,
                frame,
                &OutTypeSubstitutionFrames(call_instances),
            )
        },
    )
}

/// Chooses the already-finalized checked occurrence when the instantiated
/// callable result denotes exactly the same fully concrete value set.
///
/// Call-frame substitution must retain transient union shape because its
/// members can still belong to independently normalized alpha namespaces.
/// After both sides are recursively closed, mutual assignability proves that
/// the checked occurrence is an equivalent representation supplied by the
/// typechecker rather than a narrowing guess. This prevents a redundant
/// instantiated `Text | Text` from escaping instead of the exact checked
/// occurrence `Text` without inventing a new representation in OutNet.
fn specialize_checked_occurrence_result(instantiated: &Type, occurrence: &Type) -> Type {
    if boon_checked::type_is_recursively_closed(instantiated)
        && boon_checked::type_is_recursively_closed(occurrence)
        && boon_checked::resolved_type_is_assignable_to(instantiated, occurrence)
        && boon_checked::resolved_type_is_assignable_to(occurrence, instantiated)
    {
        occurrence.clone()
    } else {
        boon_checked::specialize_checked_call_result(instantiated, occurrence)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ConcreteOutProducer {
    pub call: OutCallInstanceId,
    pub port: OutPortId,
    pub net: OutNetId,
    pub owner: StaticOwnerId,
}

impl<Contract> OutNet<Contract> {
    #[cfg(test)]
    pub(crate) fn empty_for_tests() -> Self {
        Self {
            call_instances: Vec::new(),
            ports: Vec::new(),
            nets: Vec::new(),
            static_owners: Vec::new(),
            call_instance_by_checked_frame: BTreeMap::new(),
            intentionally_elided_calls: BTreeSet::new(),
            intentionally_elided_frames: BTreeSet::new(),
            output_net_by_frame_target: BTreeMap::new(),
            concrete_producers_by_checked: BTreeMap::new(),
            producer_roots: Vec::new(),
            producer_root_by_identity: BTreeMap::new(),
            producer_root_calls: BTreeSet::new(),
        }
    }

    pub fn producer_root_result_path(&self, call: OutCallInstanceId) -> Option<&str> {
        self.producer_roots
            .iter()
            .find(|root| root.call == call)
            .map(|root| root.spec.result_path.as_str())
    }

    pub fn call_instance_for_checked_call(
        &self,
        call_id: CheckedCallId,
        frame: Option<OutCallInstanceId>,
    ) -> Option<OutCallInstanceId> {
        self.call_instance_by_checked_frame
            .get(&(call_id, frame))
            .copied()
            .flatten()
    }

    pub(crate) fn has_checked_call_frame_index_entry(
        &self,
        call_id: CheckedCallId,
        frame: Option<OutCallInstanceId>,
    ) -> bool {
        self.call_instance_by_checked_frame
            .contains_key(&(call_id, frame))
    }

    pub(crate) fn intentionally_elided_call(
        &self,
        call_id: CheckedCallId,
        owner_callable: Option<DeclId>,
        frame: Option<OutCallInstanceId>,
    ) -> bool {
        if self.has_checked_call_frame_index_entry(call_id, frame) {
            return false;
        }
        self.intentionally_elided_calls.contains(&(call_id, frame))
            || frame.is_some_and(|frame| {
                self.intentionally_elided_frames.contains(&frame)
                    && self
                        .call_instances
                        .get(frame.as_usize())
                        .is_some_and(|instance| {
                            instance.id == frame
                                && owner_callable == Some(instance.provenance.callable)
                        })
            })
    }

    pub fn net_for_port(&self, port: OutPortId) -> OutNetId {
        self.ports[port.as_usize()].net
    }

    pub fn owner_for_net(&self, net: OutNetId) -> Option<StaticOwnerId> {
        self.nets[net.as_usize()].owner
    }

    pub fn owner_for_call(&self, call: OutCallInstanceId) -> Option<StaticOwnerId> {
        self.call_instances[call.as_usize()].owner
    }

    /// Reconstructs this call's exact local scheme-to-root environment.
    ///
    /// Raw `TypeVar` ordinals from ancestor call schemes are not globally
    /// unique, so their maps cannot be flattened into one `BTreeMap`. Values
    /// are instead composed through ordered one-shot parent frames while the
    /// returned keys remain solely in this call's scheme namespace.
    pub fn type_substitution_environment(
        &self,
        call: OutCallInstanceId,
    ) -> BTreeMap<TypeVar, Type> {
        let mut environment = BTreeMap::new();
        let Some(instance) = self
            .call_instances
            .get(call.as_usize())
            .filter(|candidate| candidate.id == call)
        else {
            return environment;
        };
        debug_assert_eq!(
            instance.local_type_substitutions.len(),
            instance.resolved_type_substitutions.len()
        );
        for substitution in &instance.resolved_type_substitutions {
            environment.insert(substitution.variable, substitution.value.clone());
        }
        environment
    }

    #[cfg(test)]
    pub(crate) fn push_local_type_substitution_for_test(
        &mut self,
        call: OutCallInstanceId,
        substitution: CheckedTypeSubstitution,
    ) {
        let parent = self.call_instances[call.as_usize()].parent;
        let resolved = CheckedTypeSubstitution {
            variable: substitution.variable,
            value: apply_type_substitution_frames(
                &self.call_instances,
                parent,
                &substitution.value,
            ),
        };
        let instance = &mut self.call_instances[call.as_usize()];
        instance.local_type_substitutions.push(substitution);
        instance.resolved_type_substitutions.push(resolved);
        instance.type_substitution_count = instance.type_substitution_count.saturating_add(1);
    }

    pub fn type_substitution_count(&self, call: OutCallInstanceId) -> usize {
        self.call_instances
            .get(call.as_usize())
            .filter(|candidate| candidate.id == call)
            .map_or(0, |instance| instance.type_substitution_count)
    }

    pub fn apply_type_substitutions(&self, call: OutCallInstanceId, ty: &Type) -> Type {
        apply_type_substitution_frames(&self.call_instances, Some(call), ty)
    }

    pub fn owner_for_call_evaluation(&self, mut call: OutCallInstanceId) -> Option<StaticOwnerId> {
        // Producer roots are synthetic distributed call-site boundaries. Their
        // parameter imports belong to the instance slab rather than to a
        // lexical caller outside that slab.
        if self.producer_root_calls.contains(&call) {
            return self.owner_for_call(call);
        }
        let mut remaining = self.call_instances.len().saturating_add(1);
        loop {
            if remaining == 0 {
                return None;
            }
            remaining -= 1;
            let instance = self.call_instances.get(call.as_usize())?;
            if let Some(output) = instance.parent_output {
                return self
                    .output_net_in_frame(instance.parent, output)
                    .and_then(|net| self.owner_for_net(net));
            }
            let parent = instance.parent?;
            if let Some(owner) = self.owner_for_call(parent) {
                return Some(owner);
            }
            call = parent;
        }
    }

    pub fn distributed_call_occurrence(
        &self,
        program: &CheckedProgramFields,
        instance: OutCallInstanceId,
    ) -> Result<(DistributedCallOccurrenceRoot, String), String> {
        let mut ancestry = Vec::new();
        let mut next = Some(instance);
        let mut remaining = self.call_instances.len().saturating_add(1);
        while let Some(call) = next {
            if remaining == 0 {
                return Err(format!(
                    "OUT call instance {instance} has cyclic concrete ancestry"
                ));
            }
            remaining -= 1;
            let concrete = self
                .call_instances
                .get(call.as_usize())
                .filter(|candidate| candidate.id == call)
                .ok_or_else(|| format!("OUT call instance {call} is missing"))?;
            ancestry.push(call);
            next = concrete.parent;
        }
        ancestry.reverse();

        let producer_root = ancestry.first().and_then(|root| {
            self.producer_root_by_identity
                .iter()
                .find_map(|(identity, candidate)| (*candidate == *root).then_some(*identity))
        });
        let root = producer_root
            .map(DistributedCallOccurrenceRoot::Producer)
            .unwrap_or(DistributedCallOccurrenceRoot::Program);
        let mut path = match root {
            DistributedCallOccurrenceRoot::Program => "program".to_owned(),
            DistributedCallOccurrenceRoot::Producer(identity) => {
                format!("producer:{}", producer_identity_text(identity))
            }
        };
        let first_static = usize::from(producer_root.is_some());
        for call in ancestry.into_iter().skip(first_static) {
            let checked = self.call_instances[call.as_usize()]
                .provenance
                .call_id
                .ok_or_else(|| {
                    format!("non-root OUT call instance {call} has no checked call provenance")
                })?;
            path.push('/');
            path.push_str(&checked_call_occurrence_segment(program, checked)?);
        }
        Ok((root, path))
    }

    pub fn owner_scope_for_net(&self, net: OutNetId) -> Option<LexicalScopeId> {
        let anchor = self.nets[net.as_usize()].owner_anchor?;
        match self.ports[anchor.as_usize()].binding {
            OutPortBinding::Fresh { scope_id, .. } => Some(scope_id),
            OutPortBinding::Forward { .. } => None,
        }
    }

    pub fn concrete_producers_for_checked_call(
        &self,
        call_id: CheckedCallId,
    ) -> Vec<ConcreteOutProducer> {
        self.concrete_producers_by_checked
            .get(&call_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn output_net_in_frame(
        &self,
        frame: Option<OutCallInstanceId>,
        target: DeclId,
    ) -> Option<OutNetId> {
        self.output_net_by_frame_target
            .get(&(frame, target))
            .copied()
            .flatten()
    }

    pub fn producer_roots(&self) -> &[ProducerRoot] {
        &self.producer_roots
    }
}

fn producer_identity_text(identity: [u8; 32]) -> String {
    identity.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn checked_call_occurrence_segment(
    program: &CheckedProgramFields,
    call_id: CheckedCallId,
) -> Result<String, String> {
    program
        .calls
        .iter()
        .find(|candidate| candidate.id == call_id)
        .ok_or_else(|| format!("checked call {} is missing", call_id.0))?;
    Ok(format!("call:{}", call_id.0))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OutNetBuild<Contract = ()> {
    pub(crate) graph: OutNet<Contract>,
    pub(crate) diagnostics: Vec<OutNetDiagnostic>,
}

impl<Contract> OutNetBuild<Contract> {
    pub fn has_errors(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum OutNetDiagnostic {
    AliasCycle {
        declarations: Vec<DeclId>,
        call_sites: Vec<OutCallProvenance>,
    },
    MissingProducer {
        net: OutNetId,
        ports: Vec<OutPortId>,
    },
    MultipleProducers {
        net: OutNetId,
        producers: Vec<OutPortId>,
    },
    MissingOwnerAnchor {
        net: OutNetId,
    },
    MultipleOwnerAnchors {
        net: OutNetId,
        anchors: Vec<OutPortId>,
    },
    UnknownParentOutput {
        call: OutCallInstanceId,
        output: DeclId,
    },
    OwnerCycle {
        net: OutNetId,
    },
    UnknownForwardTarget {
        call: OutCallInstanceId,
        target: DeclId,
    },
    DuplicateFreshOutput {
        call: OutCallInstanceId,
        output: DeclId,
    },
    DuplicateFormalBinding {
        call: OutCallInstanceId,
        formal: DeclId,
    },
    MissingCallable {
        call: OutCallInstanceId,
        callable: DeclId,
    },
    MissingPassedContext {
        call: OutCallInstanceId,
        callable: DeclId,
    },
    RecursiveContextualCall {
        call: OutCallInstanceId,
        callable: DeclId,
    },
    InvalidDefinitionTemplate {
        callable: DeclId,
        reason: String,
    },
}

impl fmt::Display for OutNetDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AliasCycle { declarations, .. } => write!(
                formatter,
                "OUT forwarding cycle across declarations {}",
                display_decl_ids(declarations)
            ),
            Self::MissingProducer { net, .. } => {
                write!(formatter, "OUT net {net} has no structural producer")
            }
            Self::MultipleProducers { net, producers } => write!(
                formatter,
                "OUT net {net} has {} structural producers; exactly one is required",
                producers.len()
            ),
            Self::MissingOwnerAnchor { net } => {
                write!(
                    formatter,
                    "OUT net {net} has no fresh structural owner anchor"
                )
            }
            Self::MultipleOwnerAnchors { net, anchors } => write!(
                formatter,
                "OUT net {net} has {} fresh structural owner anchors; exactly one is required",
                anchors.len()
            ),
            Self::UnknownParentOutput { call, output } => write!(
                formatter,
                "OUT call instance {call} is nested under unresolved output declaration {}",
                output.0
            ),
            Self::OwnerCycle { net } => {
                write!(
                    formatter,
                    "OUT net {net} is its own structural owner parent"
                )
            }
            Self::UnknownForwardTarget { call, target } => write!(
                formatter,
                "OUT call instance {call} forwards to unknown declaration {}",
                target.0
            ),
            Self::DuplicateFreshOutput { call, output } => write!(
                formatter,
                "OUT call instance {call} allocates declaration {} more than once",
                output.0
            ),
            Self::DuplicateFormalBinding { call, formal } => write!(
                formatter,
                "OUT call instance {call} binds formal declaration {} more than once",
                formal.0
            ),
            Self::MissingCallable { call, callable } => write!(
                formatter,
                "OUT call instance {call} references missing callable declaration {}",
                callable.0
            ),
            Self::MissingPassedContext { call, callable } => write!(
                formatter,
                "OUT call instance {call} requires PASS for callable declaration {} but has no explicit or inherited context",
                callable.0
            ),
            Self::RecursiveContextualCall { call, callable } => write!(
                formatter,
                "OUT call instance {call} recursively expands callable declaration {}",
                callable.0
            ),
            Self::InvalidDefinitionTemplate { callable, reason } => write!(
                formatter,
                "OUT definition template for callable {} is invalid: {reason}",
                callable.0
            ),
        }
    }
}

fn display_decl_ids(declarations: &[DeclId]) -> String {
    declarations
        .iter()
        .map(|declaration| declaration.0.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn insert_unique_index<K: Ord, V: Copy + Eq>(index: &mut BTreeMap<K, Option<V>>, key: K, value: V) {
    index
        .entry(key)
        .and_modify(|existing| {
            if existing.is_some_and(|existing| existing != value) {
                *existing = None;
            }
        })
        .or_insert(Some(value));
}

impl OutNet<()> {
    #[cfg(test)]
    pub(crate) fn build(program: &CheckedProgramFields) -> OutNetBuild {
        Self::build_with_producer_roots(program, Vec::new())
    }

    #[cfg(test)]
    pub(crate) fn build_with_producer_roots(
        program: &CheckedProgramFields,
        producer_roots: Vec<ProducerRootSpec>,
    ) -> OutNetBuild {
        Self::build_with(
            program,
            producer_roots,
            |_, _, _| (),
            |kind, _, _, _, _| kind == CheckedCallableKind::Builtin,
        )
    }
}

impl<Contract> OutNet<Contract> {
    /// Builds an `OutNet` while allowing richer checked contracts and producer
    /// capabilities to be supplied by a later schema without changing the
    /// current `CheckedProgram` adapter.
    #[cfg(test)]
    pub(crate) fn build_with<MakeContract, IsProducer>(
        program: &CheckedProgramFields,
        producer_roots: Vec<ProducerRootSpec>,
        make_contract: MakeContract,
        is_structural_producer: IsProducer,
    ) -> OutNetBuild<Contract>
    where
        MakeContract: FnMut(&CheckedCall, usize, &CheckedCallEntry) -> Contract,
        IsProducer:
            FnMut(CheckedCallableKind, &CheckedCall, usize, &CheckedCallEntry, &Contract) -> bool,
    {
        Self::build_with_retained_definitions(
            program,
            producer_roots,
            &BTreeSet::new(),
            make_contract,
            is_structural_producer,
        )
    }

    #[cfg(test)]
    pub(crate) fn build_with_retained_definitions<MakeContract, IsProducer>(
        program: &CheckedProgramFields,
        producer_roots: Vec<ProducerRootSpec>,
        retained_definitions: &BTreeSet<DeclId>,
        make_contract: MakeContract,
        is_structural_producer: IsProducer,
    ) -> OutNetBuild<Contract>
    where
        MakeContract: FnMut(&CheckedCall, usize, &CheckedCallEntry) -> Contract,
        IsProducer:
            FnMut(CheckedCallableKind, &CheckedCall, usize, &CheckedCallEntry, &Contract) -> bool,
    {
        let intent = crate::verified_intent::VerifiedSemanticIntentV1::build(
            program,
            &producer_roots,
            retained_definitions.clone(),
        )
        .expect("checked OUT test fixture has valid verified intent");
        OutNetBuilder::new(
            program,
            producer_roots,
            &intent,
            make_contract,
            is_structural_producer,
        )
        .build()
    }

    #[cfg(test)]
    pub(crate) fn try_build_with<MakeContract, IsProducer, BuildError>(
        program: &CheckedProgramFields,
        producer_roots: Vec<ProducerRootSpec>,
        make_contract: MakeContract,
        is_structural_producer: IsProducer,
    ) -> Result<OutNetBuild<Contract>, BuildError>
    where
        MakeContract: FnMut(&CheckedCall, usize, &CheckedCallEntry) -> Result<Contract, BuildError>,
        IsProducer:
            FnMut(CheckedCallableKind, &CheckedCall, usize, &CheckedCallEntry, &Contract) -> bool,
    {
        Self::try_build_with_retained_definitions(
            program,
            producer_roots,
            &BTreeSet::new(),
            make_contract,
            is_structural_producer,
        )
    }

    #[cfg(test)]
    pub(crate) fn try_build_with_retained_definitions<MakeContract, IsProducer, BuildError>(
        program: &CheckedProgramFields,
        producer_roots: Vec<ProducerRootSpec>,
        retained_definitions: &BTreeSet<DeclId>,
        make_contract: MakeContract,
        is_structural_producer: IsProducer,
    ) -> Result<OutNetBuild<Contract>, BuildError>
    where
        MakeContract: FnMut(&CheckedCall, usize, &CheckedCallEntry) -> Result<Contract, BuildError>,
        IsProducer:
            FnMut(CheckedCallableKind, &CheckedCall, usize, &CheckedCallEntry, &Contract) -> bool,
    {
        let intent = crate::verified_intent::VerifiedSemanticIntentV1::build(
            program,
            &producer_roots,
            retained_definitions.clone(),
        )
        .expect("checked OUT test fixture has valid verified intent");
        Self::try_build_with_intent(
            program,
            producer_roots,
            &intent,
            make_contract,
            is_structural_producer,
        )
    }

    pub(crate) fn try_build_with_intent<MakeContract, IsProducer, BuildError>(
        program: &CheckedProgramFields,
        producer_roots: Vec<ProducerRootSpec>,
        intent: &crate::verified_intent::VerifiedSemanticIntentV1,
        make_contract: MakeContract,
        mut is_structural_producer: IsProducer,
    ) -> Result<OutNetBuild<Contract>, BuildError>
    where
        MakeContract: FnMut(&CheckedCall, usize, &CheckedCallEntry) -> Result<Contract, BuildError>,
        IsProducer:
            FnMut(CheckedCallableKind, &CheckedCall, usize, &CheckedCallEntry, &Contract) -> bool,
    {
        let build = OutNetBuilder::new(
            program,
            producer_roots,
            intent,
            make_contract,
            |kind, call, entry_ordinal, entry, contract: &Result<Contract, BuildError>| {
                contract.as_ref().is_ok_and(|contract| {
                    is_structural_producer(kind, call, entry_ordinal, entry, contract)
                })
            },
        )
        .build();
        let OutNetBuild { graph, diagnostics } = build;
        let OutNet {
            call_instances,
            ports,
            nets,
            static_owners,
            call_instance_by_checked_frame,
            intentionally_elided_calls,
            intentionally_elided_frames,
            output_net_by_frame_target,
            concrete_producers_by_checked,
            producer_roots,
            producer_root_by_identity,
            producer_root_calls,
        } = graph;
        let ports = ports
            .into_iter()
            .map(|port| {
                Ok(OutPort {
                    id: port.id,
                    call: port.call,
                    entry_ordinal: port.entry_ordinal,
                    formal: port.formal,
                    name: port.name,
                    binding: port.binding,
                    contract: port.contract?,
                    net: port.net,
                })
            })
            .collect::<Result<Vec<_>, BuildError>>()?;
        Ok(OutNetBuild {
            graph: OutNet {
                call_instances,
                ports,
                nets,
                static_owners,
                call_instance_by_checked_frame,
                intentionally_elided_calls,
                intentionally_elided_frames,
                output_net_by_frame_target,
                concrete_producers_by_checked,
                producer_roots,
                producer_root_by_identity,
                producer_root_calls,
            },
            diagnostics,
        })
    }
}

struct PendingOutPort<Contract> {
    id: OutPortId,
    call: OutCallInstanceId,
    entry_ordinal: usize,
    formal: DeclId,
    name: String,
    binding: OutPortBinding,
    contract: Contract,
    union_node: usize,
}

struct PendingFrameCall {
    instance: OutCallInstanceId,
    callable: DeclId,
    kind: Option<CheckedCallableKind>,
    output_bindings: BTreeMap<DeclId, usize>,
}

struct PendingForward {
    call: OutCallInstanceId,
    port_node: usize,
    target: DeclId,
}

struct PendingUnifiedNet {
    root: usize,
    ports: Vec<OutPortId>,
    producers: Vec<OutPortId>,
    owner_anchors: Vec<OutPortId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CheckedStaticSelectorValue {
    Number(boon_data::ExactNumber),
    Text(String),
    Tag(String),
    Bits(boon_data::Bits),
}

impl CheckedStaticSelectorValue {
    fn matches(&self, pattern: &CheckedMatchPattern) -> bool {
        match (self, pattern) {
            (_, CheckedMatchPattern::Wildcard | CheckedMatchPattern::Binding { .. }) => true,
            (Self::Number(actual), CheckedMatchPattern::Number { value }) => actual == value,
            (Self::Text(actual), CheckedMatchPattern::Text { value }) => actual == value,
            (Self::Tag(actual), CheckedMatchPattern::Tag { name, .. }) => actual == name,
            (Self::Bits(actual), CheckedMatchPattern::Bits { value }) => actual == value,
            _ => false,
        }
    }
}

pub(crate) fn singleton_tag_for_type_projection(
    ty: &Type,
    projection: &[String],
) -> Option<String> {
    if let Some((field, rest)) = projection.split_first() {
        return match ty {
            Type::Object(shape) => {
                singleton_tag_for_type_projection(shape.fields.get(field)?, rest)
            }
            Type::Union(members) => {
                let mut tags = members
                    .iter()
                    .map(|member| singleton_tag_for_type_projection(member, projection));
                let first = tags.next()??;
                tags.all(|tag| tag.as_ref() == Some(&first))
                    .then_some(first)
            }
            _ => None,
        };
    }
    match ty {
        Type::VariantSet(variants) if variants.len() == 1 => match &variants[0] {
            boon_checked::Variant::Tag(tag) | boon_checked::Variant::Tagged { tag, .. } => {
                Some(tag.clone())
            }
        },
        Type::Union(members) => {
            let mut tags = members
                .iter()
                .map(|member| singleton_tag_for_type_projection(member, projection));
            let first = tags.next()??;
            tags.all(|tag| tag.as_ref() == Some(&first))
                .then_some(first)
        }
        _ => None,
    }
}

fn checked_static_selector_from_type(
    ty: &Type,
    projection: &[String],
) -> Option<CheckedStaticSelectorValue> {
    singleton_tag_for_type_projection(ty, projection).map(CheckedStaticSelectorValue::Tag)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum StaticOwnerNode {
    Net(OutNetId),
    Call(OutCallInstanceId),
}

struct OutNetBuilder<'program, Contract, MakeContract, IsProducer> {
    program: &'program CheckedProgramFields,
    signature_by_id: BTreeMap<DeclId, &'program CheckedCallableSignature>,
    calls_by_owner: BTreeMap<Option<DeclId>, Vec<usize>>,
    call_index_by_id: BTreeMap<CheckedCallId, usize>,
    declaration_by_id: BTreeMap<DeclId, &'program CheckedDeclaration>,
    pattern_binding_by_declaration: BTreeMap<DeclId, &'program CheckedPatternBinding>,
    statements_with_children_by_value: BTreeMap<CheckedExprId, usize>,
    function_owner_by_scope: Vec<Option<DeclId>>,
    definition_node_by_expression: Vec<
        Option<
            Option<(
                DeclId,
                &'program boon_checked::CheckedDefinitionExecutionNodeV1,
            )>,
        >,
    >,
    definition_call_indices_by_callable: BTreeMap<DeclId, Vec<usize>>,
    root_expressions: Vec<CheckedExprId>,
    resource_owning_callables: BTreeSet<DeclId>,
    producer_root_specs: Vec<ProducerRootSpec>,
    producer_roots: Vec<ProducerRoot>,
    producer_identity_by_call: BTreeMap<OutCallInstanceId, [u8; 32]>,
    retained_definitions: BTreeSet<DeclId>,
    retained_overlay_definitions: BTreeSet<DeclId>,
    retained_frame_expansion_skips: usize,
    retained_direct_call_sites_not_instantiated: usize,
    retained_overlay_frames: usize,
    retained_overlay_call_sites_instantiated: usize,
    intentionally_elided_calls: BTreeSet<(CheckedCallId, Option<OutCallInstanceId>)>,
    intentionally_elided_frames: BTreeSet<OutCallInstanceId>,
    expanded_frames: usize,
    lexical_call_sites_considered: usize,
    demanded_call_sites_instantiated: usize,
    conservative_effect_frames: usize,
    make_contract: MakeContract,
    is_structural_producer: IsProducer,
    call_instances: Vec<OutCallInstance>,
    ports: Vec<PendingOutPort<Contract>>,
    producer_ports: BTreeSet<OutPortId>,
    union_find: UnionFind,
    diagnostics: Vec<OutNetDiagnostic>,
}

impl<'program, Contract, MakeContract, IsProducer>
    OutNetBuilder<'program, Contract, MakeContract, IsProducer>
where
    MakeContract: FnMut(&CheckedCall, usize, &CheckedCallEntry) -> Contract,
    IsProducer:
        FnMut(CheckedCallableKind, &CheckedCall, usize, &CheckedCallEntry, &Contract) -> bool,
{
    fn new(
        program: &'program CheckedProgramFields,
        producer_root_specs: Vec<ProducerRootSpec>,
        intent: &crate::verified_intent::VerifiedSemanticIntentV1,
        make_contract: MakeContract,
        is_structural_producer: IsProducer,
    ) -> Self {
        let signature_by_id = program
            .callables
            .iter()
            .map(|signature| (signature.decl_id, signature))
            .collect();
        let mut calls_by_owner = BTreeMap::<Option<DeclId>, Vec<usize>>::new();
        let mut call_index_by_id = BTreeMap::new();
        for (index, call) in program.calls.iter().enumerate() {
            calls_by_owner
                .entry(call.owner_callable)
                .or_default()
                .push(index);
            call_index_by_id.insert(call.id, index);
        }
        for calls in calls_by_owner.values_mut() {
            calls.sort_by_key(|index| {
                let call = &program.calls[*index];
                (call.expression, call.id, call.callable, *index)
            });
        }

        let declaration_by_id = program
            .declarations
            .iter()
            .map(|declaration| (declaration.id, declaration))
            .collect();
        let pattern_binding_by_declaration = program
            .pattern_bindings
            .iter()
            .map(|binding| (binding.declaration, binding))
            .collect();
        let mut statements_with_children_by_value = BTreeMap::new();
        for (index, statement) in program.statements.iter().enumerate() {
            if !statement.children.is_empty()
                && let Some(value) = statement.value
            {
                statements_with_children_by_value
                    .entry(value)
                    .or_insert(index);
            }
        }
        let mut function_owner_by_scope = vec![None; program.scopes.len()];
        for scope in &program.scopes {
            if let Some(slot) = function_owner_by_scope.get_mut(scope.id.0 as usize) {
                *slot = function_owner_for_scope(program, scope.id);
            }
        }
        let resource_owning_callables =
            resource_owning_callables(program, &signature_by_id, &function_owner_by_scope);
        let mut diagnostics = alias_cycle_diagnostics(program);
        let mut definition_template_by_callable = BTreeMap::new();
        let mut definition_node_by_expression = Vec::new();
        let mut definition_call_indices_by_callable = BTreeMap::new();
        for template in &program.definition_execution_templates {
            if definition_template_by_callable
                .insert(template.callable, template)
                .is_some()
            {
                diagnostics.push(OutNetDiagnostic::InvalidDefinitionTemplate {
                    callable: template.callable,
                    reason: "duplicate callable template".to_owned(),
                });
                continue;
            }
            if template.schema != boon_checked::CHECKED_DEFINITION_EXECUTION_TEMPLATE_SCHEMA_V1
                || template.nodes.last().map(|node| node.expression) != Some(template.result)
                || signature_by_id
                    .get(&template.callable)
                    .is_none_or(|callable| {
                        callable.kind != CheckedCallableKind::User
                            || callable.result_expression != Some(template.result)
                    })
            {
                diagnostics.push(OutNetDiagnostic::InvalidDefinitionTemplate {
                    callable: template.callable,
                    reason: "schema or result root mismatch".to_owned(),
                });
            }
            let node_expressions = template
                .nodes
                .iter()
                .map(|node| node.expression)
                .collect::<BTreeSet<_>>();
            let node_calls = template
                .nodes
                .iter()
                .filter_map(|node| node.call)
                .collect::<BTreeSet<_>>();
            if node_expressions.len() != template.nodes.len()
                || template.nodes.iter().any(|node| {
                    node.dependencies
                        .iter()
                        .any(|dependency| !node_expressions.contains(dependency))
                        || node.selector.as_ref().is_some_and(|selector| {
                            !node_expressions.contains(&selector.input)
                                || selector
                                    .arms
                                    .iter()
                                    .any(|arm| !node_expressions.contains(arm))
                        })
                })
                || node_calls != template.calls.iter().copied().collect::<BTreeSet<_>>()
                || node_calls.len() != template.calls.len()
            {
                diagnostics.push(OutNetDiagnostic::InvalidDefinitionTemplate {
                    callable: template.callable,
                    reason: "node, dependency, selector, or call inventory mismatch".to_owned(),
                });
            }
            for node in &template.nodes {
                let index = node.expression.0 as usize;
                if definition_node_by_expression.len() <= index {
                    definition_node_by_expression.resize(index.saturating_add(1), None);
                }
                let slot = &mut definition_node_by_expression[index];
                match slot {
                    None => *slot = Some(Some((template.callable, node))),
                    Some(_) => {
                        *slot = Some(None);
                        diagnostics.push(OutNetDiagnostic::InvalidDefinitionTemplate {
                            callable: template.callable,
                            reason: format!("duplicate expression {}", node.expression.0),
                        });
                    }
                }
            }
            let mut call_indices = Vec::with_capacity(template.calls.len());
            for call in &template.calls {
                match call_index_by_id.get(call).copied() {
                    Some(index)
                        if program.calls[index].owner_callable == Some(template.callable) =>
                    {
                        call_indices.push(index)
                    }
                    None => diagnostics.push(OutNetDiagnostic::InvalidDefinitionTemplate {
                        callable: template.callable,
                        reason: format!("missing call {}", call.0),
                    }),
                    Some(_) => diagnostics.push(OutNetDiagnostic::InvalidDefinitionTemplate {
                        callable: template.callable,
                        reason: format!("call {} belongs to another definition", call.0),
                    }),
                }
            }
            call_indices.sort_by_key(|index| {
                let call = &program.calls[*index];
                (call.expression, call.id, call.callable, *index)
            });
            definition_call_indices_by_callable.insert(template.callable, call_indices);
        }
        for callable in program.callables.iter().filter(|callable| {
            callable.kind == CheckedCallableKind::User && callable.result_expression.is_some()
        }) {
            if !definition_template_by_callable.contains_key(&callable.decl_id) {
                diagnostics.push(OutNetDiagnostic::InvalidDefinitionTemplate {
                    callable: callable.decl_id,
                    reason: "missing user definition template".to_owned(),
                });
            }
        }
        let retained_definitions = intent.retained_definitions();
        let retained_overlay_definitions =
            retained_overlay_definitions(program, retained_definitions, &signature_by_id);
        let root_expressions = intent.program_schedule_roots().to_vec();
        Self {
            program,
            signature_by_id,
            calls_by_owner,
            call_index_by_id,
            declaration_by_id,
            pattern_binding_by_declaration,
            statements_with_children_by_value,
            function_owner_by_scope,
            definition_node_by_expression,
            definition_call_indices_by_callable,
            root_expressions,
            resource_owning_callables,
            producer_root_specs,
            producer_roots: Vec::new(),
            producer_identity_by_call: BTreeMap::new(),
            retained_definitions: retained_definitions.clone(),
            retained_overlay_definitions,
            retained_frame_expansion_skips: 0,
            retained_direct_call_sites_not_instantiated: 0,
            retained_overlay_frames: 0,
            retained_overlay_call_sites_instantiated: 0,
            intentionally_elided_calls: BTreeSet::new(),
            intentionally_elided_frames: BTreeSet::new(),
            expanded_frames: 0,
            lexical_call_sites_considered: 0,
            demanded_call_sites_instantiated: 0,
            conservative_effect_frames: 0,
            make_contract,
            is_structural_producer,
            call_instances: Vec::new(),
            ports: Vec::new(),
            producer_ports: BTreeSet::new(),
            union_find: UnionFind::default(),
            diagnostics,
        }
    }

    fn build(mut self) -> OutNetBuild<Contract> {
        self.instantiate_frame(None, None, BTreeMap::new(), &mut Vec::new());
        let producer_roots = std::mem::take(&mut self.producer_root_specs);
        for producer in producer_roots {
            self.instantiate_producer_root(producer);
        }
        self.finish()
    }

    fn instantiate_producer_root(&mut self, spec: ProducerRootSpec) {
        let Some(signature) = self.signature_by_id.get(&spec.callable).copied() else {
            return;
        };
        let Some(result_expression) = signature.result_expression else {
            return;
        };
        let call = OutCallInstanceId(self.call_instances.len());
        let inputs = spec
            .parameters
            .iter()
            .map(|parameter| OutInputBinding {
                formal: parameter.formal,
                value: OutInputValue::ProducerParameter {
                    parameter: parameter.parameter,
                    flow_type: parameter.flow_type.clone(),
                },
            })
            .collect();
        self.call_instances.push(OutCallInstance {
            id: call,
            parent: None,
            provenance: OutCallProvenance {
                call_id: None,
                expression: result_expression,
                owner_callable: None,
                callable: spec.callable,
            },
            parent_output: None,
            parent_output_node: None,
            inputs,
            passed: None,
            ports: Vec::new(),
            local_type_substitutions: Vec::new(),
            resolved_type_substitutions: Vec::new(),
            type_substitution_count: 0,
            result: signature.result.clone(),
            result_is_exact_occurrence: false,
            owner: None,
        });
        self.producer_identity_by_call.insert(call, spec.identity);
        self.producer_roots.push(ProducerRoot { spec, call });
        self.instantiate_frame(
            Some(signature.decl_id),
            Some(call),
            BTreeMap::new(),
            &mut vec![signature.decl_id],
        );
    }

    fn reachable_call_indices(
        &mut self,
        owner_callable: Option<DeclId>,
        frame: Option<OutCallInstanceId>,
    ) -> Vec<usize> {
        let conservative = owner_callable.is_some_and(|owner| {
            self.resource_owning_callables.contains(&owner)
                || self.signature_by_id.get(&owner).is_some_and(|callable| {
                    callable.effect.writes_state
                        || callable.effect.emits_source
                        || callable.effect.invokes_host
                })
        });
        let all_calls = match owner_callable {
            Some(owner) if !conservative => self
                .definition_call_indices_by_callable
                .get(&owner)
                .cloned()
                .unwrap_or_default(),
            _ => self
                .calls_by_owner
                .get(&owner_callable)
                .cloned()
                .unwrap_or_default(),
        };
        self.expanded_frames += 1;
        self.lexical_call_sites_considered += all_calls.len();
        if conservative {
            // Stateful/effectful callable bodies can contain update expressions
            // rooted by statement scheduling rather than their result value.
            // Keep their complete lexical call inventory until that scheduling
            // is represented by the same explicit expression roots.
            self.conservative_effect_frames += 1;
            self.demanded_call_sites_instantiated += all_calls.len();
            return all_calls;
        }

        let mut pending = owner_callable.map_or_else(
            || self.root_expressions.clone(),
            |owner| {
                self.signature_by_id
                    .get(&owner)
                    .and_then(|callable| callable.result_expression)
                    .into_iter()
                    .collect()
            },
        );
        let mut visited = BTreeSet::new();
        let mut reachable = BTreeSet::new();
        while let Some(expression) = pending.pop() {
            if !visited.insert(expression) {
                continue;
            }
            if let Some(owner) = owner_callable
                && let Some(node) = self
                    .definition_node_by_expression
                    .get(expression.0 as usize)
                    .copied()
                    .flatten()
                    .flatten()
                    .and_then(|(node_owner, node)| (node_owner == owner).then_some(node))
            {
                if let Some(call) = node.call
                    && let Some(index) = self.call_index_by_id.get(&call).copied()
                {
                    reachable.insert(index);
                }
                if let Some(selector) = &node.selector {
                    pending.push(selector.input);
                    let selected = self
                        .static_checked_selector_value(
                            selector.input,
                            frame,
                            Vec::new(),
                            &mut BTreeSet::new(),
                        )
                        .and_then(|value| self.selected_checked_arm(&value, &selector.arms));
                    if let Some(selected) = selected {
                        pending.push(selected);
                    } else {
                        pending.extend(selector.arms.iter().copied());
                    }
                } else {
                    pending.extend(node.dependencies.iter().copied());
                }
                continue;
            }
            let Some(expression) = self
                .program
                .expressions
                .get(expression.0 as usize)
                .filter(|candidate| candidate.id == expression)
            else {
                continue;
            };
            match &expression.kind {
                CheckedExpressionKind::Read { target, .. } => {
                    if let Some(declaration) = self.declaration_by_id.get(target)
                        && declaration.kind == CheckedDeclarationKind::Field
                        && self
                            .function_owner_by_scope
                            .get(declaration.scope_id.0 as usize)
                            .copied()
                            .flatten()
                            == owner_callable
                    {
                        pending.extend(declaration.value);
                    }
                }
                CheckedExpressionKind::Call { call } => {
                    let Some(index) = self.call_index_by_id.get(call).copied() else {
                        continue;
                    };
                    reachable.insert(index);
                    let call = &self.program.calls[index];
                    pending.extend(call.entries.iter().filter_map(|entry| match entry {
                        CheckedCallEntry::Input { value, .. } => Some(*value),
                        CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => {
                            None
                        }
                    }));
                    pending.extend(call.context_binding.explicit().map(|(value, _)| value));
                }
                CheckedExpressionKind::When { input, arms } => {
                    pending.push(*input);
                    let selected = self
                        .static_checked_selector_value(
                            *input,
                            frame,
                            Vec::new(),
                            &mut BTreeSet::new(),
                        )
                        .and_then(|selector| self.selected_checked_arm(&selector, arms));
                    if let Some(selected) = selected {
                        pending.push(selected);
                    } else {
                        pending.extend(arms.iter().copied());
                    }
                }
                CheckedExpressionKind::While { input, arms } => {
                    pending.push(*input);
                    pending.extend(arms.iter().copied());
                }
                CheckedExpressionKind::TextTemplate { segments } => {
                    pending.extend(segments.iter().filter_map(|segment| match segment {
                        boon_checked::CheckedTextSegment::Static { .. } => None,
                        boon_checked::CheckedTextSegment::Dynamic { value } => Some(*value),
                    }));
                }
                CheckedExpressionKind::TaggedObject { fields, .. }
                | CheckedExpressionKind::Object { fields } => {
                    pending.extend(fields.iter().map(|field| field.value));
                }
                CheckedExpressionKind::Flush { payload: input }
                | CheckedExpressionKind::Draining { input } => pending.push(*input),
                CheckedExpressionKind::Hold { initial, .. } => {
                    pending.push(*initial);
                    pending.extend(self.checked_statement_child_values(expression.id));
                }
                CheckedExpressionKind::Latest { branches } => {
                    pending.extend(branches.iter().copied());
                }
                CheckedExpressionKind::Then { input, output } => {
                    pending.push(*input);
                    pending.extend(*output);
                }
                CheckedExpressionKind::Infix { left, right, .. }
                | CheckedExpressionKind::MapEntry {
                    key: left,
                    value: right,
                } => {
                    pending.push(*left);
                    pending.push(*right);
                }
                CheckedExpressionKind::MatchArm { output, .. } => {
                    pending.extend(*output);
                    pending.extend(self.checked_statement_child_values(expression.id));
                }
                CheckedExpressionKind::Block { bindings, result } => {
                    pending.extend(bindings.iter().map(|binding| binding.value));
                    pending.extend(*result);
                }
                CheckedExpressionKind::List { items, .. }
                | CheckedExpressionKind::Bytes { items, .. }
                | CheckedExpressionKind::Set { items }
                | CheckedExpressionKind::Map { entries: items } => {
                    pending.extend(items.iter().copied());
                }
                CheckedExpressionKind::Passed { .. }
                | CheckedExpressionKind::ExternalRead { .. }
                | CheckedExpressionKind::Drain { .. }
                | CheckedExpressionKind::Text { .. }
                | CheckedExpressionKind::Number { .. }
                | CheckedExpressionKind::Bits { .. }
                | CheckedExpressionKind::BytesByte { .. }
                | CheckedExpressionKind::Absent
                | CheckedExpressionKind::Tag { .. }
                | CheckedExpressionKind::Source
                | CheckedExpressionKind::Delimiter
                | CheckedExpressionKind::Invalid { .. } => {}
            }
        }
        let mut demanded = all_calls
            .into_iter()
            .filter(|call| reachable.contains(call))
            .collect::<Vec<_>>();
        if owner_callable.is_some_and(|owner| self.retained_definitions.contains(&owner)) {
            self.retained_overlay_frames += 1;
            let reachable_count = demanded.len();
            let mut intentionally_elided = Vec::new();
            demanded.retain(|index| {
                let requires_overlay = self.retained_call_site_requires_overlay(*index);
                if !requires_overlay && let Some(call) = self.program.calls.get(*index) {
                    intentionally_elided.push((call.id, frame));
                }
                requires_overlay
            });
            self.intentionally_elided_calls.extend(intentionally_elided);
            self.retained_direct_call_sites_not_instantiated +=
                reachable_count.saturating_sub(demanded.len());
            self.retained_overlay_call_sites_instantiated += demanded.len();
        }
        self.demanded_call_sites_instantiated += demanded.len();
        demanded
    }

    fn retained_call_site_requires_overlay(&self, call_index: usize) -> bool {
        let Some(call) = self.program.calls.get(call_index) else {
            return true;
        };
        if !call.contexts.is_empty()
            || call
                .entries
                .iter()
                .any(|entry| !matches!(entry, CheckedCallEntry::Input { .. }))
        {
            return true;
        }
        let Some(callable) = self.signature_by_id.get(&call.callable).copied() else {
            return true;
        };
        match callable.kind {
            CheckedCallableKind::User => {
                !self.retained_definitions.contains(&callable.decl_id)
                    || self
                        .retained_overlay_definitions
                        .contains(&callable.decl_id)
            }
            CheckedCallableKind::Builtin | CheckedCallableKind::External => {
                callable.effect != boon_checked::CheckedEffectSummary::default()
                    || !callable.contexts.is_empty()
                    || callable.context_formal.is_some()
            }
        }
    }

    fn call_is_callable_result_output(
        &self,
        callable: DeclId,
        target: CheckedExprId,
        frame: Option<OutCallInstanceId>,
    ) -> bool {
        let mut pending = self
            .signature_by_id
            .get(&callable)
            .and_then(|signature| signature.result_expression)
            .into_iter()
            .collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if current == target {
                return true;
            }
            if !visited.insert(current) {
                continue;
            }
            let Some(expression) = self
                .program
                .expressions
                .get(current.0 as usize)
                .filter(|expression| expression.id == current)
            else {
                continue;
            };
            match &expression.kind {
                CheckedExpressionKind::When { input, arms } => {
                    let selected = self
                        .static_checked_selector_value(
                            *input,
                            frame,
                            Vec::new(),
                            &mut BTreeSet::new(),
                        )
                        .and_then(|selector| self.selected_checked_arm(&selector, arms));
                    if let Some(selected) = selected {
                        pending.push(selected);
                    } else {
                        pending.extend(arms.iter().copied());
                    }
                }
                CheckedExpressionKind::While { arms, .. }
                | CheckedExpressionKind::Latest { branches: arms } => {
                    pending.extend(arms.iter().copied());
                }
                CheckedExpressionKind::MatchArm { output, .. } => pending.extend(*output),
                CheckedExpressionKind::Block { result, .. } => pending.extend(*result),
                CheckedExpressionKind::Then { input, output } => {
                    pending.push(output.unwrap_or(*input));
                }
                CheckedExpressionKind::Read {
                    target, projection, ..
                } if projection.is_empty() => {
                    pending.extend(
                        self.declaration_by_id
                            .get(target)
                            .and_then(|declaration| declaration.value),
                    );
                }
                CheckedExpressionKind::Draining { input }
                | CheckedExpressionKind::Hold { initial: input, .. } => pending.push(*input),
                CheckedExpressionKind::Call { .. }
                | CheckedExpressionKind::Passed { .. }
                | CheckedExpressionKind::ExternalRead { .. }
                | CheckedExpressionKind::Drain { .. }
                | CheckedExpressionKind::Read { .. }
                | CheckedExpressionKind::Text { .. }
                | CheckedExpressionKind::TextTemplate { .. }
                | CheckedExpressionKind::Number { .. }
                | CheckedExpressionKind::Bits { .. }
                | CheckedExpressionKind::BytesByte { .. }
                | CheckedExpressionKind::Absent
                | CheckedExpressionKind::Flush { .. }
                | CheckedExpressionKind::Tag { .. }
                | CheckedExpressionKind::TaggedObject { .. }
                | CheckedExpressionKind::Source
                | CheckedExpressionKind::Infix { .. }
                | CheckedExpressionKind::Object { .. }
                | CheckedExpressionKind::List { .. }
                | CheckedExpressionKind::MapEntry { .. }
                | CheckedExpressionKind::Map { .. }
                | CheckedExpressionKind::Set { .. }
                | CheckedExpressionKind::Bytes { .. }
                | CheckedExpressionKind::Delimiter
                | CheckedExpressionKind::Invalid { .. } => {}
            }
        }
        false
    }

    fn checked_statement_child_values(
        &self,
        parent_expression: CheckedExprId,
    ) -> Vec<CheckedExprId> {
        let Some(statement) = self
            .statements_with_children_by_value
            .get(&parent_expression)
            .and_then(|index| self.program.statements.get(*index))
        else {
            return Vec::new();
        };
        let mut statements = statement.children.iter().rev().copied().collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        let mut values = Vec::new();
        while let Some(statement_id) = statements.pop() {
            if !visited.insert(statement_id) {
                continue;
            }
            let Some(statement) = self
                .program
                .statements
                .get(statement_id.0 as usize)
                .filter(|candidate| candidate.id == statement_id)
            else {
                continue;
            };
            match statement.value {
                Some(value) if value == parent_expression => {
                    statements.extend(statement.children.iter().rev().copied());
                }
                Some(value) => values.push(value),
                None => statements.extend(statement.children.iter().rev().copied()),
            }
        }
        values
    }

    fn selected_checked_arm(
        &self,
        selector: &CheckedStaticSelectorValue,
        arms: &[CheckedExprId],
    ) -> Option<CheckedExprId> {
        arms.iter().copied().find(|arm| {
            self.program
                .expressions
                .get(arm.0 as usize)
                .filter(|candidate| candidate.id == *arm)
                .and_then(|expression| match &expression.kind {
                    CheckedExpressionKind::MatchArm { pattern, .. } => Some(pattern),
                    _ => None,
                })
                .is_some_and(|pattern| selector.matches(pattern))
        })
    }

    fn checked_type_in_frame(&self, frame: OutCallInstanceId, ty: &Type) -> Type {
        apply_type_substitution_frames(&self.call_instances, Some(frame), ty)
    }

    fn static_checked_selector_value(
        &self,
        expression: CheckedExprId,
        frame: Option<OutCallInstanceId>,
        mut projection: Vec<String>,
        visited: &mut BTreeSet<(CheckedExprId, Option<OutCallInstanceId>, Vec<String>)>,
    ) -> Option<CheckedStaticSelectorValue> {
        if !visited.insert((expression, frame, projection.clone())) {
            return None;
        }
        let definition = self
            .program
            .expressions
            .get(expression.0 as usize)
            .filter(|candidate| candidate.id == expression)?;
        let type_selected_occurrence = match &definition.kind {
            CheckedExpressionKind::Call { .. } => true,
            CheckedExpressionKind::Read { target, .. } => self
                .declaration_by_id
                .get(target)
                .is_some_and(|declaration| {
                    matches!(
                        declaration.kind,
                        CheckedDeclarationKind::FreshOut | CheckedDeclarationKind::OutParameter
                    )
                }),
            _ => false,
        };
        let inferred_selector = type_selected_occurrence
            .then(|| {
                checked_static_selector_from_type(&definition.flow_type.ty, &projection).or_else(
                    || {
                        frame.and_then(|frame| {
                            let concrete =
                                self.checked_type_in_frame(frame, &definition.flow_type.ty);
                            checked_static_selector_from_type(&concrete, &projection)
                        })
                    },
                )
            })
            .flatten();
        match &definition.kind {
            CheckedExpressionKind::Read {
                target,
                projection: read_projection,
                ..
            } => {
                let mut fields = read_projection.clone();
                fields.extend(projection);
                if let Some(actual) = frame.and_then(|frame| {
                    self.call_instances
                        .get(frame.as_usize())?
                        .inputs
                        .iter()
                        .find(|binding| binding.formal == *target)
                        .map(|binding| binding.value.clone())
                }) {
                    return match actual {
                        OutInputValue::Checked(actual) => self.static_checked_selector_value(
                            actual.expression,
                            actual.frame,
                            fields,
                            visited,
                        ),
                        OutInputValue::ProducerParameter { .. } => None,
                    };
                }
                if let Some(binding) = self.pattern_binding_by_declaration.get(target) {
                    let mut binding_projection = binding.projection.clone();
                    binding_projection.extend(fields);
                    return self.static_checked_selector_value(
                        binding.selector,
                        frame,
                        binding_projection,
                        visited,
                    );
                }
                if let Some(declaration) = self.declaration_by_id.get(target)
                    && declaration.kind == CheckedDeclarationKind::Field
                    && self
                        .function_owner_by_scope
                        .get(declaration.scope_id.0 as usize)
                        .copied()
                        .flatten()
                        .is_some()
                    && let Some(value) = declaration.value
                {
                    return self.static_checked_selector_value(value, frame, fields, visited);
                }
                inferred_selector
            }
            CheckedExpressionKind::Passed {
                formal,
                projection: passed_projection,
                access: CheckedPassedAccess::Read,
            } => {
                let passed = frame
                    .and_then(|frame| self.call_instances.get(frame.as_usize()))?
                    .passed?;
                if passed.formal != *formal {
                    return None;
                }
                let mut fields = passed_projection.clone();
                fields.extend(projection);
                self.static_checked_selector_value(
                    passed.value.expression,
                    passed.value.frame,
                    fields,
                    visited,
                )
            }
            CheckedExpressionKind::TaggedObject { fields, .. }
            | CheckedExpressionKind::Object { fields }
                if !projection.is_empty() && fields.iter().all(|field| !field.spread) =>
            {
                let field = projection.remove(0);
                let value = fields
                    .iter()
                    .rev()
                    .find(|candidate| candidate.name == field)?
                    .value;
                self.static_checked_selector_value(value, frame, projection, visited)
            }
            CheckedExpressionKind::Block { result, .. } => {
                self.static_checked_selector_value((*result)?, frame, projection, visited)
            }
            CheckedExpressionKind::Flush { payload }
            | CheckedExpressionKind::Draining { input: payload } => {
                self.static_checked_selector_value(*payload, frame, projection, visited)
            }
            CheckedExpressionKind::When { input, arms } if projection.is_empty() => {
                let selector =
                    self.static_checked_selector_value(*input, frame, Vec::new(), visited)?;
                let selected = self.selected_checked_arm(&selector, arms)?;
                let output = self
                    .program
                    .expressions
                    .get(selected.0 as usize)
                    .filter(|candidate| candidate.id == selected)
                    .and_then(|expression| match &expression.kind {
                        CheckedExpressionKind::MatchArm { output, .. } => *output,
                        _ => None,
                    })?;
                self.static_checked_selector_value(output, frame, Vec::new(), visited)
            }
            CheckedExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => self.static_checked_selector_value(*output, frame, projection, visited),
            CheckedExpressionKind::Number { value } if projection.is_empty() => {
                Some(CheckedStaticSelectorValue::Number(value.clone()))
            }
            CheckedExpressionKind::Text { value } if projection.is_empty() => {
                Some(CheckedStaticSelectorValue::Text(value.clone()))
            }
            CheckedExpressionKind::Tag { name } if projection.is_empty() => {
                Some(CheckedStaticSelectorValue::Tag(name.clone()))
            }
            CheckedExpressionKind::TaggedObject { tag, .. } if projection.is_empty() => {
                Some(CheckedStaticSelectorValue::Tag(tag.clone()))
            }
            CheckedExpressionKind::Bits { value } if projection.is_empty() => {
                Some(CheckedStaticSelectorValue::Bits(value.clone()))
            }
            _ => inferred_selector,
        }
    }

    fn instantiate_frame(
        &mut self,
        owner_callable: Option<DeclId>,
        parent: Option<OutCallInstanceId>,
        mut frame_bindings: BTreeMap<DeclId, usize>,
        active_callables: &mut Vec<DeclId>,
    ) {
        let program = self.program;
        let static_calls = self.reachable_call_indices(owner_callable, parent);
        let mut pending_calls = Vec::with_capacity(static_calls.len());
        let mut pending_forwards = Vec::new();

        // Allocate every fresh declaration in the frame before resolving any
        // forwarding edge. DeclId resolution has already happened, so this is
        // deterministic and independent of checked-call storage order.
        for static_call_index in static_calls {
            let checked_call = &program.calls[static_call_index];
            let provenance = OutCallProvenance::from(checked_call);
            let instance = OutCallInstanceId(self.call_instances.len());
            let signature = self.signature_by_id.get(&checked_call.callable).copied();
            let inherited_parent_output_node =
                parent.and_then(|parent| self.call_instances[parent.as_usize()].parent_output_node);
            let inherited_passed =
                parent.and_then(|parent| self.call_instances[parent.as_usize()].passed);
            let passed = match checked_call.context_binding {
                CheckedContextBinding::Explicit { value, .. } => signature.and_then(|signature| {
                    signature.context_formal.map(|formal| PassedBinding {
                        formal,
                        value: ScopedCheckedExpr {
                            expression: value,
                            frame: parent,
                            evaluation_port: None,
                            value_frame: None,
                        },
                        evaluation_call: instance,
                    })
                }),
                CheckedContextBinding::Inherited { .. } => {
                    let passed = signature
                        .and_then(|signature| signature.context_formal)
                        .zip(inherited_passed)
                        .map(|(formal, inherited)| PassedBinding {
                            formal,
                            ..inherited
                        });
                    if passed.is_none() {
                        self.diagnostics
                            .push(OutNetDiagnostic::MissingPassedContext {
                                call: instance,
                                callable: checked_call.callable,
                            });
                    }
                    passed
                }
                CheckedContextBinding::None => {
                    if signature.is_some_and(CheckedCallableSignature::requires_pass) {
                        self.diagnostics
                            .push(OutNetDiagnostic::MissingPassedContext {
                                call: instance,
                                callable: checked_call.callable,
                            });
                    }
                    None
                }
            };
            // Keep the call row as one directional namespace edge. Its keys
            // are callee alphas and its values are caller alphas; pre-flattening
            // those values through an ancestry map aliases unrelated equal
            // ordinals from intermediate call schemes.
            let local_type_substitutions = checked_call.type_substitutions.to_vec();
            let resolved_type_substitutions = local_type_substitutions
                .iter()
                .map(|substitution| CheckedTypeSubstitution {
                    variable: substitution.variable,
                    value: apply_type_substitution_frames(
                        &self.call_instances,
                        parent,
                        &substitution.value,
                    ),
                })
                .collect();
            let type_substitution_count = parent
                .and_then(|parent| self.call_instances.get(parent.as_usize()))
                .map_or(0, |parent| parent.type_substitution_count)
                .saturating_add(local_type_substitutions.len());
            let result_scheme = signature
                .map(|signature| &signature.result)
                .unwrap_or(&checked_call.result);
            let local_result =
                apply_checked_type_substitutions_once(&result_scheme.ty, &local_type_substitutions);
            let instantiated_result =
                apply_type_substitution_frames(&self.call_instances, parent, &local_result);
            let checked_occurrence_result = apply_type_substitution_frames(
                &self.call_instances,
                parent,
                &checked_call.result.ty,
            );
            let checked_result = if checked_call.syntax_discriminated_result {
                // Checked-call finalization owns syntax-discriminated
                // occurrences. A heterogeneous dispatcher may have a closed
                // scalar principal while this exact tagged request selects a
                // closed record/list result; reapplying the principal here
                // would erase the authoritative occurrence before semantic
                // expansion.
                checked_occurrence_result
            } else {
                specialize_checked_occurrence_result(
                    &instantiated_result,
                    &checked_occurrence_result,
                )
            };
            let expression_result = self
                .program
                .expressions
                .get(checked_call.expression.0 as usize)
                .filter(|expression| expression.id == checked_call.expression)
                .map(|expression| {
                    apply_type_substitution_frames(
                        &self.call_instances,
                        parent,
                        &expression.flow_type.ty,
                    )
                })
                .unwrap_or_else(|| checked_result.clone());
            let occurrence_result =
                boon_checked::specialize_checked_call_result(&checked_result, &expression_result);
            let enclosing_result = checked_call
                .owner_callable
                .and_then(|owner| self.signature_by_id.get(&owner).copied())
                .and_then(|owner| {
                    parent.map(|parent| {
                        (
                            owner,
                            parent,
                            self.call_instances[parent.as_usize()].result.clone(),
                            self.call_instances[parent.as_usize()].result_is_exact_occurrence,
                        )
                    })
                })
                .filter(|(owner, parent, _, parent_is_exact)| {
                    owner.result_expression == Some(checked_call.expression)
                        || (*parent_is_exact
                            && self.call_is_callable_result_output(
                                owner.decl_id,
                                checked_call.expression,
                                Some(*parent),
                            ))
                });
            let result_type = enclosing_result
                .as_ref()
                .map(|(_, _, enclosing, enclosing_is_exact)| {
                    if *enclosing_is_exact {
                        enclosing.ty.clone()
                    } else {
                        boon_checked::specialize_checked_call_result(
                            &occurrence_result,
                            &enclosing.ty,
                        )
                    }
                })
                .unwrap_or(occurrence_result);
            let result_is_exact_occurrence = checked_call.syntax_discriminated_result
                || enclosing_result
                    .as_ref()
                    .is_some_and(|(_, _, _, enclosing_is_exact)| *enclosing_is_exact);
            let result = FlowType {
                // The callable signature supplies the generic result type, but
                // temporal gating is occurrence-specific and has already been
                // resolved on the checked call.
                mode: enclosing_result
                    .map(|(_, _, enclosing, _)| enclosing.mode)
                    .unwrap_or(checked_call.result.mode),
                ty: result_type,
            };
            self.call_instances.push(OutCallInstance {
                id: instance,
                parent,
                provenance,
                parent_output: self.nearest_repeated_output(checked_call.expression),
                parent_output_node: inherited_parent_output_node,
                inputs: Vec::new(),
                passed,
                ports: Vec::new(),
                local_type_substitutions,
                resolved_type_substitutions,
                type_substitution_count,
                result,
                result_is_exact_occurrence,
                owner: None,
            });

            let kind = signature.map(|signature| signature.kind);
            if kind.is_none() {
                self.diagnostics.push(OutNetDiagnostic::MissingCallable {
                    call: instance,
                    callable: checked_call.callable,
                });
            }

            let mut output_bindings = BTreeMap::new();
            for (entry_ordinal, entry) in checked_call.entries.iter().enumerate() {
                let (formal, name, binding) = match entry {
                    CheckedCallEntry::Input { .. } => continue,
                    CheckedCallEntry::FreshOut {
                        formal,
                        name,
                        output,
                        scope_id,
                    } => (
                        *formal,
                        name.clone(),
                        OutPortBinding::Fresh {
                            output: *output,
                            scope_id: *scope_id,
                        },
                    ),
                    CheckedCallEntry::ForwardOut {
                        formal,
                        name,
                        target,
                        ..
                    } => (
                        *formal,
                        name.clone(),
                        OutPortBinding::Forward { target: *target },
                    ),
                };
                let contract = (self.make_contract)(checked_call, entry_ordinal, entry);
                let port = OutPortId(self.ports.len());
                let union_node = self.union_find.make_set();
                if kind.is_some_and(|kind| {
                    (self.is_structural_producer)(
                        kind,
                        checked_call,
                        entry_ordinal,
                        entry,
                        &contract,
                    )
                }) {
                    self.producer_ports.insert(port);
                }
                self.ports.push(PendingOutPort {
                    id: port,
                    call: instance,
                    entry_ordinal,
                    formal,
                    name,
                    binding,
                    contract,
                    union_node,
                });
                self.call_instances[instance.as_usize()].ports.push(port);

                if let Some(previous) = output_bindings.insert(formal, union_node) {
                    self.union_find.union(previous, union_node);
                    self.diagnostics
                        .push(OutNetDiagnostic::DuplicateFormalBinding {
                            call: instance,
                            formal,
                        });
                }
                match binding {
                    OutPortBinding::Fresh { output, .. } => {
                        if let Some(previous) = frame_bindings.insert(output, union_node) {
                            self.union_find.union(previous, union_node);
                            self.diagnostics
                                .push(OutNetDiagnostic::DuplicateFreshOutput {
                                    call: instance,
                                    output,
                                });
                        }
                    }
                    OutPortBinding::Forward { target } => {
                        pending_forwards.push(PendingForward {
                            call: instance,
                            port_node: union_node,
                            target,
                        });
                    }
                }
            }
            self.call_instances[instance.as_usize()].inputs = checked_call
                .entries
                .iter()
                .filter_map(|entry| {
                    let CheckedCallEntry::Input {
                        formal,
                        value,
                        evaluation_scope,
                        ..
                    } = entry
                    else {
                        return None;
                    };
                    let evaluation_port = match evaluation_scope {
                        CheckedEvaluationScope::Parent => None,
                        CheckedEvaluationScope::Output { formal } => self.call_instances
                            [instance.as_usize()]
                        .ports
                        .iter()
                        .copied()
                        .find(|port_id| self.ports[port_id.as_usize()].formal == *formal),
                    };
                    Some(OutInputBinding {
                        formal: *formal,
                        value: OutInputValue::Checked(ScopedCheckedExpr {
                            expression: *value,
                            frame: parent,
                            evaluation_port,
                            value_frame: None,
                        }),
                    })
                })
                .collect();
            pending_calls.push(PendingFrameCall {
                instance,
                callable: checked_call.callable,
                kind,
                output_bindings,
            });
        }

        for pending in &pending_calls {
            let Some(parent_output) =
                self.call_instances[pending.instance.as_usize()].parent_output
            else {
                continue;
            };
            if let Some(parent_node) = frame_bindings.get(&parent_output).copied() {
                self.call_instances[pending.instance.as_usize()].parent_output_node =
                    Some(parent_node);
            } else {
                self.diagnostics
                    .push(OutNetDiagnostic::UnknownParentOutput {
                        call: pending.instance,
                        output: parent_output,
                    });
            }
        }

        for forwarding in pending_forwards {
            if let Some(target_node) = frame_bindings.get(&forwarding.target).copied() {
                self.union_find.union(target_node, forwarding.port_node);
            } else {
                self.diagnostics
                    .push(OutNetDiagnostic::UnknownForwardTarget {
                        call: forwarding.call,
                        target: forwarding.target,
                    });
            }
        }

        // A call written inside a repeated-output body evaluates its
        // parent-scoped arguments in that enclosing output, even when none of
        // its own formals is output-scoped. Preserve the concrete enclosing
        // port so later type resolution can recover the per-output generic
        // substitutions instead of falling back to the caller frame.
        let inherited_evaluation_ports = pending_calls
            .iter()
            .filter_map(|pending| {
                let call = &self.call_instances[pending.instance.as_usize()];
                let parent_output = call.parent_output?;
                let parent_frame = call.parent;
                let evaluation_port = self
                    .ports
                    .iter()
                    .find(|port| {
                        matches!(
                            port.binding,
                            OutPortBinding::Fresh { output, .. } if output == parent_output
                        ) && self.call_instances[port.call.as_usize()].parent == parent_frame
                    })
                    .map(|port| port.id)?;
                Some((pending.instance, evaluation_port))
            })
            .collect::<Vec<_>>();
        for (instance, evaluation_port) in inherited_evaluation_ports {
            let call = &mut self.call_instances[instance.as_usize()];
            for input in &mut call.inputs {
                if let OutInputValue::Checked(value) = &mut input.value
                    && value.evaluation_port.is_none()
                {
                    value.evaluation_port = Some(evaluation_port);
                }
            }
            if let Some(passed) = &mut call.passed
                && passed.evaluation_call == instance
                && passed.value.evaluation_port.is_none()
            {
                passed.value.evaluation_port = Some(evaluation_port);
            }
        }

        for pending in pending_calls {
            if pending.kind != Some(CheckedCallableKind::User) {
                continue;
            }
            if self.retained_definitions.contains(&pending.callable) {
                if !self
                    .retained_overlay_definitions
                    .contains(&pending.callable)
                {
                    self.retained_frame_expansion_skips += 1;
                    self.intentionally_elided_frames.insert(pending.instance);
                    continue;
                }
            }
            if active_callables.contains(&pending.callable) {
                self.diagnostics
                    .push(OutNetDiagnostic::RecursiveContextualCall {
                        call: pending.instance,
                        callable: pending.callable,
                    });
                continue;
            }
            active_callables.push(pending.callable);
            self.instantiate_frame(
                Some(pending.callable),
                Some(pending.instance),
                pending.output_bindings,
                active_callables,
            );
            active_callables.pop();
        }
    }

    fn nearest_repeated_output(&self, expression: CheckedExprId) -> Option<DeclId> {
        let mut scope = checked_expression(self.program, expression)?.scope_id;
        loop {
            let checked_scope = checked_scope(self.program, scope)?;
            if checked_scope.kind == CheckedScopeKind::RepeatedOutput {
                return checked_scope.owner;
            }
            scope = checked_scope.parent?;
        }
    }

    fn finish(mut self) -> OutNetBuild<Contract> {
        if std::env::var_os("BOON_SEMANTIC_TRACE").is_some() {
            eprintln!(
                "boon_semantic out_demand checked_call_sites={} retained_definitions={} retained_overlay_definitions={} expanded_frames={} frame_expansion_skips={} retained_overlay_frames={} lexical_call_sites_considered={} demanded_call_sites_instantiated={} retained_overlay_call_sites_instantiated={} conservative_effect_frames={} direct_body_call_sites_not_instantiated={}",
                self.program.calls.len(),
                self.retained_definitions.len(),
                self.retained_overlay_definitions.len(),
                self.expanded_frames,
                self.retained_frame_expansion_skips,
                self.retained_overlay_frames,
                self.lexical_call_sites_considered,
                self.demanded_call_sites_instantiated,
                self.retained_overlay_call_sites_instantiated,
                self.conservative_effect_frames,
                self.retained_direct_call_sites_not_instantiated,
            );
        }
        let mut grouped = BTreeMap::<usize, PendingUnifiedNet>::new();
        for port in &self.ports {
            let root = self.union_find.find(port.union_node);
            let group = grouped.entry(root).or_insert_with(|| PendingUnifiedNet {
                root,
                ports: Vec::new(),
                producers: Vec::new(),
                owner_anchors: Vec::new(),
            });
            group.ports.push(port.id);
            if self.producer_ports.contains(&port.id) {
                group.producers.push(port.id);
            }
            if matches!(port.binding, OutPortBinding::Fresh { .. }) {
                group.owner_anchors.push(port.id);
            }
        }

        let mut pending_nets = grouped.into_values().collect::<Vec<_>>();
        pending_nets.sort_by_key(|net| net.ports.first().copied());
        let mut net_by_root = BTreeMap::new();
        for (index, pending) in pending_nets.iter().enumerate() {
            net_by_root.insert(pending.root, OutNetId(index));
        }

        let mut owner_anchor_by_net = vec![None; pending_nets.len()];
        let mut parent_by_net = vec![None; pending_nets.len()];
        for (index, pending) in pending_nets.iter_mut().enumerate() {
            pending.ports.sort_unstable();
            pending.producers.sort_unstable();
            pending.owner_anchors.sort_unstable();
            pending.owner_anchors.dedup();
            let id = OutNetId(index);
            match pending.producers.as_slice() {
                [] => self.diagnostics.push(OutNetDiagnostic::MissingProducer {
                    net: id,
                    ports: pending.ports.clone(),
                }),
                [_] => {}
                producers => self.diagnostics.push(OutNetDiagnostic::MultipleProducers {
                    net: id,
                    producers: producers.to_vec(),
                }),
            }
            let anchor = match pending.owner_anchors.as_slice() {
                [anchor] => Some(*anchor),
                [] => {
                    self.diagnostics
                        .push(OutNetDiagnostic::MissingOwnerAnchor { net: id });
                    None
                }
                anchors => {
                    self.diagnostics
                        .push(OutNetDiagnostic::MultipleOwnerAnchors {
                            net: id,
                            anchors: anchors.to_vec(),
                        });
                    None
                }
            };
            owner_anchor_by_net[index] = anchor;
            if let Some(parent_node) = anchor.and_then(|anchor| {
                let call = self.ports[anchor.as_usize()].call;
                self.call_instances[call.as_usize()].parent_output_node
            }) {
                let parent_root = self.union_find.find(parent_node);
                let parent = net_by_root[&parent_root];
                if parent == id {
                    self.diagnostics
                        .push(OutNetDiagnostic::OwnerCycle { net: id });
                } else {
                    parent_by_net[index] = Some(parent);
                }
            }
        }

        let resource_calls = self
            .call_instances
            .iter()
            .filter(|call| {
                self.resource_owning_callables
                    .contains(&call.provenance.callable)
                    || self.producer_identity_by_call.contains_key(&call.id)
            })
            .map(|call| call.id)
            .collect::<BTreeSet<_>>();
        let nearest_resource_call = |mut call: Option<OutCallInstanceId>| {
            while let Some(candidate) = call {
                if resource_calls.contains(&candidate) {
                    return Some(candidate);
                }
                call = self.call_instances[candidate.as_usize()].parent;
            }
            None
        };

        let parent_output_net_by_call = self
            .call_instances
            .iter()
            .map(|instance| {
                instance.parent_output_node.map(|node| {
                    let root = self.union_find.find(node);
                    net_by_root[&root]
                })
            })
            .collect::<Vec<_>>();

        let owner_parent = |parent_output: Option<OutNetId>,
                            resource_parent: Option<OutCallInstanceId>| {
            match (parent_output, resource_parent) {
                (Some(output), Some(resource))
                    if parent_output_net_by_call[resource.as_usize()] == Some(output) =>
                {
                    Some(StaticOwnerNode::Call(resource))
                }
                (Some(output), _) => Some(StaticOwnerNode::Net(output)),
                (None, Some(resource)) => Some(StaticOwnerNode::Call(resource)),
                (None, None) => None,
            }
        };

        let mut children = BTreeMap::<Option<StaticOwnerNode>, Vec<StaticOwnerNode>>::new();
        for call in resource_calls.iter().copied() {
            let instance = &self.call_instances[call.as_usize()];
            let parent = owner_parent(
                parent_output_net_by_call[call.as_usize()],
                nearest_resource_call(instance.parent),
            );
            children
                .entry(parent)
                .or_default()
                .push(StaticOwnerNode::Call(call));
        }
        for (index, parent_net) in parent_by_net.iter().copied().enumerate() {
            let Some(anchor) = owner_anchor_by_net[index] else {
                continue;
            };
            let anchor_call = self.ports[anchor.as_usize()].call;
            let parent = owner_parent(parent_net, nearest_resource_call(Some(anchor_call)));
            children
                .entry(parent)
                .or_default()
                .push(StaticOwnerNode::Net(OutNetId(index)));
        }
        for siblings in children.values_mut() {
            siblings.sort_by_key(|node| {
                let producer_identity = match *node {
                    StaticOwnerNode::Call(call) => {
                        self.producer_identity_by_call.get(&call).copied()
                    }
                    StaticOwnerNode::Net(_) => None,
                };
                let (scope_span, expression_span, expression, kind, ordinal) = match *node {
                    StaticOwnerNode::Net(net) => {
                        let anchor = owner_anchor_by_net[net.as_usize()]
                            .expect("owner forest contains only anchored nets");
                        let port = &self.ports[anchor.as_usize()];
                        let call = port.call;
                        let expression = self.call_instances[call.as_usize()].provenance.expression;
                        let expression_span = checked_expression(self.program, expression)
                            .map(|expression| expression.span)
                            .unwrap_or_default();
                        let scope_span = match port.binding {
                            OutPortBinding::Fresh { scope_id, .. } => {
                                checked_scope(self.program, scope_id).map(|scope| scope.span)
                            }
                            OutPortBinding::Forward { .. } => None,
                        }
                        .unwrap_or(expression_span);
                        (
                            scope_span,
                            expression_span,
                            expression,
                            1_u8,
                            net.as_usize(),
                        )
                    }
                    StaticOwnerNode::Call(call) => {
                        let expression = self.call_instances[call.as_usize()].provenance.expression;
                        let span = checked_expression(self.program, expression)
                            .map(|expression| expression.span)
                            .unwrap_or_default();
                        (span, span, expression, 0_u8, call.as_usize())
                    }
                };
                (
                    producer_identity.is_some(),
                    producer_identity.unwrap_or([0; 32]),
                    scope_span.start,
                    scope_span.end,
                    expression_span.start,
                    expression_span.end,
                    expression,
                    kind,
                    ordinal,
                )
            });
        }

        fn assign_owner_tree(
            parent_node: Option<StaticOwnerNode>,
            parent_owner: Option<StaticOwnerId>,
            children: &BTreeMap<Option<StaticOwnerNode>, Vec<StaticOwnerNode>>,
            owner_by_net: &mut [Option<StaticOwnerId>],
            owner_by_call: &mut [Option<StaticOwnerId>],
            owners: &mut Vec<StaticOwnerDef>,
        ) {
            for (child_ordinal, node) in children
                .get(&parent_node)
                .into_iter()
                .flatten()
                .copied()
                .enumerate()
            {
                let existing = match node {
                    StaticOwnerNode::Net(net) => owner_by_net[net.as_usize()],
                    StaticOwnerNode::Call(call) => owner_by_call[call.as_usize()],
                };
                if existing.is_some() {
                    continue;
                }
                let owner = StaticOwnerId(owners.len());
                match node {
                    StaticOwnerNode::Net(net) => owner_by_net[net.as_usize()] = Some(owner),
                    StaticOwnerNode::Call(call) => owner_by_call[call.as_usize()] = Some(owner),
                }
                owners.push(StaticOwnerDef {
                    id: owner,
                    parent: parent_owner,
                    child_ordinal: child_ordinal as u32,
                });
                assign_owner_tree(
                    Some(node),
                    Some(owner),
                    children,
                    owner_by_net,
                    owner_by_call,
                    owners,
                );
            }
        }

        let mut owner_by_net = vec![None; pending_nets.len()];
        let mut owner_by_call = vec![None; self.call_instances.len()];
        let mut static_owners = Vec::new();
        assign_owner_tree(
            None,
            None,
            &children,
            &mut owner_by_net,
            &mut owner_by_call,
            &mut static_owners,
        );
        for (index, anchor) in owner_anchor_by_net.iter().enumerate() {
            if anchor.is_some() && owner_by_net[index].is_none() {
                self.diagnostics.push(OutNetDiagnostic::OwnerCycle {
                    net: OutNetId(index),
                });
            }
        }
        for call in resource_calls {
            self.call_instances[call.as_usize()].owner = owner_by_call[call.as_usize()];
        }

        let nets = pending_nets
            .into_iter()
            .enumerate()
            .map(|(index, pending)| UnifiedOutNet {
                id: OutNetId(index),
                ports: pending.ports,
                producers: pending
                    .producers
                    .iter()
                    .map(|port_id| StructuralProducer {
                        port: *port_id,
                        call: self.ports[port_id.as_usize()].call,
                    })
                    .collect(),
                owner: owner_by_net[index],
                owner_anchor: owner_anchor_by_net[index],
            })
            .collect::<Vec<_>>();

        let ports = self
            .ports
            .into_iter()
            .map(|port| {
                let root = self.union_find.find(port.union_node);
                OutPort {
                    id: port.id,
                    call: port.call,
                    entry_ordinal: port.entry_ordinal,
                    formal: port.formal,
                    name: port.name,
                    binding: port.binding,
                    contract: port.contract,
                    net: net_by_root[&root],
                }
            })
            .collect::<Vec<_>>();

        let call_instances = self.call_instances;
        let mut call_instance_by_checked_frame = BTreeMap::new();
        let mut output_net_by_frame_target = BTreeMap::new();
        let mut concrete_producers_by_checked =
            BTreeMap::<CheckedCallId, Vec<ConcreteOutProducer>>::new();
        for call in &call_instances {
            if let Some(checked_call) = call.provenance.call_id {
                insert_unique_index(
                    &mut call_instance_by_checked_frame,
                    (checked_call, call.parent),
                    call.id,
                );
            }
            for port_id in &call.ports {
                let port = &ports[port_id.as_usize()];
                insert_unique_index(
                    &mut output_net_by_frame_target,
                    (Some(call.id), port.formal),
                    port.net,
                );
                if let OutPortBinding::Fresh { output, .. } = port.binding {
                    insert_unique_index(
                        &mut output_net_by_frame_target,
                        (call.parent, output),
                        port.net,
                    );
                }
                let net = &nets[port.net.as_usize()];
                if net
                    .producers
                    .iter()
                    .any(|producer| producer.port == *port_id)
                    && let Some(owner) = net.owner
                    && let Some(checked_call) = call.provenance.call_id
                {
                    concrete_producers_by_checked
                        .entry(checked_call)
                        .or_default()
                        .push(ConcreteOutProducer {
                            call: call.id,
                            port: *port_id,
                            net: net.id,
                            owner,
                        });
                }
            }
        }
        for producers in concrete_producers_by_checked.values_mut() {
            producers.sort_by_key(|producer| (producer.owner, producer.call, producer.port));
        }

        let producer_root_by_identity = self
            .producer_roots
            .iter()
            .map(|root| (root.spec.identity, root.call))
            .collect::<BTreeMap<_, _>>();
        let producer_root_calls = producer_root_by_identity.values().copied().collect();
        let mut intentionally_elided_calls = self.intentionally_elided_calls;
        intentionally_elided_calls.retain(|key| !call_instance_by_checked_frame.contains_key(key));

        OutNetBuild {
            graph: OutNet {
                call_instances,
                ports,
                nets,
                static_owners,
                call_instance_by_checked_frame,
                intentionally_elided_calls,
                intentionally_elided_frames: self.intentionally_elided_frames,
                output_net_by_frame_target,
                concrete_producers_by_checked,
                producer_roots: self.producer_roots,
                producer_root_by_identity,
                producer_root_calls,
            },
            diagnostics: self.diagnostics,
        }
    }
}

#[derive(Default)]
struct UnionFind {
    parents: Vec<usize>,
    ranks: Vec<u8>,
}

impl UnionFind {
    fn make_set(&mut self) -> usize {
        let node = self.parents.len();
        self.parents.push(node);
        self.ranks.push(0);
        node
    }

    fn find(&mut self, node: usize) -> usize {
        let parent = self.parents[node];
        if parent != node {
            self.parents[node] = self.find(parent);
        }
        self.parents[node]
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left_root = self.find(left);
        let mut right_root = self.find(right);
        if left_root == right_root {
            return;
        }
        if self.ranks[left_root] < self.ranks[right_root] {
            std::mem::swap(&mut left_root, &mut right_root);
        }
        self.parents[right_root] = left_root;
        if self.ranks[left_root] == self.ranks[right_root] {
            self.ranks[left_root] += 1;
        }
    }
}

fn function_owner_for_scope(
    program: &CheckedProgramFields,
    mut scope: LexicalScopeId,
) -> Option<DeclId> {
    loop {
        let checked = checked_scope(program, scope)?;
        if checked.kind == CheckedScopeKind::Function {
            return checked.owner;
        }
        scope = checked.parent?;
    }
}

fn checked_expression(
    program: &CheckedProgramFields,
    expression: CheckedExprId,
) -> Option<&boon_checked::CheckedExpression> {
    program
        .expressions
        .get(expression.0 as usize)
        .filter(|candidate| candidate.id == expression)
}

fn checked_scope(
    program: &CheckedProgramFields,
    scope: LexicalScopeId,
) -> Option<&boon_checked::CheckedScope> {
    program
        .scopes
        .get(scope.0 as usize)
        .filter(|candidate| candidate.id == scope)
}

fn resource_owning_callables(
    program: &CheckedProgramFields,
    signatures: &BTreeMap<DeclId, &CheckedCallableSignature>,
    function_owner_by_scope: &[Option<DeclId>],
) -> BTreeSet<DeclId> {
    let mut owners = BTreeSet::new();

    for expression in &program.expressions {
        let Some(owner) = function_owner_by_scope
            .get(expression.scope_id.0 as usize)
            .copied()
            .flatten()
        else {
            continue;
        };
        let directly_allocates = match expression.kind {
            boon_checked::CheckedExpressionKind::Source
            | boon_checked::CheckedExpressionKind::Hold { .. }
            | boon_checked::CheckedExpressionKind::Latest { .. } => true,
            boon_checked::CheckedExpressionKind::Call { call } => program
                .calls
                .get(call.0 as usize)
                .filter(|candidate| candidate.id == call)
                .and_then(|call| signatures.get(&call.callable))
                .is_some_and(|callable| {
                    callable.kind != CheckedCallableKind::User
                        && (callable.effect.writes_state
                            || callable.effect.emits_source
                            || callable.effect.invokes_host)
                }),
            _ => false,
        };
        if directly_allocates {
            owners.insert(owner);
        }
    }

    for statement in &program.statements {
        if !matches!(
            statement.kind,
            boon_checked::CheckedStatementKind::List { .. }
        ) {
            continue;
        }
        if let Some(owner) = function_owner_by_scope
            .get(statement.scope_id.0 as usize)
            .copied()
            .flatten()
        {
            owners.insert(owner);
        }
    }
    owners
}

/// Retained pure bodies are shared, but a concrete invocation still needs a
/// sparse occurrence path to any call-local host context below it. Keep only
/// the retained user-call chain that reaches such a context; context-free
/// builtin work remains exclusively in the canonical definition body.
fn retained_overlay_definitions(
    program: &CheckedProgramFields,
    retained: &BTreeSet<DeclId>,
    signatures: &BTreeMap<DeclId, &CheckedCallableSignature>,
) -> BTreeSet<DeclId> {
    let mut required = BTreeSet::new();
    let mut retained_dependencies = BTreeMap::<DeclId, BTreeSet<DeclId>>::new();

    for call in &program.calls {
        let Some(owner) = call.owner_callable.filter(|owner| retained.contains(owner)) else {
            continue;
        };
        let Some(target) = signatures.get(&call.callable).copied() else {
            required.insert(owner);
            continue;
        };
        let direct_overlay = !call.contexts.is_empty()
            || call
                .entries
                .iter()
                .any(|entry| !matches!(entry, CheckedCallEntry::Input { .. }))
            || match target.kind {
                CheckedCallableKind::User => !retained.contains(&target.decl_id),
                CheckedCallableKind::Builtin | CheckedCallableKind::External => {
                    target.effect != boon_checked::CheckedEffectSummary::default()
                        || !target.contexts.is_empty()
                        || target.context_formal.is_some()
                }
            };
        if direct_overlay {
            required.insert(owner);
        } else if target.kind == CheckedCallableKind::User {
            retained_dependencies
                .entry(owner)
                .or_default()
                .insert(target.decl_id);
        }
    }

    loop {
        let newly_required = retained_dependencies
            .iter()
            .filter(|(owner, dependencies)| {
                !required.contains(owner)
                    && dependencies
                        .iter()
                        .any(|dependency| required.contains(dependency))
            })
            .map(|(owner, _)| *owner)
            .collect::<Vec<_>>();
        if newly_required.is_empty() {
            break;
        }
        required.extend(newly_required);
    }
    required
}

fn alias_cycle_diagnostics(program: &CheckedProgramFields) -> Vec<OutNetDiagnostic> {
    let edges = program
        .calls
        .iter()
        .flat_map(|call| {
            call.entries.iter().filter_map(move |entry| match entry {
                CheckedCallEntry::ForwardOut { formal, target, .. } => {
                    Some((*target, *formal, OutCallProvenance::from(call)))
                }
                _ => None,
            })
        })
        .collect::<Vec<_>>();
    cyclic_alias_components(&edges)
        .into_iter()
        .map(|declarations| {
            let declaration_set = declarations.iter().copied().collect::<BTreeSet<_>>();
            let mut call_sites = edges
                .iter()
                .filter(|(from, to, _)| {
                    declaration_set.contains(from) && declaration_set.contains(to)
                })
                .map(|(_, _, provenance)| *provenance)
                .collect::<Vec<_>>();
            call_sites.sort_unstable();
            call_sites.dedup();
            OutNetDiagnostic::AliasCycle {
                declarations,
                call_sites,
            }
        })
        .collect()
}

fn cyclic_alias_components(edges: &[(DeclId, DeclId, OutCallProvenance)]) -> Vec<Vec<DeclId>> {
    let mut graph = BTreeMap::<DeclId, Vec<DeclId>>::new();
    let mut reverse = BTreeMap::<DeclId, Vec<DeclId>>::new();
    let mut nodes = BTreeSet::new();
    for (from, to, _) in edges {
        graph.entry(*from).or_default().push(*to);
        reverse.entry(*to).or_default().push(*from);
        nodes.insert(*from);
        nodes.insert(*to);
    }
    for neighbors in graph.values_mut().chain(reverse.values_mut()) {
        neighbors.sort_unstable();
        neighbors.dedup();
    }

    fn postorder(
        node: DeclId,
        graph: &BTreeMap<DeclId, Vec<DeclId>>,
        visited: &mut BTreeSet<DeclId>,
        order: &mut Vec<DeclId>,
    ) {
        if !visited.insert(node) {
            return;
        }
        for next in graph.get(&node).into_iter().flatten().copied() {
            postorder(next, graph, visited, order);
        }
        order.push(node);
    }

    fn collect_component(
        node: DeclId,
        graph: &BTreeMap<DeclId, Vec<DeclId>>,
        visited: &mut BTreeSet<DeclId>,
        component: &mut Vec<DeclId>,
    ) {
        if !visited.insert(node) {
            return;
        }
        component.push(node);
        for next in graph.get(&node).into_iter().flatten().copied() {
            collect_component(next, graph, visited, component);
        }
    }

    let mut order = Vec::with_capacity(nodes.len());
    let mut visited = BTreeSet::new();
    for node in nodes.iter().copied() {
        postorder(node, &graph, &mut visited, &mut order);
    }

    visited.clear();
    let mut components = Vec::new();
    for node in order.into_iter().rev() {
        if visited.contains(&node) {
            continue;
        }
        let mut component = Vec::new();
        collect_component(node, &reverse, &mut visited, &mut component);
        component.sort_unstable();
        let cyclic = component.len() > 1
            || graph
                .get(&component[0])
                .is_some_and(|neighbors| neighbors.contains(&component[0]));
        if cyclic {
            components.push(component);
        }
    }
    components.sort_by_key(|component| component[0]);
    components
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_checked::CheckedProgram;

    fn checked_program(name: &str, source: &str) -> CheckedProgram {
        let parsed = boon_parser::parse_source(name, source).expect("fixture parses");
        let output = boon_typecheck::check_program(&parsed);
        assert!(
            !output.report.has_errors(),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
        output.program.expect("fixture typechecks")
    }

    fn retained_definition_build(program: &CheckedProgramFields) -> OutNetBuild {
        let retained = crate::contextual_expansion::ordinary_callable_declarations(program);
        OutNet::build_with_retained_definitions(
            program,
            Vec::new(),
            &retained,
            |_, _, _| (),
            |kind, _, _, _, _| kind == CheckedCallableKind::Builtin,
        )
    }

    #[test]
    fn retained_pure_definition_bodies_are_not_reinstantiated_per_call() {
        let program = checked_program(
            "out-net-retained-pure-definitions.bn",
            r#"
FUNCTION increment(value) {
    value + 1
}

FUNCTION wrapped(value) {
    increment(value: value)
}

first: wrapped(value: 1)
second: wrapped(value: 2)
"#,
        );
        let expanded = OutNet::build(&program);
        let retained = retained_definition_build(&program);
        assert!(!expanded.has_errors(), "{:#?}", expanded.diagnostics);
        assert!(!retained.has_errors(), "{:#?}", retained.diagnostics);
        assert_eq!(expanded.graph.call_instances.len(), 4);
        assert_eq!(retained.graph.call_instances.len(), 2);
        assert_eq!(retained.graph.ports, expanded.graph.ports);
        assert_eq!(retained.graph.nets, expanded.graph.nets);
        assert_eq!(retained.graph.static_owners, expanded.graph.static_owners);
        assert!(retained.graph.call_instances.iter().all(|instance| {
            instance
                .provenance
                .call_id
                .and_then(|call| program.calls.iter().find(|candidate| candidate.id == call))
                .is_some_and(|call| call.function.ends_with("wrapped"))
        }));
    }

    #[test]
    fn retained_render_definitions_keep_only_the_concrete_context_overlay_path() {
        let program = checked_program(
            "out-net-retained-render-overlay.bn",
            r#"
FUNCTION identity(value) {
    value
}

FUNCTION label(value) {
    Scene/Element/text(
        element: []
        style: []
        text: identity(value: value)
    )
}

FUNCTION outer(value) {
    label(value: value)
}

result: outer(value: TEXT { hello })
"#,
        );
        let retained_definitions =
            crate::contextual_expansion::ordinary_callable_declarations(&program);
        for name in ["identity", "label", "outer"] {
            let callable = program
                .callables
                .iter()
                .find(|callable| callable.name.ends_with(name))
                .unwrap_or_else(|| panic!("missing `{name}` callable"));
            assert!(retained_definitions.contains(&callable.decl_id));
        }

        let expanded = OutNet::build(&program);
        let demanded = retained_definition_build(&program);
        assert!(!expanded.has_errors(), "{:#?}", expanded.diagnostics);
        assert!(!demanded.has_errors(), "{:#?}", demanded.diagnostics);
        assert_eq!(expanded.graph.call_instances.len(), 4);
        assert_eq!(demanded.graph.call_instances.len(), 3);

        let functions = demanded
            .graph
            .call_instances
            .iter()
            .map(|instance| {
                let call = instance.provenance.call_id.expect("authored call overlay");
                let function = program
                    .calls
                    .iter()
                    .find(|candidate| candidate.id == call)
                    .expect("overlay call exists")
                    .function
                    .as_str();
                (function, instance.parent)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            functions,
            vec![
                ("outer", None),
                ("label", Some(OutCallInstanceId(0))),
                ("Scene/Element/text", Some(OutCallInstanceId(1))),
            ]
        );
        assert!(
            demanded.graph.call_instances.iter().all(|instance| {
                instance
                    .provenance
                    .call_id
                    .and_then(|call| program.calls.iter().find(|candidate| candidate.id == call))
                    .is_none_or(|call| call.function != "identity")
            }),
            "context-free identity must stay only in the shared body"
        );
        let text_call = program
            .calls
            .iter()
            .find(|call| call.function == "Scene/Element/text")
            .expect("text call");
        assert!(!text_call.contexts.is_empty());
    }

    #[test]
    fn retained_definition_demand_keeps_out_and_resource_owning_bodies_concrete() {
        let out_program = checked_program(
            "out-net-retained-out-owner.bn",
            r#"
FUNCTION mapped(list, row: OUT, new) {
    list |> List/map(item: row, new: new)
}

rows: LIST { [value: 1] }
result: rows |> mapped(row, new: row.value + 1)
"#,
        );
        let retained_out =
            crate::contextual_expansion::ordinary_callable_declarations(&out_program);
        let mapped = out_program
            .callables
            .iter()
            .find(|callable| callable.name.ends_with("mapped"))
            .expect("mapped callable");
        assert!(!retained_out.contains(&mapped.decl_id));
        let expanded = OutNet::build(&out_program);
        let demanded = retained_definition_build(&out_program);
        assert!(!expanded.has_errors(), "{:#?}", expanded.diagnostics);
        assert!(!demanded.has_errors(), "{:#?}", demanded.diagnostics);
        assert_eq!(demanded.graph, expanded.graph);

        let resource_program = checked_program(
            "out-net-retained-resource-owner.bn",
            r#"
FUNCTION controls() {
    [press: SOURCE]
}

result: controls()
"#,
        );
        let retained_resource =
            crate::contextual_expansion::ordinary_callable_declarations(&resource_program);
        let controls = resource_program
            .callables
            .iter()
            .find(|callable| callable.name.ends_with("controls"))
            .expect("controls callable");
        assert!(!retained_resource.contains(&controls.decl_id));
        let demanded = retained_definition_build(&resource_program);
        assert!(!demanded.has_errors(), "{:#?}", demanded.diagnostics);
        assert_eq!(demanded.graph.call_instances.len(), 1);
        assert_eq!(
            demanded.graph.call_instances[0].provenance.callable,
            controls.decl_id
        );
    }

    #[test]
    fn occurrence_specific_builtin_results_survive_out_elaboration() {
        let program = checked_program(
            "out-net-static-bits-results.bn",
            r#"
bits: BITS[8] { 2u10100011 }
slice: bits |> Bits/slice(from: 2, count: 3)
converted: 255 |> Number/to_bits(width: 8, interpretation: Unsigned)
"#,
        );
        let built = OutNet::build(&program);
        assert!(!built.has_errors(), "{:#?}", built.diagnostics);

        for function in ["Bits/slice", "Number/to_bits"] {
            let call = program
                .calls
                .iter()
                .find(|call| call.function == function)
                .unwrap_or_else(|| panic!("missing checked call `{function}`"));
            let instance = built
                .graph
                .call_instances
                .iter()
                .find(|instance| instance.provenance.call_id == Some(call.id))
                .unwrap_or_else(|| panic!("missing OUT instance for `{function}`"));
            assert_eq!(
                instance.result, call.result,
                "checked call: {call:#?}\nOUT instance: {instance:#?}"
            );
        }
        assert_eq!(
            program
                .calls
                .iter()
                .find(|call| call.function == "Bits/slice")
                .unwrap()
                .result
                .ty,
            boon_checked::Type::Bits { width: 3 }
        );
    }

    #[test]
    fn closed_equivalent_checked_occurrence_is_the_result_authority() {
        assert_eq!(
            specialize_checked_occurrence_result(
                &Type::Union(vec![Type::Text, Type::Text]),
                &Type::Text,
            ),
            Type::Text
        );
        assert_eq!(
            specialize_checked_occurrence_result(
                &Type::Union(vec![Type::Text, Type::Number]),
                &Type::Text,
            ),
            Type::Union(vec![Type::Text, Type::Number]),
            "a non-equivalent occurrence must not narrow a closed callable result"
        );
    }

    #[test]
    fn call_frame_substitution_uses_equivalent_closed_checked_occurrence() {
        let (mut program, _) = checked_program(
            "out-net-collapsed-result-union.bn",
            r#"
FUNCTION identity(value) {
    value
}
result: identity(value: TEXT { alpha })
"#,
        )
        .into_parts();
        let call_id = program
            .calls
            .iter()
            .find(|call| call.function == "identity")
            .expect("identity call")
            .id;
        let callable_id = program
            .calls
            .iter()
            .find(|call| call.id == call_id)
            .expect("identity call")
            .callable;
        let first = TypeVar(10_000);
        let second = TypeVar(10_001);
        program
            .callables
            .iter_mut()
            .find(|callable| callable.decl_id == callable_id)
            .expect("identity callable")
            .result
            .ty = Type::Union(vec![Type::Var(first), Type::Var(second)]);
        let call = program
            .calls
            .iter_mut()
            .find(|call| call.id == call_id)
            .expect("identity call");
        call.type_substitutions = vec![
            boon_checked::CheckedTypeSubstitution {
                variable: first,
                value: Type::Text,
            },
            boon_checked::CheckedTypeSubstitution {
                variable: second,
                value: Type::Text,
            },
        ];
        call.result.ty = Type::Text;

        let built = OutNet::build(&program);
        assert!(!built.has_errors(), "{:#?}", built.diagnostics);
        let instance = built
            .graph
            .call_instances
            .iter()
            .find(|instance| instance.provenance.call_id == Some(call_id))
            .expect("identity call instance");
        assert_eq!(instance.result.ty, Type::Text);
    }

    #[test]
    fn static_dispatch_instantiates_only_the_selected_call_path() {
        let mut source = String::new();
        for index in 0..64 {
            source.push_str(&format!(
                "FUNCTION branch_{index}() {{\n    {index}\n}}\n\n"
            ));
        }
        source.push_str("FUNCTION dispatch(choice) {\n    choice |> WHEN {\n");
        for index in 0..64 {
            source.push_str(&format!("        Choice{index} => branch_{index}()\n"));
        }
        source.push_str("    }\n}\n\nresult: dispatch(choice: Choice0)\n");

        let program = checked_program("out-net-static-dispatch.bn", &source);
        let built = OutNet::build(&program);
        assert!(!built.has_errors(), "{:#?}", built.diagnostics);
        assert_eq!(built.graph.call_instances.len(), 2);
        assert_eq!(
            built
                .graph
                .call_instances
                .iter()
                .filter_map(|instance| instance.provenance.call_id)
                .filter_map(|call| {
                    program
                        .calls
                        .iter()
                        .find(|candidate| candidate.id == call)
                        .map(|call| call.function.as_str())
                })
                .collect::<Vec<_>>(),
            vec!["dispatch", "branch_0"]
        );
    }

    #[test]
    fn static_dispatch_uses_singleton_projection_from_a_checked_call_result() {
        let program = checked_program(
            "out-net-static-call-result-projection.bn",
            r#"
FUNCTION make_row() {
    [item_kind: VariableRow]
}

FUNCTION variable_row(row) {
    [kind: row.item_kind]
}

FUNCTION group_row(row) {
    [group: row.item_kind]
}

FUNCTION dispatch(row) {
    row.item_kind |> WHEN {
        VariableRow => variable_row(row: row)
        __ => group_row(row: row)
    }
}

result: dispatch(row: make_row())
"#,
        );
        let built = OutNet::build(&program);
        assert!(!built.has_errors(), "{:#?}", built.diagnostics);
        let functions = built
            .graph
            .call_instances
            .iter()
            .filter_map(|instance| instance.provenance.call_id)
            .filter_map(|call| {
                program
                    .calls
                    .iter()
                    .find(|candidate| candidate.id == call)
                    .map(|call| call.function.as_str())
            })
            .collect::<Vec<_>>();
        assert!(functions.contains(&"make_row"));
        assert!(functions.contains(&"dispatch"));
        assert!(functions.contains(&"variable_row"));
        assert!(!functions.contains(&"group_row"));
    }

    #[test]
    fn static_dispatch_uses_singleton_projection_from_a_contextual_out_row() {
        let program = checked_program(
            "out-net-static-contextual-row-projection.bn",
            r#"
FUNCTION variable_row(row) {
    [kind: row.item_kind]
}

FUNCTION group_row(row) {
    [group: row.item_kind]
}

FUNCTION dispatch(row) {
    row.item_kind |> WHEN {
        VariableRow => variable_row(row: row)
        __ => group_row(row: row)
    }
}

result:
    LIST { [item_kind: VariableRow] }
    |> List/map(item, new: dispatch(row: item))
"#,
        );
        let built = OutNet::build(&program);
        assert!(!built.has_errors(), "{:#?}", built.diagnostics);
        let functions = built
            .graph
            .call_instances
            .iter()
            .filter_map(|instance| instance.provenance.call_id)
            .filter_map(|call| {
                program
                    .calls
                    .iter()
                    .find(|candidate| candidate.id == call)
                    .map(|call| call.function.as_str())
            })
            .collect::<Vec<_>>();
        assert!(functions.contains(&"List/map"));
        assert!(functions.contains(&"dispatch"));
        assert!(functions.contains(&"variable_row"));
        assert!(!functions.contains(&"group_row"));
    }

    #[test]
    fn static_structural_dispatch_keeps_selected_statement_child_calls() {
        let program = checked_program(
            "out-net-static-structural-dispatch.bn",
            r#"
FUNCTION branch_a() {
    [a: 1]
}

FUNCTION branch_b() {
    [b: 2]
}

FUNCTION dispatch(choice) {
    choice |> WHEN {
        A => [
            ...branch_a()
        ]

        __ => [
            ...branch_b()
        ]
    }
}

result: dispatch(choice: A)
"#,
        );
        let built = OutNet::build(&program);
        assert!(!built.has_errors(), "{:#?}", built.diagnostics);
        let functions = built
            .graph
            .call_instances
            .iter()
            .filter_map(|instance| instance.provenance.call_id)
            .filter_map(|call| {
                program
                    .calls
                    .iter()
                    .find(|candidate| candidate.id == call)
                    .map(|call| call.function.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(functions, vec!["dispatch", "branch_a"]);
    }

    #[test]
    fn dynamic_dispatch_retains_every_reachable_call_path() {
        let program = checked_program(
            "out-net-dynamic-dispatch.bn",
            r#"
FUNCTION branch_a() {
    1
}

FUNCTION branch_b() {
    2
}

FUNCTION branch_c() {
    3
}

FUNCTION dispatch(choice) {
    choice |> WHEN {
        A => branch_a()
        B => branch_b()
        C => branch_c()
    }
}

choice:
    A |> HOLD selected {
        LATEST {}
    }
result: dispatch(choice: choice)
"#,
        );
        let built = OutNet::build(&program);
        assert!(!built.has_errors(), "{:#?}", built.diagnostics);
        let functions = built
            .graph
            .call_instances
            .iter()
            .filter_map(|instance| instance.provenance.call_id)
            .filter_map(|call| {
                program
                    .calls
                    .iter()
                    .find(|candidate| candidate.id == call)
                    .map(|call| call.function.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            functions,
            vec!["dispatch", "branch_a", "branch_b", "branch_c"]
        );
    }

    #[test]
    fn separate_wrapper_calls_get_separate_static_owners() {
        let wrapped = checked_program(
            "out-net-two-wrappers.bn",
            r#"
FUNCTION wrapped(list, entry: OUT, new) {
    list |> List/map(item: entry, new: new)
}

rows: LIST { [value: 1] }
first: rows |> wrapped(entry, new: entry.value + 1)
second: rows |> wrapped(entry, new: entry.value + 2)
"#,
        );
        let first = OutNet::build(&wrapped);
        assert!(!first.has_errors(), "{:#?}", first.diagnostics);
        assert_eq!(first.graph.call_instances.len(), 4);
        assert_eq!(first.graph.nets.len(), 2);
        assert_eq!(
            first
                .graph
                .nets
                .iter()
                .map(|net| net.id.as_usize())
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert!(
            first
                .graph
                .nets
                .iter()
                .all(|net| net.producers.len() == 1 && net.ports.len() == 2)
        );

        assert_eq!(
            first.graph.static_owners,
            vec![
                StaticOwnerDef {
                    id: StaticOwnerId(0),
                    parent: None,
                    child_ordinal: 0,
                },
                StaticOwnerDef {
                    id: StaticOwnerId(1),
                    parent: None,
                    child_ordinal: 1,
                },
            ]
        );

        let direct = checked_program(
            "out-net-two-direct.bn",
            r#"
rows: LIST { [value: 1] }
first: rows |> List/map(item, new: item.value + 1)
second: rows |> List/map(item, new: item.value + 2)
"#,
        );
        let direct = OutNet::build(&direct);
        assert!(!direct.has_errors(), "{:#?}", direct.diagnostics);
        assert_eq!(first.graph.static_owners, direct.graph.static_owners);
        assert!(
            direct
                .graph
                .nets
                .iter()
                .all(|net| net.ports.len() == 1 && net.producers.len() == 1)
        );
    }

    #[test]
    fn multiple_wrapper_layers_erase_to_the_direct_owner_forest() {
        let wrapped_program = checked_program(
            "out-net-multiple-wrappers.bn",
            r#"
FUNCTION wrapped(list, entry: OUT, new) {
    list |> List/map(item: entry, new: new)
}

FUNCTION outer(list, entry: OUT, new) {
    list |> wrapped(entry: entry, new: new)
}

rows: LIST { [value: 1] }
result: rows |> outer(entry, new: entry.value + 1)
"#,
        );
        let wrapped = OutNet::build(&wrapped_program);
        assert!(!wrapped.has_errors(), "{:#?}", wrapped.diagnostics);

        let direct_program = checked_program(
            "out-net-one-direct.bn",
            r#"
rows: LIST { [value: 1] }
result: rows |> List/map(item, new: item.value + 1)
"#,
        );
        let direct = OutNet::build(&direct_program);
        assert!(!direct.has_errors(), "{:#?}", direct.diagnostics);
        assert_eq!(wrapped.graph.static_owners, direct.graph.static_owners);
        let map_call = wrapped_program
            .calls
            .iter()
            .find(|call| call.function == "List/map")
            .expect("nested structural producer");
        let producers = wrapped
            .graph
            .concrete_producers_for_checked_call(map_call.id);
        let [producer] = producers.as_slice() else {
            panic!("multi-wrapper expansion must expose one concrete producer");
        };
        assert_eq!(producer.owner, StaticOwnerId(0));
        assert_eq!(wrapped.graph.net_for_port(producer.port), producer.net);
        assert_eq!(wrapped.graph.nets.len(), 1);
        assert_eq!(wrapped.graph.nets[0].ports.len(), 3);
    }

    #[test]
    fn repeated_output_scopes_form_real_owner_ancestry() {
        let program = checked_program(
            "out-net-repeated-owner-ancestry.bn",
            r#"
outer_rows: LIST { [value: 1] }
inner_rows: LIST { [value: 2] }

result:
    outer_rows
    |> List/map(item, new:
        inner_rows
        |> List/map(item, new: item.value + 1)
    )
"#,
        );
        let build = OutNet::build(&program);
        assert!(!build.has_errors(), "{:#?}", build.diagnostics);
        assert_eq!(
            build.graph.static_owners,
            vec![
                StaticOwnerDef {
                    id: StaticOwnerId(0),
                    parent: None,
                    child_ordinal: 0,
                },
                StaticOwnerDef {
                    id: StaticOwnerId(1),
                    parent: Some(StaticOwnerId(0)),
                    child_ordinal: 0,
                },
            ]
        );
        assert_eq!(build.graph.nets.len(), 2);
        assert_eq!(build.graph.static_owners[1].parent, Some(StaticOwnerId(0)));
    }

    #[test]
    fn checked_call_order_builds_a_deterministic_owner_forest() {
        let program = checked_program(
            "out-net-deterministic-order.bn",
            r#"
rows: LIST { [value: 1] }
first: rows |> List/map(item, new: item.value + 1)
second: rows |> List/map(item, new: item.value + 2)
"#,
        );
        let first = OutNet::build(&program);
        let repeated = OutNet::build(&program);
        assert!(!first.has_errors(), "{:#?}", first.diagnostics);
        assert_eq!(first, repeated);
        assert_eq!(
            first.graph.static_owners,
            vec![
                StaticOwnerDef {
                    id: StaticOwnerId(0),
                    parent: None,
                    child_ordinal: 0,
                },
                StaticOwnerDef {
                    id: StaticOwnerId(1),
                    parent: None,
                    child_ordinal: 1,
                },
            ]
        );
    }

    #[test]
    fn invalid_structural_producer_counts_never_cross_the_checked_boundary() {
        let missing = boon_parser::parse_source(
            "out-net-missing-producer.bn",
            r#"
FUNCTION missing(list, entry: OUT) {
    list
}

rows: LIST { [value: 1] }
result: rows |> missing(entry)
"#,
        )
        .unwrap();
        let missing = boon_typecheck::check_program(&missing);
        assert!(
            missing
                .report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("has no structural producer"))
        );
        assert!(missing.program.is_none());

        let multiple = boon_parser::parse_source(
            "out-net-multiple-producers.bn",
            r#"
FUNCTION multiple(list, entry: OUT) {
    [
        first: list |> List/map(item: entry, new: entry.value + 1)
        second: list |> List/map(item: entry, new: entry.value + 2)
    ]
}

rows: LIST { [value: 1] }
result: rows |> multiple(entry)
"#,
        )
        .unwrap();
        let multiple = boon_typecheck::check_program(&multiple);
        assert!(
            multiple
                .report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic
                    .message
                    .contains("structural producers; exactly one is required")),
            "diagnostics: {:#?}",
            multiple.report.diagnostics
        );
        assert!(multiple.program.is_none());
    }

    #[test]
    fn reports_forwarding_alias_cycles() {
        let first = DeclId(30);
        let second = DeclId(40);
        let provenance = |callable, expression| OutCallProvenance {
            call_id: None,
            expression: CheckedExprId(expression),
            owner_callable: None,
            callable,
        };
        let components = cyclic_alias_components(&[
            (first, second, provenance(first, 1)),
            (second, first, provenance(second, 2)),
            (DeclId(50), DeclId(60), provenance(DeclId(50), 3)),
        ]);
        assert_eq!(components, vec![vec![first, second]]);
    }
}
