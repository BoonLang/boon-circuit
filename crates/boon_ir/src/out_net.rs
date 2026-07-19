//! Compile-time elaboration of checked `OUT` bindings.
//!
//! This graph is intentionally private to `boon_ir`. It retains enough checked
//! provenance for diagnostics and later structural-owner interning, but it is
//! not a runtime value and is not serializable.

use super::{StaticOwnerDef, StaticOwnerId};
use boon_typecheck::{
    CheckedCall, CheckedCallEntry, CheckedCallId, CheckedCallableKind, CheckedCallableSignature,
    CheckedEvaluationScope, CheckedExprId, CheckedProgram, CheckedScopeKind, DeclId,
    LexicalScopeId,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

macro_rules! typed_out_id {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
            pub(crate) struct $name(usize);

            impl $name {
                pub(crate) const fn as_usize(self) -> usize {
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

typed_out_id!(OutCallInstanceId, OutPortId, OutNetId);

/// Stable checked-program coordinates for one static call site.
///
/// `pass` is deliberately excluded: `PASS` is context for expansion,
/// not part of executable ownership.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct OutCallProvenance {
    pub(crate) call_id: CheckedCallId,
    pub(crate) expression: CheckedExprId,
    pub(crate) owner_callable: Option<DeclId>,
    pub(crate) callable: DeclId,
}

impl From<&CheckedCall> for OutCallProvenance {
    fn from(call: &CheckedCall) -> Self {
        Self {
            call_id: call.id,
            expression: call.expression,
            owner_callable: call.owner_callable,
            callable: call.callable,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OutCallInstance {
    pub(crate) id: OutCallInstanceId,
    pub(crate) parent: Option<OutCallInstanceId>,
    pub(crate) provenance: OutCallProvenance,
    pub(crate) parent_output: Option<DeclId>,
    parent_output_node: Option<usize>,
    pub(crate) inputs: Vec<OutInputBinding>,
    pub(crate) passed: Option<ScopedCheckedExpr>,
    pub(crate) ports: Vec<OutPortId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScopedCheckedExpr {
    pub(crate) expression: CheckedExprId,
    /// The concrete user-call frame in which this expression was written.
    pub(crate) frame: Option<OutCallInstanceId>,
    /// A call-local output formal under which this argument is evaluated.
    pub(crate) evaluation_port: Option<OutPortId>,
    /// A standalone pure-function binding frame used outside a concrete call site.
    pub(crate) value_frame: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OutInputBinding {
    pub(crate) formal: DeclId,
    pub(crate) value: ScopedCheckedExpr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutPortBinding {
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OutPort<Contract = ()> {
    pub(crate) id: OutPortId,
    pub(crate) call: OutCallInstanceId,
    pub(crate) entry_ordinal: usize,
    pub(crate) formal: DeclId,
    pub(crate) name: String,
    pub(crate) binding: OutPortBinding,
    pub(crate) contract: Contract,
    pub(crate) net: OutNetId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StructuralProducer {
    pub(crate) port: OutPortId,
    pub(crate) call: OutCallInstanceId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UnifiedOutNet {
    pub(crate) id: OutNetId,
    pub(crate) ports: Vec<OutPortId>,
    pub(crate) producers: Vec<StructuralProducer>,
    pub(crate) owner: Option<StaticOwnerId>,
    pub(crate) owner_anchor: Option<OutPortId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OutNet<Contract = ()> {
    pub(crate) call_instances: Vec<OutCallInstance>,
    pub(crate) ports: Vec<OutPort<Contract>>,
    pub(crate) nets: Vec<UnifiedOutNet>,
    pub(crate) static_owners: Vec<StaticOwnerDef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConcreteOutProducer {
    pub(crate) call: OutCallInstanceId,
    pub(crate) port: OutPortId,
    pub(crate) net: OutNetId,
    pub(crate) owner: StaticOwnerId,
}

impl<Contract> OutNet<Contract> {
    pub(crate) fn call_instance_for_checked_call(
        &self,
        call_id: CheckedCallId,
        frame: Option<OutCallInstanceId>,
    ) -> Option<OutCallInstanceId> {
        let mut matches = self
            .call_instances
            .iter()
            .filter(|call| call.provenance.call_id == call_id && call.parent == frame)
            .map(|call| call.id);
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }

    pub(crate) fn net_for_port(&self, port: OutPortId) -> OutNetId {
        self.ports[port.as_usize()].net
    }

    pub(crate) fn owner_for_net(&self, net: OutNetId) -> Option<StaticOwnerId> {
        self.nets[net.as_usize()].owner
    }

    pub(crate) fn owner_scope_for_net(&self, net: OutNetId) -> Option<LexicalScopeId> {
        let anchor = self.nets[net.as_usize()].owner_anchor?;
        match self.ports[anchor.as_usize()].binding {
            OutPortBinding::Fresh { scope_id, .. } => Some(scope_id),
            OutPortBinding::Forward { .. } => None,
        }
    }

    pub(crate) fn concrete_producers_for_checked_call(
        &self,
        call_id: CheckedCallId,
    ) -> Vec<ConcreteOutProducer> {
        let mut producers = self
            .call_instances
            .iter()
            .filter(|call| call.provenance.call_id == call_id)
            .flat_map(|call| {
                call.ports.iter().filter_map(move |port_id| {
                    let port = &self.ports[port_id.as_usize()];
                    let net = &self.nets[port.net.as_usize()];
                    let is_producer = net
                        .producers
                        .iter()
                        .any(|producer| producer.port == *port_id);
                    is_producer.then_some(ConcreteOutProducer {
                        call: call.id,
                        port: *port_id,
                        net: net.id,
                        owner: net.owner?,
                    })
                })
            })
            .collect::<Vec<_>>();
        producers.sort_by_key(|producer| (producer.owner, producer.call, producer.port));
        producers
    }

    pub(crate) fn output_net_in_frame(
        &self,
        frame: Option<OutCallInstanceId>,
        target: DeclId,
    ) -> Option<OutNetId> {
        if let Some(frame) = frame {
            let call = &self.call_instances[frame.as_usize()];
            if let Some(net) = call.ports.iter().find_map(|port_id| {
                let port = &self.ports[port_id.as_usize()];
                (port.formal == target).then_some(port.net)
            }) {
                return Some(net);
            }
        }
        self.call_instances
            .iter()
            .filter(|call| call.parent == frame)
            .flat_map(|call| call.ports.iter())
            .find_map(|port_id| {
                let port = &self.ports[port_id.as_usize()];
                matches!(
                    port.binding,
                    OutPortBinding::Fresh { output, .. } if output == target
                )
                .then_some(port.net)
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OutNetBuild<Contract = ()> {
    pub(crate) graph: OutNet<Contract>,
    pub(crate) diagnostics: Vec<OutNetDiagnostic>,
}

impl<Contract> OutNetBuild<Contract> {
    pub(crate) fn has_errors(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OutNetDiagnostic {
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
    RecursiveContextualCall {
        call: OutCallInstanceId,
        callable: DeclId,
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
            Self::RecursiveContextualCall { call, callable } => write!(
                formatter,
                "OUT call instance {call} recursively expands callable declaration {}",
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

impl OutNet<()> {
    pub(crate) fn build(program: &CheckedProgram) -> OutNetBuild {
        Self::build_with(
            program,
            |_, _, _| (),
            |kind, _, _, _, _| kind == CheckedCallableKind::Builtin,
        )
    }
}

impl<Contract> OutNet<Contract> {
    /// Builds an `OutNet` while allowing richer checked contracts and producer
    /// capabilities to be supplied by a later schema without changing the
    /// current `CheckedProgram` adapter.
    pub(crate) fn build_with<MakeContract, IsProducer>(
        program: &CheckedProgram,
        make_contract: MakeContract,
        is_structural_producer: IsProducer,
    ) -> OutNetBuild<Contract>
    where
        MakeContract: FnMut(&CheckedCall, usize, &CheckedCallEntry) -> Contract,
        IsProducer:
            FnMut(CheckedCallableKind, &CheckedCall, usize, &CheckedCallEntry, &Contract) -> bool,
    {
        OutNetBuilder::new(program, make_contract, is_structural_producer).build()
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

struct OutNetBuilder<'program, Contract, MakeContract, IsProducer> {
    program: &'program CheckedProgram,
    signature_by_id: BTreeMap<DeclId, &'program CheckedCallableSignature>,
    calls_by_owner: BTreeMap<Option<DeclId>, Vec<usize>>,
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
        program: &'program CheckedProgram,
        make_contract: MakeContract,
        is_structural_producer: IsProducer,
    ) -> Self {
        let signature_by_id = program
            .callables
            .iter()
            .map(|signature| (signature.decl_id, signature))
            .collect();
        let mut calls_by_owner = BTreeMap::<Option<DeclId>, Vec<usize>>::new();
        for (index, call) in program.calls.iter().enumerate() {
            calls_by_owner
                .entry(call.owner_callable)
                .or_default()
                .push(index);
        }
        for calls in calls_by_owner.values_mut() {
            calls.sort_by_key(|index| {
                let call = &program.calls[*index];
                (call.expression, call.id, call.callable, *index)
            });
        }

        Self {
            program,
            signature_by_id,
            calls_by_owner,
            make_contract,
            is_structural_producer,
            call_instances: Vec::new(),
            ports: Vec::new(),
            producer_ports: BTreeSet::new(),
            union_find: UnionFind::default(),
            diagnostics: alias_cycle_diagnostics(program),
        }
    }

    fn build(mut self) -> OutNetBuild<Contract> {
        self.instantiate_frame(None, None, BTreeMap::new(), &mut Vec::new());
        self.finish()
    }

    fn instantiate_frame(
        &mut self,
        owner_callable: Option<DeclId>,
        parent: Option<OutCallInstanceId>,
        mut frame_bindings: BTreeMap<DeclId, usize>,
        active_callables: &mut Vec<DeclId>,
    ) {
        let static_calls = self
            .calls_by_owner
            .get(&owner_callable)
            .cloned()
            .unwrap_or_default();
        let mut pending_calls = Vec::with_capacity(static_calls.len());
        let mut pending_forwards = Vec::new();

        // Allocate every fresh declaration in the frame before resolving any
        // forwarding edge. DeclId resolution has already happened, so this is
        // deterministic and independent of checked-call storage order.
        for static_call_index in static_calls {
            let checked_call = self.program.calls[static_call_index].clone();
            let provenance = OutCallProvenance::from(&checked_call);
            let instance = OutCallInstanceId(self.call_instances.len());
            let inherited_parent_output_node =
                parent.and_then(|parent| self.call_instances[parent.as_usize()].parent_output_node);
            self.call_instances.push(OutCallInstance {
                id: instance,
                parent,
                provenance,
                parent_output: self.nearest_repeated_output(checked_call.expression),
                parent_output_node: inherited_parent_output_node,
                inputs: Vec::new(),
                passed: checked_call.pass.map(|expression| ScopedCheckedExpr {
                    expression,
                    frame: parent,
                    evaluation_port: None,
                    value_frame: None,
                }),
                ports: Vec::new(),
            });

            let kind = self
                .signature_by_id
                .get(&checked_call.callable)
                .map(|signature| signature.kind);
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
                let contract = (self.make_contract)(&checked_call, entry_ordinal, entry);
                let port = OutPortId(self.ports.len());
                let union_node = self.union_find.make_set();
                if kind.is_some_and(|kind| {
                    (self.is_structural_producer)(
                        kind,
                        &checked_call,
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
                        value: ScopedCheckedExpr {
                            expression: *value,
                            frame: parent,
                            evaluation_port,
                            value_frame: None,
                        },
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

        for pending in pending_calls {
            if pending.kind != Some(CheckedCallableKind::User) {
                continue;
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
        let mut scope = self
            .program
            .expressions
            .iter()
            .find(|candidate| candidate.id == expression)
            .map(|expression| expression.scope_id)?;
        loop {
            let checked_scope = self
                .program
                .scopes
                .iter()
                .find(|candidate| candidate.id == scope)?;
            if checked_scope.kind == CheckedScopeKind::RepeatedOutput {
                return checked_scope.owner;
            }
            scope = checked_scope.parent?;
        }
    }

    fn finish(mut self) -> OutNetBuild<Contract> {
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

        let mut children = BTreeMap::<Option<OutNetId>, Vec<OutNetId>>::new();
        for (index, parent) in parent_by_net.iter().copied().enumerate() {
            if owner_anchor_by_net[index].is_some() {
                children.entry(parent).or_default().push(OutNetId(index));
            }
        }
        for siblings in children.values_mut() {
            siblings.sort_by_key(|net| {
                let anchor = owner_anchor_by_net[net.as_usize()]
                    .expect("owner forest contains only anchored nets");
                let port = &self.ports[anchor.as_usize()];
                let call = port.call;
                let expression = self.call_instances[call.as_usize()].provenance.expression;
                let expression_span = self
                    .program
                    .expressions
                    .iter()
                    .find(|candidate| candidate.id == expression)
                    .map(|expression| expression.span)
                    .unwrap_or_default();
                let output_scope_span = match port.binding {
                    OutPortBinding::Fresh { scope_id, .. } => self
                        .program
                        .scopes
                        .iter()
                        .find(|scope| scope.id == scope_id)
                        .map(|scope| scope.span),
                    OutPortBinding::Forward { .. } => None,
                }
                .unwrap_or(expression_span);
                (
                    output_scope_span.start,
                    output_scope_span.end,
                    expression_span.start,
                    expression_span.end,
                    expression,
                    *net,
                )
            });
        }

        fn assign_owner_tree(
            parent_net: Option<OutNetId>,
            parent_owner: Option<StaticOwnerId>,
            children: &BTreeMap<Option<OutNetId>, Vec<OutNetId>>,
            owner_by_net: &mut [Option<StaticOwnerId>],
            owners: &mut Vec<StaticOwnerDef>,
        ) {
            for (child_ordinal, net) in children
                .get(&parent_net)
                .into_iter()
                .flatten()
                .copied()
                .enumerate()
            {
                if owner_by_net[net.as_usize()].is_some() {
                    continue;
                }
                let owner = StaticOwnerId(owners.len());
                owner_by_net[net.as_usize()] = Some(owner);
                owners.push(StaticOwnerDef {
                    id: owner,
                    parent: parent_owner,
                    child_ordinal: child_ordinal as u32,
                });
                assign_owner_tree(Some(net), Some(owner), children, owner_by_net, owners);
            }
        }

        let mut owner_by_net = vec![None; pending_nets.len()];
        let mut static_owners = Vec::new();
        assign_owner_tree(None, None, &children, &mut owner_by_net, &mut static_owners);
        for (index, anchor) in owner_anchor_by_net.iter().enumerate() {
            if anchor.is_some() && owner_by_net[index].is_none() {
                self.diagnostics.push(OutNetDiagnostic::OwnerCycle {
                    net: OutNetId(index),
                });
            }
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
            .collect();

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
            .collect();

        OutNetBuild {
            graph: OutNet {
                call_instances: self.call_instances,
                ports,
                nets,
                static_owners,
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

fn alias_cycle_diagnostics(program: &CheckedProgram) -> Vec<OutNetDiagnostic> {
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
    use boon_typecheck::{
        CheckedCallableSignature, CheckedEffectSummary, CheckedEvaluationScope, CheckedExpression,
        CheckedExpressionKind, CheckedParameter, CheckedParameterKind, CheckedScope, CheckedSpan,
        FlowMode, FlowType, ProgramRole, Type,
    };

    const WRAPPER: DeclId = DeclId(1);
    const WRAPPER_OUT: DeclId = DeclId(2);
    const BUILTIN: DeclId = DeclId(10);
    const BUILTIN_OUT: DeclId = DeclId(11);

    fn unknown_flow_type() -> FlowType {
        FlowType {
            mode: FlowMode::Continuous,
            ty: Type::Unknown,
        }
    }

    fn signature(
        decl_id: DeclId,
        kind: CheckedCallableKind,
        name: &str,
        output: DeclId,
    ) -> CheckedCallableSignature {
        CheckedCallableSignature {
            decl_id,
            scope_id: LexicalScopeId(decl_id.0),
            kind,
            name: name.to_owned(),
            parameters: vec![CheckedParameter {
                decl_id: output,
                name: "item".to_owned(),
                kind: CheckedParameterKind::Out,
                ordinal: 0,
                flow_type: unknown_flow_type(),
                evaluation_scope: CheckedEvaluationScope::Parent,
                start: 0,
                end: 0,
            }],
            result: unknown_flow_type(),
            role: ProgramRole::Client,
            effect: CheckedEffectSummary::default(),
            body: None,
            result_expression: None,
            contextual_operation: None,
        }
    }

    fn fresh_call(
        expr_id: usize,
        callable: DeclId,
        owner_callable: Option<DeclId>,
        formal: DeclId,
        output: DeclId,
    ) -> CheckedCall {
        CheckedCall {
            id: CheckedCallId(expr_id as u32),
            expression: CheckedExprId(expr_id as u32),
            callable,
            owner_callable,
            function: format!("call_{}", callable.0),
            entries: vec![CheckedCallEntry::FreshOut {
                formal,
                name: "item".to_owned(),
                output,
                scope_id: LexicalScopeId(output.0),
            }],
            pass: None,
            result: unknown_flow_type(),
            role: ProgramRole::Client,
            span: CheckedSpan::default(),
        }
    }

    fn forward_call(
        expr_id: usize,
        callable: DeclId,
        owner_callable: Option<DeclId>,
        formal: DeclId,
        target: DeclId,
    ) -> CheckedCall {
        CheckedCall {
            id: CheckedCallId(expr_id as u32),
            expression: CheckedExprId(expr_id as u32),
            callable,
            owner_callable,
            function: format!("call_{}", callable.0),
            entries: vec![CheckedCallEntry::ForwardOut {
                formal,
                name: "item".to_owned(),
                target,
                target_name: "item".to_owned(),
            }],
            pass: None,
            result: unknown_flow_type(),
            role: ProgramRole::Client,
            span: CheckedSpan::default(),
        }
    }

    fn wrapper_program(calls: Vec<CheckedCall>) -> CheckedProgram {
        CheckedProgram {
            root_scope: LexicalScopeId(0),
            callables: vec![
                signature(WRAPPER, CheckedCallableKind::User, "wrapper", WRAPPER_OUT),
                signature(
                    BUILTIN,
                    CheckedCallableKind::Builtin,
                    "producer",
                    BUILTIN_OUT,
                ),
            ],
            calls,
            ..CheckedProgram::default()
        }
    }

    fn direct_program(calls: Vec<CheckedCall>) -> CheckedProgram {
        CheckedProgram {
            root_scope: LexicalScopeId(0),
            callables: vec![signature(
                BUILTIN,
                CheckedCallableKind::Builtin,
                "producer",
                BUILTIN_OUT,
            )],
            calls,
            ..CheckedProgram::default()
        }
    }

    fn expression(id: usize, scope_id: LexicalScopeId, start: usize) -> CheckedExpression {
        CheckedExpression {
            id: CheckedExprId(id as u32),
            scope_id,
            declaration: None,
            flow_type: unknown_flow_type(),
            effect: CheckedEffectSummary::default(),
            kind: CheckedExpressionKind::Bool { value: true },
            span: CheckedSpan {
                line: 0,
                start,
                end: start + 1,
            },
        }
    }

    fn scope(
        id: u32,
        parent: Option<u32>,
        owner: Option<DeclId>,
        kind: CheckedScopeKind,
        start: usize,
    ) -> CheckedScope {
        CheckedScope {
            id: LexicalScopeId(id),
            parent: parent.map(LexicalScopeId),
            owner,
            kind,
            span: CheckedSpan {
                line: 0,
                start,
                end: start + 1,
            },
        }
    }

    fn producer_owners_for_output(
        graph: &OutNet,
        call: CheckedCallId,
        output: DeclId,
    ) -> Vec<StaticOwnerId> {
        graph
            .concrete_producers_for_checked_call(call)
            .into_iter()
            .filter(
                |producer| match graph.ports[producer.port.as_usize()].binding {
                    OutPortBinding::Fresh {
                        output: candidate, ..
                    } => candidate == output,
                    OutPortBinding::Forward { target } => target == output,
                },
            )
            .map(|producer| producer.owner)
            .collect()
    }

    #[test]
    fn separate_wrapper_calls_get_separate_static_owners() {
        let calls = vec![
            forward_call(100, BUILTIN, Some(WRAPPER), BUILTIN_OUT, WRAPPER_OUT),
            fresh_call(201, WRAPPER, None, WRAPPER_OUT, DeclId(21)),
            fresh_call(200, WRAPPER, None, WRAPPER_OUT, DeclId(20)),
        ];
        let first = OutNet::build(&wrapper_program(calls.clone()));
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

        assert_eq!(
            producer_owners_for_output(&first.graph, CheckedCallId(100), WRAPPER_OUT),
            vec![StaticOwnerId(0), StaticOwnerId(1)]
        );

        let direct = OutNet::build(&direct_program(vec![
            fresh_call(200, BUILTIN, None, BUILTIN_OUT, DeclId(20)),
            fresh_call(201, BUILTIN, None, BUILTIN_OUT, DeclId(21)),
        ]));
        assert!(!direct.has_errors(), "{:#?}", direct.diagnostics);
        assert_eq!(first.graph.static_owners, direct.graph.static_owners);

        let mut reordered = calls;
        reordered.reverse();
        let second = OutNet::build(&wrapper_program(reordered));
        assert_eq!(first, second);
    }

    #[test]
    fn multiple_wrapper_layers_erase_to_the_direct_owner_forest() {
        const OUTER: DeclId = DeclId(3);
        const OUTER_OUT: DeclId = DeclId(4);

        let wrapped = OutNet::build(&CheckedProgram {
            root_scope: LexicalScopeId(0),
            callables: vec![
                signature(OUTER, CheckedCallableKind::User, "outer", OUTER_OUT),
                signature(WRAPPER, CheckedCallableKind::User, "wrapper", WRAPPER_OUT),
                signature(
                    BUILTIN,
                    CheckedCallableKind::Builtin,
                    "producer",
                    BUILTIN_OUT,
                ),
            ],
            calls: vec![
                fresh_call(300, OUTER, None, OUTER_OUT, DeclId(20)),
                forward_call(200, WRAPPER, Some(OUTER), WRAPPER_OUT, OUTER_OUT),
                forward_call(100, BUILTIN, Some(WRAPPER), BUILTIN_OUT, WRAPPER_OUT),
            ],
            ..CheckedProgram::default()
        });
        assert!(!wrapped.has_errors(), "{:#?}", wrapped.diagnostics);

        let direct = OutNet::build(&direct_program(vec![fresh_call(
            300,
            BUILTIN,
            None,
            BUILTIN_OUT,
            DeclId(20),
        )]));
        assert!(!direct.has_errors(), "{:#?}", direct.diagnostics);
        assert_eq!(wrapped.graph.static_owners, direct.graph.static_owners);
        let producers = wrapped
            .graph
            .concrete_producers_for_checked_call(CheckedCallId(100));
        let [producer] = producers.as_slice() else {
            panic!("multi-wrapper expansion must expose one concrete producer");
        };
        assert_eq!(producer.owner, StaticOwnerId(0));
        assert_eq!(wrapped.graph.net_for_port(producer.port), producer.net);
        assert_eq!(
            wrapped.graph.output_net_in_frame(None, DeclId(20)),
            Some(producer.net)
        );
        assert_eq!(
            wrapped
                .graph
                .output_net_in_frame(Some(producer.call), BUILTIN_OUT),
            Some(producer.net)
        );
        assert_eq!(
            producer_owners_for_output(&wrapped.graph, CheckedCallId(100), WRAPPER_OUT),
            vec![StaticOwnerId(0)]
        );
    }

    #[test]
    fn repeated_output_scopes_form_real_owner_ancestry() {
        let outer_output = DeclId(20);
        let inner_output = DeclId(21);
        let root = LexicalScopeId(0);
        let repeated = LexicalScopeId(20);
        let build = OutNet::build(&CheckedProgram {
            root_scope: root,
            scopes: vec![
                scope(0, None, None, CheckedScopeKind::Root, 0),
                scope(
                    repeated.0,
                    Some(root.0),
                    Some(outer_output),
                    CheckedScopeKind::RepeatedOutput,
                    20,
                ),
            ],
            expressions: vec![expression(1, root, 1), expression(2, repeated, 21)],
            callables: direct_program(Vec::new()).callables,
            calls: vec![
                fresh_call(1, BUILTIN, None, BUILTIN_OUT, outer_output),
                fresh_call(2, BUILTIN, None, BUILTIN_OUT, inner_output),
            ],
            ..CheckedProgram::default()
        });
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
        assert_eq!(
            producer_owners_for_output(&build.graph, CheckedCallId(2), inner_output),
            vec![StaticOwnerId(1)]
        );
    }

    #[test]
    fn output_entry_order_does_not_change_static_owner_identity() {
        let first_formal = DeclId(11);
        let second_formal = DeclId(12);
        let first_output = DeclId(31);
        let second_output = DeclId(32);
        let callable = CheckedCallableSignature {
            decl_id: BUILTIN,
            scope_id: LexicalScopeId(BUILTIN.0),
            kind: CheckedCallableKind::Builtin,
            name: "two_outputs".to_owned(),
            parameters: vec![
                CheckedParameter {
                    decl_id: first_formal,
                    name: "first".to_owned(),
                    kind: CheckedParameterKind::Out,
                    ordinal: 0,
                    flow_type: unknown_flow_type(),
                    evaluation_scope: CheckedEvaluationScope::Parent,
                    start: 0,
                    end: 0,
                },
                CheckedParameter {
                    decl_id: second_formal,
                    name: "second".to_owned(),
                    kind: CheckedParameterKind::Out,
                    ordinal: 1,
                    flow_type: unknown_flow_type(),
                    evaluation_scope: CheckedEvaluationScope::Parent,
                    start: 0,
                    end: 0,
                },
            ],
            result: unknown_flow_type(),
            role: ProgramRole::Client,
            effect: CheckedEffectSummary::default(),
            body: None,
            result_expression: None,
            contextual_operation: None,
        };
        let entry = |formal, name: &str, output, scope_id| CheckedCallEntry::FreshOut {
            formal,
            name: name.to_owned(),
            output,
            scope_id,
        };
        let program = |entries| CheckedProgram {
            root_scope: LexicalScopeId(0),
            scopes: vec![
                scope(0, None, None, CheckedScopeKind::Root, 0),
                scope(
                    31,
                    Some(0),
                    Some(first_output),
                    CheckedScopeKind::RepeatedOutput,
                    31,
                ),
                scope(
                    32,
                    Some(0),
                    Some(second_output),
                    CheckedScopeKind::RepeatedOutput,
                    32,
                ),
            ],
            expressions: vec![expression(1, LexicalScopeId(0), 1)],
            callables: vec![callable.clone()],
            calls: vec![CheckedCall {
                id: CheckedCallId(1),
                expression: CheckedExprId(1),
                callable: BUILTIN,
                owner_callable: None,
                function: "two_outputs".to_owned(),
                entries,
                pass: None,
                result: unknown_flow_type(),
                role: ProgramRole::Client,
                span: CheckedSpan::default(),
            }],
            ..CheckedProgram::default()
        };
        let first = OutNet::build(&program(vec![
            entry(first_formal, "first", first_output, LexicalScopeId(31)),
            entry(second_formal, "second", second_output, LexicalScopeId(32)),
        ]));
        let reversed = OutNet::build(&program(vec![
            entry(second_formal, "second", second_output, LexicalScopeId(32)),
            entry(first_formal, "first", first_output, LexicalScopeId(31)),
        ]));
        assert!(!first.has_errors(), "{:#?}", first.diagnostics);
        assert!(!reversed.has_errors(), "{:#?}", reversed.diagnostics);
        assert_eq!(first.graph.static_owners, reversed.graph.static_owners);
        assert_eq!(
            producer_owners_for_output(&first.graph, CheckedCallId(1), first_output),
            producer_owners_for_output(&reversed.graph, CheckedCallId(1), first_output)
        );
        assert_eq!(
            producer_owners_for_output(&first.graph, CheckedCallId(1), second_output),
            producer_owners_for_output(&reversed.graph, CheckedCallId(1), second_output)
        );
    }

    #[test]
    fn reports_zero_and_multiple_structural_producers() {
        let missing = OutNet::build(&wrapper_program(vec![fresh_call(
            1,
            WRAPPER,
            None,
            WRAPPER_OUT,
            DeclId(20),
        )]));
        assert!(
            missing
                .diagnostics
                .iter()
                .any(|diagnostic| matches!(diagnostic, OutNetDiagnostic::MissingProducer { .. }))
        );

        let multiple = OutNet::build(&wrapper_program(vec![
            fresh_call(1, WRAPPER, None, WRAPPER_OUT, DeclId(20)),
            forward_call(2, BUILTIN, Some(WRAPPER), BUILTIN_OUT, WRAPPER_OUT),
            forward_call(3, BUILTIN, Some(WRAPPER), BUILTIN_OUT, WRAPPER_OUT),
        ]));
        assert!(multiple.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            OutNetDiagnostic::MultipleProducers { producers, .. } if producers.len() == 2
        )));

        let missing_anchor = OutNet::build(&direct_program(vec![forward_call(
            4,
            BUILTIN,
            None,
            BUILTIN_OUT,
            DeclId(99),
        )]));
        assert!(
            missing_anchor.diagnostics.iter().any(|diagnostic| matches!(
                diagnostic,
                OutNetDiagnostic::MissingOwnerAnchor { .. }
            ))
        );

        let multiple_anchors = OutNet::build(&direct_program(vec![
            fresh_call(5, BUILTIN, None, BUILTIN_OUT, DeclId(100)),
            fresh_call(6, BUILTIN, None, BUILTIN_OUT, DeclId(100)),
        ]));
        assert!(
            multiple_anchors
                .diagnostics
                .iter()
                .any(|diagnostic| matches!(
                    diagnostic,
                    OutNetDiagnostic::MultipleOwnerAnchors { anchors, .. } if anchors.len() == 2
                ))
        );
    }

    #[test]
    fn reports_forwarding_alias_cycles() {
        let first = DeclId(30);
        let first_out = DeclId(31);
        let second = DeclId(40);
        let second_out = DeclId(41);
        let program = CheckedProgram {
            root_scope: LexicalScopeId(0),
            callables: vec![
                signature(first, CheckedCallableKind::User, "first", first_out),
                signature(second, CheckedCallableKind::User, "second", second_out),
            ],
            calls: vec![
                fresh_call(1, first, None, first_out, DeclId(50)),
                forward_call(2, second, Some(first), second_out, first_out),
                forward_call(3, first, Some(second), first_out, second_out),
            ],
            ..CheckedProgram::default()
        };

        let build = OutNet::build(&program);
        assert!(build.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            OutNetDiagnostic::AliasCycle { declarations, .. }
                if declarations == &vec![first_out, second_out]
        )));
    }
}
