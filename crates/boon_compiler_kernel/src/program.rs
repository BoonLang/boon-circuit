use crate::{NameId, TypeTerm, TypeTermArena, TypeTermId, TypeVariableId};
use boon_checked::FlowMode;
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OperationId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OutputId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishMode {
    /// A true type equation. It may bind holes on either side.
    Unify,
    /// Directional branch/value flow. Distinct shapes form a canonical union.
    Union,
    /// Directional collection flow. Like-shaped records widen field-by-field.
    StructuralWiden,
    /// Exact provider epoch. Previous output scaffolds are not requirements.
    Replace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelCollectionOperationKind {
    List,
    Set,
    Map,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelCollectionProjectionKind {
    Item,
    MapKey,
    MapValue,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelPattern {
    Wildcard,
    Number,
    Text,
    Bits {
        width: u32,
    },
    Tag {
        name: Box<str>,
        fields: Box<[Box<str>]>,
    },
    Binding {
        name: Box<str>,
    },
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelSelectArm {
    pub pattern: KernelPattern,
    pub output: TypeTermId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelRecordEntry {
    Field { name: NameId, value: TypeTermId },
    Spread { value: TypeTermId },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KernelSummaryValueId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelSummaryRecordEntry {
    Field {
        name: NameId,
        value: KernelSummaryValueId,
    },
    Spread {
        value: KernelSummaryValueId,
    },
}

/// Immutable result-construction bytecode shared by every compatible call.
///
/// Input slots are the only occurrence-local values. Constants and record
/// structure are interned once in the owning component term arena, so a call
/// publishes its result without allocating a complete callee expression
/// frame or recursively interpreting source nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelSummaryProgram {
    /// Dense definition identity used only for work attribution. It is not a
    /// cross-revision or serialized semantic identity.
    pub definition: u32,
    pub nodes: Box<[KernelSummaryNode]>,
    pub result: KernelSummaryValueId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelSummaryNode {
    Input(u32),
    Term(TypeTermId),
    /// One occurrence-local contextual hole (`[]`). The evaluator allocates a
    /// fresh union-find variable for every summary invocation so an enclosing
    /// constraint can choose record, list, set, map, or bytes shape without
    /// sharing that choice across calls.
    ContextualHole,
    /// Directional projection of a value constructed inside the immutable
    /// summary. Formal-root projections stay occurrence-local call inputs so
    /// they can retain requirement backflow; computed values need no mutable
    /// provider cell and are projected directly during summary evaluation.
    Projection {
        provider: KernelSummaryValueId,
        fields: Box<[NameId]>,
    },
    Constrain {
        value: KernelSummaryValueId,
        expected: TypeTermId,
    },
    Sequence {
        inputs: Box<[KernelSummaryValueId]>,
        result: KernelSummaryValueId,
    },
    Collection {
        kind: KernelCollectionOperationKind,
        inputs: Box<[KernelSummaryValueId]>,
        values: Box<[KernelSummaryValueId]>,
    },
    /// One call into immutable bytecode owned by another definition. Callee
    /// input slots resolve their mapped caller values lazily, while the callee
    /// receives its own generation-stamped scratch frame. This preserves
    /// unselected-branch laziness and keeps each definition's result program
    /// unique instead of recursively embedding the callee graph into every
    /// caller summary.
    Invoke {
        program: Arc<KernelSummaryProgram>,
        inputs: Box<[KernelSummaryValueId]>,
    },
    Select {
        selector: KernelSummaryValueId,
        /// Only authored WHEN selection creates checked-call syntax provenance.
        /// Internal ABI/render specialization may use the same compact branch
        /// evaluator without relabelling the enclosing call.
        syntax_discriminating: bool,
        arms: Box<[KernelSummarySelectArm]>,
    },
    Record {
        tag: Option<NameId>,
        entries: Box<[KernelSummaryRecordEntry]>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelSummarySelectArm {
    pub pattern: KernelPattern,
    pub output: KernelSummaryValueId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelSummaryProjectionStep {
    pub field: Option<NameId>,
    pub consumer: TypeVariableId,
}

/// One occurrence-local operand for immutable definition-summary bytecode.
///
/// A projection stores preallocated private cells but no standalone graph
/// operations. The summary evaluates its steps only when the corresponding
/// input node is demanded, which preserves static branch laziness while the
/// invocation subscribes directly to the provider root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelSummaryCallInput {
    Term(TypeTermId),
    Projection {
        provider: TypeVariableId,
        steps: Box<[KernelSummaryProjectionStep]>,
        /// Ordinary argument reads start parameter-derived in the called
        /// definition. Context reads deliberately preserve the caller value's
        /// provenance instead.
        parameter_derived: bool,
    },
}

impl From<TypeTermId> for KernelSummaryCallInput {
    fn from(term: TypeTermId) -> Self {
        Self::Term(term)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelOperation {
    Unify {
        left: TypeTermId,
        right: TypeTermId,
    },
    /// One ordered whole-value lexical equation. Evaluation preserves true
    /// equality/backflow, while the packed provider/consumer roles make its
    /// initial dataflow direction explicit to the scheduler.
    Alias {
        provider: TypeVariableId,
        consumer: TypeVariableId,
    },
    Publish {
        output: TypeVariableId,
        inputs: Box<[TypeTermId]>,
        mode: PublishMode,
    },
    /// One explicit projection equation. Chained field reads compile into one
    /// operation per segment, so every intermediate occurrence is refreshed.
    Projection {
        provider: TypeVariableId,
        field: Option<NameId>,
        consumer: TypeVariableId,
    },
    /// Directional projection through one authored match pattern. This is a
    /// single packed equation because ordinary field projections cannot
    /// retain which tagged-variant arm introduced a payload binding.
    PatternProjection {
        provider: TypeVariableId,
        pattern: KernelPattern,
        fields: Box<[NameId]>,
        consumer: TypeVariableId,
    },
    /// Directional extraction of one collection component authority. This is
    /// the compact residual behind contextual callback bindings and Map ABI
    /// correlations; it never equates the producer with the occurrence.
    CollectionProjection {
        provider: TypeVariableId,
        kind: KernelCollectionProjectionKind,
        consumer: TypeVariableId,
    },
    /// One collection authority. Item/key/value inputs are widened directly
    /// into the final collection term, avoiding mutable intermediate cells.
    Collection {
        output: TypeVariableId,
        kind: KernelCollectionOperationKind,
        inputs: Box<[TypeTermId]>,
        values: Box<[TypeTermId]>,
    },
    /// One syntax-discriminated branch join. A singleton tag selects the
    /// first matching non-absent arm. Multiple closed outputs structurally
    /// widen like the language's WHEN/LATEST rules; unresolved generic arms
    /// remain a symbolic union until their invocation frame settles.
    Select {
        output: TypeVariableId,
        selector: TypeVariableId,
        /// The selector is derived from an invocation formal. A singleton
        /// selector therefore proves that this occurrence chose one authored
        /// syntax branch rather than merely joining a definition's principal
        /// result surface.
        selector_parameter_derived: bool,
        arms: Box<[KernelSelectArm]>,
    },
    /// One ordered record assembly. Spread fields and explicit fields replace
    /// earlier values without changing the first-authored field position.
    Record {
        output: TypeVariableId,
        tag: Option<NameId>,
        entries: Box<[KernelRecordEntry]>,
    },
    /// One invocation of immutable definition-result bytecode. Only its
    /// projected formal/external inputs and result are occurrence-local.
    SummaryCall {
        output: TypeVariableId,
        program: Arc<KernelSummaryProgram>,
        inputs: Box<[KernelSummaryCallInput]>,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VariableSpec {
    pub contextual_hole: bool,
    pub authoritative_provider: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramOutput {
    pub id: OutputId,
    pub variable: TypeVariableId,
    pub mode: FlowMode,
}

#[derive(Debug)]
pub struct ComponentProgram {
    pub(crate) terms: TypeTermArena,
    pub(crate) variables: Box<[VariableSpec]>,
    pub(crate) operations: Box<[Arc<KernelOperation>]>,
    pub(crate) residual_frames: Box<[ResidualOperationFrame]>,
    pub(crate) work_items: Box<[ProgramOperationRef]>,
    pub(crate) instruction_count: u64,
    pub(crate) initial_order: Box<[OperationId]>,
    pub(crate) acyclic_initial_operations: u64,
    pub(crate) dependency_offsets: Box<[u32]>,
    pub(crate) dependencies: Box<[TypeVariableId]>,
    pub(crate) consumer_offsets: Box<[u32]>,
    pub(crate) consumers: Box<[ProgramConsumer]>,
    pub(crate) outputs: Box<[ProgramOutput]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProgramOperationRef {
    Direct(u32),
    /// One fully acyclic residual module evaluated as a coarse scheduled unit.
    /// Its immutable instruction payload remains in the shared module.
    ResidualFrame {
        frame: u32,
    },
    Residual {
        frame: u32,
        operation: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ProgramConsumer {
    pub operation: OperationId,
}

#[derive(Clone, Debug)]
pub(crate) struct ResidualOperationFrame {
    pub module: Arc<ComponentProgram>,
    pub variables: Arc<[TypeVariableId]>,
    /// Dense linker tables into the owning component arena. Building these
    /// once keeps the hot solver loop to indexed reads while the immutable
    /// operation payload remains shared by every invocation frame.
    pub terms: Arc<[Option<TypeTermId>]>,
    pub names: Arc<[Option<NameId>]>,
}

#[derive(Clone, Copy, Debug)]
enum BuilderWorkItem {
    Direct(u32),
    Residual(u32),
}

impl ComponentProgram {
    pub fn terms(&self) -> &TypeTermArena {
        &self.terms
    }

    pub fn variable_count(&self) -> usize {
        self.variables.len()
    }

    pub fn operation_count(&self) -> usize {
        usize::try_from(self.instruction_count).unwrap_or(usize::MAX)
    }

    pub fn scheduled_work_item_count(&self) -> usize {
        self.work_items.len()
    }

    pub fn acyclic_initial_operation_count(&self) -> u64 {
        self.acyclic_initial_operations
    }

    pub fn outputs(&self) -> &[ProgramOutput] {
        &self.outputs
    }

    pub(crate) fn variable_specs(&self) -> &[VariableSpec] {
        &self.variables
    }

    #[cfg(test)]
    pub(crate) fn consumers(&self, variable: TypeVariableId) -> &[ProgramConsumer] {
        let index = variable.0 as usize;
        let start = self.consumer_offsets[index] as usize;
        let end = self.consumer_offsets[index + 1] as usize;
        &self.consumers[start..end]
    }

    pub(crate) fn operation_dependencies(&self, operation: OperationId) -> &[TypeVariableId] {
        let index = operation.0 as usize;
        let start = self.dependency_offsets[index] as usize;
        let end = self.dependency_offsets[index + 1] as usize;
        &self.dependencies[start..end]
    }
}

#[derive(Debug, Default)]
pub struct ComponentProgramBuilder {
    terms: TypeTermArena,
    variables: Vec<VariableSpec>,
    operations: Vec<KernelOperation>,
    residual_frames: Vec<ResidualOperationFrame>,
    work_order: Vec<BuilderWorkItem>,
    outputs: Vec<ProgramOutput>,
}

impl ComponentProgramBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn terms(&self) -> &TypeTermArena {
        &self.terms
    }

    pub fn terms_mut(&mut self) -> &mut TypeTermArena {
        &mut self.terms
    }

    pub fn new_variable(&mut self) -> TypeVariableId {
        self.new_variable_with(VariableSpec::default())
    }

    pub fn new_contextual_hole(&mut self) -> TypeVariableId {
        self.new_variable_with(VariableSpec {
            contextual_hole: true,
            authoritative_provider: false,
        })
    }

    pub fn new_authoritative_provider(&mut self) -> TypeVariableId {
        self.new_variable_with(VariableSpec {
            contextual_hole: false,
            authoritative_provider: true,
        })
    }

    pub fn new_variable_with(&mut self, spec: VariableSpec) -> TypeVariableId {
        let variable = TypeVariableId(
            u32::try_from(self.variables.len()).expect("kernel variable count exceeds u32"),
        );
        self.variables.push(spec);
        variable
    }

    pub fn variable_term(&mut self, variable: TypeVariableId) -> TypeTermId {
        assert!(
            (variable.0 as usize) < self.variables.len(),
            "kernel variable belongs to this program"
        );
        self.terms.variable(variable)
    }

    pub fn mark_authoritative(&mut self, variable: TypeVariableId) {
        self.variables[variable.0 as usize].authoritative_provider = true;
    }

    pub fn is_authoritative(&self, variable: TypeVariableId) -> bool {
        self.variables
            .get(variable.0 as usize)
            .is_some_and(|spec| spec.authoritative_provider)
    }

    pub fn add_unify(&mut self, left: TypeTermId, right: TypeTermId) -> OperationId {
        self.push_operation(KernelOperation::Unify { left, right })
    }

    pub fn add_alias(&mut self, provider: TypeVariableId, consumer: TypeVariableId) -> OperationId {
        self.push_operation(KernelOperation::Alias { provider, consumer })
    }

    pub fn add_publish(
        &mut self,
        output: TypeVariableId,
        inputs: impl IntoIterator<Item = TypeTermId>,
        mode: PublishMode,
    ) -> OperationId {
        // Every publication except equality-unification owns a replaceable
        // derived root. Projections must observe that root directionally and
        // replay after later epochs; otherwise an early consumer scaffold can
        // flow backward into a cyclic HOLD/LATEST/union provider.
        if mode != PublishMode::Unify {
            self.mark_authoritative(output);
        }
        self.push_operation(KernelOperation::Publish {
            output,
            inputs: inputs.into_iter().collect::<Vec<_>>().into_boxed_slice(),
            mode,
        })
    }

    /// Compile a complete occurrence projection.
    ///
    /// Even an empty path receives a detached consumer. This lets a provider
    /// become authoritative after the read was created without coalescing the
    /// consumer requirement into the provider root.
    pub fn add_projection(
        &mut self,
        provider: TypeVariableId,
        path: impl IntoIterator<Item = NameId>,
    ) -> TypeVariableId {
        let consumer = self.new_variable();
        self.add_projection_into(provider, path, consumer);
        consumer
    }

    /// Compile a projection directly into an already allocated occurrence.
    /// This is the compact form used by definition residuals; it avoids a
    /// temporary projection result followed by a copy/equality operation.
    pub fn add_projection_into(
        &mut self,
        provider: TypeVariableId,
        path: impl IntoIterator<Item = NameId>,
        consumer: TypeVariableId,
    ) {
        let path = path.into_iter().collect::<Vec<_>>();
        if path.is_empty() {
            self.push_operation(KernelOperation::Projection {
                provider,
                field: None,
                consumer,
            });
            return;
        }
        let mut provider = provider;
        let last = path.len() - 1;
        for (index, field) in path.into_iter().enumerate() {
            let next = if index == last {
                consumer
            } else {
                self.new_variable()
            };
            self.push_operation(KernelOperation::Projection {
                provider,
                field: Some(field),
                consumer: next,
            });
            provider = next;
        }
    }

    pub fn add_pattern_projection_into(
        &mut self,
        provider: TypeVariableId,
        pattern: KernelPattern,
        fields: impl IntoIterator<Item = NameId>,
        consumer: TypeVariableId,
    ) -> OperationId {
        self.push_operation(KernelOperation::PatternProjection {
            provider,
            pattern,
            fields: fields.into_iter().collect::<Vec<_>>().into_boxed_slice(),
            consumer,
        })
    }

    pub fn add_select(
        &mut self,
        output: TypeVariableId,
        selector: TypeVariableId,
        arms: impl IntoIterator<Item = KernelSelectArm>,
    ) -> OperationId {
        self.add_select_with_parameter_provenance(output, selector, false, arms)
    }

    pub fn add_select_with_parameter_provenance(
        &mut self,
        output: TypeVariableId,
        selector: TypeVariableId,
        selector_parameter_derived: bool,
        arms: impl IntoIterator<Item = KernelSelectArm>,
    ) -> OperationId {
        self.mark_authoritative(output);
        self.push_operation(KernelOperation::Select {
            output,
            selector,
            selector_parameter_derived,
            arms: arms.into_iter().collect::<Vec<_>>().into_boxed_slice(),
        })
    }

    pub fn add_collection_item_projection(
        &mut self,
        provider: TypeVariableId,
        consumer: TypeVariableId,
    ) -> OperationId {
        self.mark_authoritative(consumer);
        self.push_operation(KernelOperation::CollectionProjection {
            provider,
            kind: KernelCollectionProjectionKind::Item,
            consumer,
        })
    }

    pub fn add_map_key_projection(
        &mut self,
        provider: TypeVariableId,
        consumer: TypeVariableId,
    ) -> OperationId {
        self.mark_authoritative(consumer);
        self.push_operation(KernelOperation::CollectionProjection {
            provider,
            kind: KernelCollectionProjectionKind::MapKey,
            consumer,
        })
    }

    pub fn add_map_value_projection(
        &mut self,
        provider: TypeVariableId,
        consumer: TypeVariableId,
    ) -> OperationId {
        self.mark_authoritative(consumer);
        self.push_operation(KernelOperation::CollectionProjection {
            provider,
            kind: KernelCollectionProjectionKind::MapValue,
            consumer,
        })
    }

    pub fn add_collection(
        &mut self,
        output: TypeVariableId,
        kind: KernelCollectionOperationKind,
        inputs: impl IntoIterator<Item = TypeTermId>,
        values: impl IntoIterator<Item = TypeTermId>,
    ) -> OperationId {
        self.mark_authoritative(output);
        self.push_operation(KernelOperation::Collection {
            output,
            kind,
            inputs: inputs.into_iter().collect::<Vec<_>>().into_boxed_slice(),
            values: values.into_iter().collect::<Vec<_>>().into_boxed_slice(),
        })
    }

    pub fn add_record(
        &mut self,
        output: TypeVariableId,
        tag: Option<NameId>,
        entries: impl IntoIterator<Item = KernelRecordEntry>,
    ) -> OperationId {
        self.mark_authoritative(output);
        self.push_operation(KernelOperation::Record {
            output,
            tag,
            entries: entries.into_iter().collect::<Vec<_>>().into_boxed_slice(),
        })
    }

    pub fn add_summary_call<I, T>(
        &mut self,
        output: TypeVariableId,
        program: Arc<KernelSummaryProgram>,
        inputs: I,
    ) -> OperationId
    where
        I: IntoIterator<Item = T>,
        T: Into<KernelSummaryCallInput>,
    {
        self.mark_authoritative(output);
        self.push_operation(KernelOperation::SummaryCall {
            output,
            program,
            inputs: inputs
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })
    }

    pub fn add_output(&mut self, variable: TypeVariableId, mode: FlowMode) -> OutputId {
        let id =
            OutputId(u32::try_from(self.outputs.len()).expect("kernel output count exceeds u32"));
        self.outputs.push(ProgramOutput { id, variable, mode });
        id
    }

    /// Link one compiled residual module through a compact variable frame.
    /// The operation and term payloads remain owned once by the module.
    pub(crate) fn add_residual_frame(
        &mut self,
        module: Arc<ComponentProgram>,
        variables: Vec<TypeVariableId>,
    ) -> u32 {
        assert_eq!(
            variables.len(),
            module.variables.len(),
            "residual module frame must map every variable"
        );
        assert!(
            module.residual_frames.is_empty(),
            "residual modules cannot contain nested physical frames"
        );
        let frame = u32::try_from(self.residual_frames.len())
            .expect("kernel residual frame count exceeds u32");
        self.residual_frames.push(ResidualOperationFrame {
            module,
            variables: variables.into(),
            terms: Arc::from([]),
            names: Arc::from([]),
        });
        self.work_order.push(BuilderWorkItem::Residual(frame));
        frame
    }

    pub fn finish(mut self) -> ComponentProgram {
        // Link module-local immutable terms and names once. The previous lazy
        // solver-side translation performed the same work in the activation
        // loop and made shared modules slower than their flattened precursor.
        let target_terms = &mut self.terms;
        for frame in &mut self.residual_frames {
            let mut term_cache = vec![None; frame.module.terms.len()];
            let mut name_cache = vec![None; frame.module.terms.name_count()];
            for operation in frame.module.operations.iter() {
                link_residual_operation_terms(
                    operation,
                    &frame.module.terms,
                    target_terms,
                    &frame.variables,
                    &mut term_cache,
                    &mut name_cache,
                );
            }
            frame.terms = term_cache.into();
            frame.names = name_cache.into();
        }

        let mut reverse = vec![BTreeSet::<ProgramConsumer>::new(); self.variables.len()];
        let mut work_items = Vec::new();
        let mut dependency_offsets = Vec::new();
        let mut forward_dependencies = Vec::new();
        let mut operation_outputs = Vec::<Box<[TypeVariableId]>>::new();
        let mut instruction_count = 0_u64;
        dependency_offsets.push(0);
        for item in &self.work_order {
            match *item {
                BuilderWorkItem::Direct(index) => {
                    instruction_count = instruction_count.saturating_add(1);
                    let operation_id = OperationId(
                        u32::try_from(work_items.len())
                            .expect("kernel operation count exceeds u32"),
                    );
                    work_items.push(ProgramOperationRef::Direct(index));
                    operation_outputs.push(
                        operation_output(&self.operations[index as usize], None)
                            .into_iter()
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    let mut operation_dependencies = BTreeSet::new();
                    collect_operation_variables(
                        &self.operations[index as usize],
                        &self.terms,
                        &mut operation_dependencies,
                    );
                    for dependency in operation_dependencies {
                        assert!(
                            (dependency.0 as usize) < reverse.len(),
                            "kernel operation references an undeclared variable"
                        );
                        reverse[dependency.0 as usize].insert(ProgramConsumer {
                            operation: operation_id,
                        });
                        forward_dependencies.push(dependency);
                    }
                    dependency_offsets.push(
                        u32::try_from(forward_dependencies.len())
                            .expect("kernel dependency edge count exceeds u32"),
                    );
                }
                BuilderWorkItem::Residual(frame_index) => {
                    let frame = &self.residual_frames[frame_index as usize];
                    instruction_count =
                        instruction_count.saturating_add(frame.module.operation_count() as u64);
                    let fully_acyclic = frame.module.acyclic_initial_operation_count()
                        == frame.module.operation_count() as u64;
                    if fully_acyclic {
                        let operation_id = OperationId(
                            u32::try_from(work_items.len())
                                .expect("kernel operation count exceeds u32"),
                        );
                        work_items.push(ProgramOperationRef::ResidualFrame { frame: frame_index });
                        let mut outputs = BTreeSet::new();
                        let mut dependencies = BTreeSet::new();
                        for operation_index in 0..frame.module.operations.len() {
                            if let Some(output) = operation_output(
                                &frame.module.operations[operation_index],
                                Some(&frame.variables),
                            ) {
                                outputs.insert(output);
                            }
                        }
                        for operation_index in 0..frame.module.operations.len() {
                            let module_operation = OperationId(
                                u32::try_from(operation_index)
                                    .expect("kernel residual operation count exceeds u32"),
                            );
                            let operation_dependencies = frame
                                .module
                                .operation_dependencies(module_operation)
                                .iter()
                                .map(|dependency| frame.variables[dependency.0 as usize])
                                .collect::<BTreeSet<_>>();
                            dependencies.extend(operation_dependencies.iter().copied());
                        }
                        // The module's authored topological order owns every
                        // value it writes internally. Only formal/imported
                        // roots are frame-level subscriptions; retaining
                        // internal intermediates here reactivated the whole
                        // frame after its own instructions had already
                        // consumed them.
                        dependencies.retain(|dependency| !outputs.contains(dependency));
                        operation_outputs.push(outputs.into_iter().collect());
                        for dependency in dependencies {
                            assert!(
                                (dependency.0 as usize) < reverse.len(),
                                "kernel residual operation references an undeclared frame variable"
                            );
                            forward_dependencies.push(dependency);
                            reverse[dependency.0 as usize].insert(ProgramConsumer {
                                operation: operation_id,
                            });
                        }
                        dependency_offsets.push(
                            u32::try_from(forward_dependencies.len())
                                .expect("kernel dependency edge count exceeds u32"),
                        );
                    } else {
                        // Cyclic residual modules retain instruction-grained
                        // scheduling. They are the small exceptional tail;
                        // acyclic definitions use one compact frame work item.
                        for operation_index in 0..frame.module.operations.len() {
                            let operation_id = OperationId(
                                u32::try_from(work_items.len())
                                    .expect("kernel operation count exceeds u32"),
                            );
                            work_items.push(ProgramOperationRef::Residual {
                                frame: frame_index,
                                operation: u32::try_from(operation_index)
                                    .expect("kernel residual operation count exceeds u32"),
                            });
                            operation_outputs.push(
                                operation_output(
                                    &frame.module.operations[operation_index],
                                    Some(&frame.variables),
                                )
                                .into_iter()
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                            );
                            let module_operation = OperationId(
                                u32::try_from(operation_index)
                                    .expect("kernel residual operation count exceeds u32"),
                            );
                            for dependency in frame.module.operation_dependencies(module_operation)
                            {
                                let dependency = frame.variables[dependency.0 as usize];
                                assert!(
                                    (dependency.0 as usize) < reverse.len(),
                                    "kernel residual operation references an undeclared frame variable"
                                );
                                reverse[dependency.0 as usize].insert(ProgramConsumer {
                                    operation: operation_id,
                                });
                                forward_dependencies.push(dependency);
                            }
                            dependency_offsets.push(
                                u32::try_from(forward_dependencies.len())
                                    .expect("kernel dependency edge count exceeds u32"),
                            );
                        }
                    }
                }
            }
        }
        let mut offsets = Vec::with_capacity(reverse.len() + 1);
        let mut consumers = Vec::new();
        offsets.push(0);
        for variable_consumers in reverse {
            consumers.extend(variable_consumers);
            offsets.push(
                u32::try_from(consumers.len()).expect("kernel consumer edge count exceeds u32"),
            );
        }
        let (initial_order, acyclic_initial_operations) = initial_operation_order(
            self.variables.len(),
            &operation_outputs,
            &dependency_offsets,
            &forward_dependencies,
        );
        ComponentProgram {
            terms: self.terms,
            variables: self.variables.into_boxed_slice(),
            operations: self
                .operations
                .into_iter()
                .map(Arc::new)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            residual_frames: self.residual_frames.into_boxed_slice(),
            work_items: work_items.into_boxed_slice(),
            instruction_count,
            initial_order: initial_order.into_boxed_slice(),
            acyclic_initial_operations,
            dependency_offsets: dependency_offsets.into_boxed_slice(),
            dependencies: forward_dependencies.into_boxed_slice(),
            consumer_offsets: offsets.into_boxed_slice(),
            consumers: consumers.into_boxed_slice(),
            outputs: self.outputs.into_boxed_slice(),
        }
    }

    fn push_operation(&mut self, operation: KernelOperation) -> OperationId {
        let id = OperationId(
            u32::try_from(self.operations.len()).expect("kernel operation count exceeds u32"),
        );
        self.operations.push(operation);
        self.work_order.push(BuilderWorkItem::Direct(id.0));
        id
    }
}

pub(crate) fn operation_output(
    operation: &KernelOperation,
    variables: Option<&[TypeVariableId]>,
) -> Option<TypeVariableId> {
    let output = match operation {
        KernelOperation::Publish { output, .. }
        | KernelOperation::Select { output, .. }
        | KernelOperation::Record { output, .. }
        | KernelOperation::Collection { output, .. }
        | KernelOperation::SummaryCall { output, .. } => Some(*output),
        KernelOperation::Alias { consumer, .. }
        | KernelOperation::Projection { consumer, .. }
        | KernelOperation::PatternProjection { consumer, .. }
        | KernelOperation::CollectionProjection { consumer, .. } => Some(*consumer),
        KernelOperation::Unify { .. } => None,
    }?;
    Some(variables.map_or(output, |variables| variables[output.0 as usize]))
}

fn initial_operation_order(
    variable_count: usize,
    outputs: &[Box<[TypeVariableId]>],
    dependency_offsets: &[u32],
    dependencies: &[TypeVariableId],
) -> (Vec<OperationId>, u64) {
    let mut writers = vec![None::<usize>; variable_count];
    for (operation, operation_outputs) in outputs.iter().enumerate() {
        for output in operation_outputs {
            let writer = &mut writers[output.0 as usize];
            if writer.is_none() {
                *writer = Some(operation);
            }
        }
    }
    let mut outgoing = vec![Vec::<usize>::new(); outputs.len()];
    let mut indegree = vec![0_u32; outputs.len()];
    for consumer in 0..outputs.len() {
        let start = dependency_offsets[consumer] as usize;
        let end = dependency_offsets[consumer + 1] as usize;
        for dependency in &dependencies[start..end] {
            let Some(writer) = writers[dependency.0 as usize] else {
                continue;
            };
            if writer == consumer {
                continue;
            }
            outgoing[writer].push(consumer);
            indegree[consumer] = indegree[consumer].saturating_add(1);
        }
    }
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(operation, indegree)| (*indegree == 0).then_some(operation))
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(outputs.len());
    let mut emitted = vec![false; outputs.len()];
    while let Some(operation) = ready.pop_first() {
        if emitted[operation] {
            continue;
        }
        emitted[operation] = true;
        ordered.push(OperationId(
            u32::try_from(operation).expect("kernel operation count exceeds u32"),
        ));
        for consumer in &outgoing[operation] {
            indegree[*consumer] -= 1;
            if indegree[*consumer] == 0 {
                ready.insert(*consumer);
            }
        }
    }
    let acyclic = ordered.len() as u64;
    ordered.extend(
        emitted
            .iter()
            .enumerate()
            .filter_map(|(operation, emitted)| {
                (!*emitted).then(|| {
                    OperationId(
                        u32::try_from(operation).expect("kernel operation count exceeds u32"),
                    )
                })
            }),
    );
    (ordered, acyclic)
}

fn link_residual_operation_terms(
    operation: &KernelOperation,
    source: &TypeTermArena,
    target: &mut TypeTermArena,
    variables: &[TypeVariableId],
    term_cache: &mut [Option<TypeTermId>],
    name_cache: &mut [Option<NameId>],
) {
    match operation {
        KernelOperation::Unify { left, right } => {
            link_residual_term(*left, source, target, variables, term_cache, name_cache);
            link_residual_term(*right, source, target, variables, term_cache, name_cache);
        }
        KernelOperation::Alias { .. } => {}
        KernelOperation::Publish { inputs, .. } => {
            for input in inputs {
                link_residual_term(*input, source, target, variables, term_cache, name_cache);
            }
        }
        KernelOperation::Projection { field, .. } => {
            if let Some(name) = field {
                link_residual_name(*name, source, target, name_cache);
            }
        }
        KernelOperation::PatternProjection { fields, .. } => {
            for field in fields {
                link_residual_name(*field, source, target, name_cache);
            }
        }
        KernelOperation::CollectionProjection { .. } => {}
        KernelOperation::Collection { inputs, values, .. } => {
            for input in inputs.iter().chain(values.iter()) {
                link_residual_term(*input, source, target, variables, term_cache, name_cache);
            }
        }
        KernelOperation::Select { arms, .. } => {
            for arm in arms {
                link_residual_term(
                    arm.output, source, target, variables, term_cache, name_cache,
                );
            }
        }
        KernelOperation::Record { tag, entries, .. } => {
            if let Some(name) = tag {
                link_residual_name(*name, source, target, name_cache);
            }
            for entry in entries {
                match entry {
                    KernelRecordEntry::Field { name, value } => {
                        link_residual_name(*name, source, target, name_cache);
                        link_residual_term(
                            *value, source, target, variables, term_cache, name_cache,
                        );
                    }
                    KernelRecordEntry::Spread { value } => link_residual_term(
                        *value, source, target, variables, term_cache, name_cache,
                    ),
                }
            }
        }
        KernelOperation::SummaryCall { .. } => {
            panic!("parametric summary calls cannot be nested inside residual modules")
        }
    }
}

fn link_residual_term(
    term: TypeTermId,
    source: &TypeTermArena,
    target: &mut TypeTermArena,
    variables: &[TypeVariableId],
    term_cache: &mut [Option<TypeTermId>],
    name_cache: &mut [Option<NameId>],
) {
    target.import_rebased_term(source, term, variables, term_cache, name_cache);
}

fn link_residual_name(
    name: NameId,
    source: &TypeTermArena,
    target: &mut TypeTermArena,
    name_cache: &mut [Option<NameId>],
) {
    let slot = &mut name_cache[name.0 as usize];
    slot.get_or_insert_with(|| target.intern_name(source.name(name)));
}

fn collect_operation_variables(
    operation: &KernelOperation,
    terms: &TypeTermArena,
    output: &mut BTreeSet<TypeVariableId>,
) {
    match operation {
        KernelOperation::Unify { left, right } => {
            collect_term_variables(*left, terms, output);
            collect_term_variables(*right, terms, output);
        }
        KernelOperation::Alias { provider, consumer } => {
            output.insert(*provider);
            output.insert(*consumer);
        }
        KernelOperation::Publish {
            output: variable,
            inputs,
            ..
        } => {
            output.insert(*variable);
            for input in inputs {
                collect_term_variables(*input, terms, output);
            }
        }
        KernelOperation::Projection {
            provider, consumer, ..
        } => {
            output.insert(*provider);
            output.insert(*consumer);
        }
        KernelOperation::PatternProjection {
            provider, consumer, ..
        } => {
            output.insert(*provider);
            output.insert(*consumer);
        }
        KernelOperation::CollectionProjection {
            provider, consumer, ..
        } => {
            output.insert(*provider);
            output.insert(*consumer);
        }
        KernelOperation::Collection {
            output: variable,
            inputs,
            values,
            ..
        } => {
            output.insert(*variable);
            for input in inputs.iter().chain(values.iter()) {
                collect_term_variables(*input, terms, output);
            }
        }
        KernelOperation::Select {
            output: variable,
            selector,
            arms,
            ..
        } => {
            output.insert(*variable);
            output.insert(*selector);
            for arm in arms {
                collect_term_variables(arm.output, terms, output);
            }
        }
        KernelOperation::Record {
            output: variable,
            entries,
            ..
        } => {
            output.insert(*variable);
            for entry in entries {
                let value = match entry {
                    KernelRecordEntry::Field { value, .. }
                    | KernelRecordEntry::Spread { value } => *value,
                };
                collect_term_variables(value, terms, output);
            }
        }
        KernelOperation::SummaryCall {
            output: variable,
            inputs,
            ..
        } => {
            output.insert(*variable);
            for input in inputs {
                match input {
                    KernelSummaryCallInput::Term(term) => {
                        collect_term_variables(*term, terms, output);
                    }
                    KernelSummaryCallInput::Projection { provider, .. } => {
                        output.insert(*provider);
                    }
                }
            }
        }
    }
}

pub(crate) fn collect_term_variables(
    term: TypeTermId,
    terms: &TypeTermArena,
    output: &mut BTreeSet<TypeVariableId>,
) {
    match terms.term(term) {
        TypeTerm::Variable(variable) => {
            output.insert(*variable);
        }
        TypeTerm::VariantSet(variants) => {
            for variant in variants {
                if let crate::VariantTerm::Tagged { fields, .. } = variant {
                    collect_term_variables(*fields, terms, output);
                }
            }
        }
        TypeTerm::Object { fields, .. } => {
            for field in fields {
                collect_term_variables(field.ty, terms, output);
            }
        }
        TypeTerm::List(item) | TypeTerm::Set(item) => {
            collect_term_variables(*item, terms, output);
        }
        TypeTerm::Function { args, result, .. } => {
            for argument in args {
                collect_term_variables(*argument, terms, output);
            }
            collect_term_variables(*result, terms, output);
        }
        TypeTerm::Union(members) => {
            for member in members {
                collect_term_variables(*member, terms, output);
            }
        }
        TypeTerm::Map { key, value } => {
            collect_term_variables(*key, terms, output);
            collect_term_variables(*value, terms, output);
        }
        TypeTerm::Text
        | TypeTerm::Number
        | TypeTerm::Bytes(_)
        | TypeTerm::Absent
        | TypeTerm::OpenObjectPlaceholder
        | TypeTerm::RenderContract
        | TypeTerm::UnresolvedShape(_)
        | TypeTerm::Unknown
        | TypeTerm::Bits(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_index_is_dense_and_deterministic() {
        let mut builder = ComponentProgramBuilder::new();
        let input = builder.new_variable();
        let output = builder.new_variable();
        let input_term = builder.variable_term(input);
        builder.add_publish(output, [input_term], PublishMode::Union);
        builder.add_output(output, FlowMode::Continuous);
        let program = builder.finish();

        let consumer = ProgramConsumer {
            operation: OperationId(0),
        };
        assert_eq!(program.consumers(input), &[consumer]);
        assert_eq!(program.consumers(output), &[consumer]);
    }

    #[test]
    fn coarse_residual_reverse_index_subscribes_only_to_external_inputs() {
        let mut module = ComponentProgramBuilder::new();
        let module_input = module.new_variable();
        let module_projection = module.new_variable();
        let module_nested_projection = module.new_variable();
        let module_constant = module.new_authoritative_provider();
        module.add_projection_into(module_input, [], module_projection);
        module.add_projection_into(module_projection, [], module_nested_projection);
        let text = module.terms().text();
        module.add_publish(module_constant, [text], PublishMode::Replace);
        let module = Arc::new(module.finish());

        let mut builder = ComponentProgramBuilder::new();
        let input = builder.new_variable();
        let projection = builder.new_variable();
        let nested_projection = builder.new_variable();
        let constant = builder.new_authoritative_provider();
        builder.add_residual_frame(module, vec![input, projection, nested_projection, constant]);
        let program = builder.finish();

        assert_eq!(program.scheduled_work_item_count(), 1);
        assert_eq!(program.operation_count(), 3);
        assert_eq!(
            program.consumers(input),
            &[ProgramConsumer {
                operation: OperationId(0),
            }]
        );
        assert!(program.consumers(projection).is_empty());
        assert!(program.consumers(constant).is_empty());
    }
}
