use crate::{
    ArtifactOutput, ComponentArtifact, ComponentProgram, KernelCollectionOperationKind,
    KernelOperation, KernelPattern, KernelRecordEntry, KernelSelectArm, KernelSolveWork,
    KernelSummaryCallInput, KernelSummaryNode, KernelSummaryProgram, KernelSummaryRecordEntry,
    NameId, OperationId, ProgramConsumer, ProgramOperationRef, PublishMode, ResidualOperationFrame,
    TypeTerm, TypeTermId, TypeVariableId, VariantTerm,
};
use boon_checked::FlowType;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelSolveError {
    message: String,
}

impl KernelSolveError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for KernelSolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for KernelSolveError {}

#[derive(Clone, Debug)]
struct VariableCell {
    parent: TypeVariableId,
    rank: u8,
    binding: Option<TypeTermId>,
    contextual_hole: bool,
    authoritative_provider: bool,
}

/// Evaluate one compact component to quiescence.
pub fn solve_component(program: ComponentProgram) -> Result<ComponentArtifact, KernelSolveError> {
    validate_single_writers(&program)?;
    let (solver, execution) = ComponentSolver::new(program);
    solver.solve(&execution)
}

fn validate_single_writers(program: &ComponentProgram) -> Result<(), KernelSolveError> {
    let mut writers = vec![None::<OperationId>; program.variables.len()];
    let mut register = |kind: &KernelOperation,
                        variables: Option<&[TypeVariableId]>,
                        operation: OperationId|
     -> Result<(), KernelSolveError> {
        let output = match kind {
            KernelOperation::Publish { output, .. } => Some(*output),
            KernelOperation::Projection { consumer, .. }
            | KernelOperation::CollectionItemProjection { consumer, .. } => Some(*consumer),
            KernelOperation::Select { output, .. }
            | KernelOperation::Record { output, .. }
            | KernelOperation::Collection { output, .. }
            | KernelOperation::SummaryCall { output, .. } => Some(*output),
            KernelOperation::Unify { .. } => None,
        }
        .map(|output| variables.map_or(output, |variables| variables[output.0 as usize]));
        let Some(output) = output else {
            return Ok(());
        };
        if let Some(previous) = writers[output.0 as usize].replace(operation) {
            return Err(KernelSolveError::new(format!(
                "kernel variable {} has multiple directional writers: operations {} and {}",
                output.0, previous.0, operation.0
            )));
        }
        Ok(())
    };
    for (index, work_item) in program.work_items.iter().enumerate() {
        let operation =
            OperationId(u32::try_from(index).expect("kernel operation count exceeds u32"));
        match *work_item {
            ProgramOperationRef::Direct(direct) => {
                register(
                    program.operations[direct as usize].as_ref(),
                    None,
                    operation,
                )?;
            }
            ProgramOperationRef::ResidualFrame { frame } => {
                let frame = &program.residual_frames[frame as usize];
                for kind in frame.module.operations.iter() {
                    register(kind.as_ref(), Some(frame.variables.as_ref()), operation)?;
                }
            }
            ProgramOperationRef::Residual {
                frame,
                operation: residual,
            } => {
                let frame = &program.residual_frames[frame as usize];
                register(
                    frame.module.operations[residual as usize].as_ref(),
                    Some(frame.variables.as_ref()),
                    operation,
                )?;
            }
        }
    }
    Ok(())
}

struct ComponentSolver {
    program: SolverStateProgram,
    cells: Vec<VariableCell>,
    pending: VecDeque<OperationId>,
    queued: Vec<bool>,
    replayable: Vec<bool>,
    self_replayable: Vec<bool>,
    active_operation: Option<OperationId>,
    binding_dependencies: Vec<Vec<TypeVariableId>>,
    binding_dependents: Vec<Vec<TypeVariableId>>,
    equivalence_next: Vec<Option<TypeVariableId>>,
    equivalence_head: Vec<TypeVariableId>,
    equivalence_tail: Vec<TypeVariableId>,
    schedule_seen: Vec<u32>,
    schedule_generation: u32,
    schedule_stack: Vec<TypeVariableId>,
    summary_memo: Vec<Option<TypeTermId>>,
    summary_active: Vec<bool>,
    resolve_cache_seen: Vec<u32>,
    resolve_cache_values: Vec<TypeTermId>,
    resolve_active: Vec<u32>,
    resolve_generation: u32,
    occurs_active: Vec<u32>,
    occurs_generation: u32,
    term_visit_seen: Vec<u32>,
    variable_visit_seen: Vec<u32>,
    term_visit_stack: Vec<TypeTermId>,
    term_variable_buffer: Vec<TypeVariableId>,
    term_visit_generation: u32,
    work: KernelSolveWork,
}

struct SolverExecution {
    operations: Box<[std::sync::Arc<KernelOperation>]>,
    residual_frames: Box<[ResidualOperationFrame]>,
    work_items: Box<[ProgramOperationRef]>,
    static_equalities: Box<[StaticEqualityRef]>,
}

#[derive(Clone, Copy, Debug)]
enum StaticEqualityRef {
    Direct(u32),
    Residual { frame: u32, operation: u32 },
}

struct SolverStateProgram {
    terms: crate::TypeTermArena,
    consumer_offsets: Box<[u32]>,
    consumers: Box<[ProgramConsumer]>,
    outputs: Box<[crate::ProgramOutput]>,
}

impl SolverStateProgram {
    fn consumers(&self, variable: TypeVariableId) -> &[ProgramConsumer] {
        let index = variable.0 as usize;
        let start = self.consumer_offsets[index] as usize;
        let end = self.consumer_offsets[index + 1] as usize;
        &self.consumers[start..end]
    }
}

impl ComponentSolver {
    fn new(program: ComponentProgram) -> (Self, SolverExecution) {
        let ComponentProgram {
            terms,
            variables,
            operations,
            residual_frames,
            work_items,
            instruction_count,
            initial_order,
            acyclic_initial_operations,
            dependency_offsets: _,
            dependencies: _,
            consumer_offsets,
            consumers,
            outputs,
        } = program;
        let cells = variables
            .iter()
            .enumerate()
            .map(|(index, spec)| VariableCell {
                parent: TypeVariableId(
                    u32::try_from(index).expect("kernel variable count exceeds u32"),
                ),
                rank: 0,
                binding: None,
                contextual_hole: spec.contextual_hole,
                authoritative_provider: spec.authoritative_provider,
            })
            .collect::<Vec<_>>();
        let scheduled_work_items = work_items.len();
        let variable_count = cells.len();
        let variable_ids = (0..variable_count)
            .map(|index| {
                TypeVariableId(u32::try_from(index).expect("kernel variable count exceeds u32"))
            })
            .collect::<Vec<_>>();
        // Equality equations establish the union-find/scaffold namespace. They
        // are persistent relationships, not reactive computations, so install
        // them once before any directional provider runs. Interleaving them
        // with publishers used to invalidate already-evaluated consumers and
        // made an almost entirely acyclic NovyWave graph execute nearly twice.
        let mut static_equalities = Vec::new();
        let mut pending = VecDeque::with_capacity(scheduled_work_items);
        let mut queued = vec![false; scheduled_work_items];
        let mut replayable = vec![true; scheduled_work_items];
        let mut self_replayable = vec![false; scheduled_work_items];
        let acyclic_initial_operations = usize::try_from(acyclic_initial_operations)
            .unwrap_or(scheduled_work_items)
            .min(scheduled_work_items);
        for (position, operation) in initial_order.into_iter().enumerate() {
            self_replayable[operation.0 as usize] = position >= acyclic_initial_operations
                && !matches!(
                    work_items[operation.0 as usize],
                    ProgramOperationRef::ResidualFrame { .. }
                );
            let has_directional = match work_items[operation.0 as usize] {
                ProgramOperationRef::Direct(direct) => {
                    let operation_kind = operations[direct as usize].as_ref();
                    if matches!(operation_kind, KernelOperation::Unify { .. }) {
                        static_equalities.push(StaticEqualityRef::Direct(direct));
                        false
                    } else {
                        true
                    }
                }
                ProgramOperationRef::ResidualFrame { frame } => {
                    let frame_value = &residual_frames[frame as usize];
                    let mut has_directional = false;
                    for (residual, operation_kind) in
                        frame_value.module.operations.iter().enumerate()
                    {
                        if matches!(operation_kind.as_ref(), KernelOperation::Unify { .. }) {
                            static_equalities.push(StaticEqualityRef::Residual {
                                frame,
                                operation: u32::try_from(residual)
                                    .expect("kernel residual operation count exceeds u32"),
                            });
                        } else {
                            has_directional = true;
                        }
                    }
                    has_directional
                }
                ProgramOperationRef::Residual {
                    frame,
                    operation: residual,
                } => {
                    let operation_kind = residual_frames[frame as usize].module.operations
                        [residual as usize]
                        .as_ref();
                    if matches!(operation_kind, KernelOperation::Unify { .. }) {
                        static_equalities.push(StaticEqualityRef::Residual {
                            frame,
                            operation: residual,
                        });
                        false
                    } else {
                        true
                    }
                }
            };
            if !has_directional {
                replayable[operation.0 as usize] = false;
            } else {
                queued[operation.0 as usize] = true;
                pending.push_back(operation);
            }
        }
        let mut terms = terms;
        terms.reset_work();
        let solver = Self {
            work: KernelSolveWork {
                variables: cells.len() as u64,
                scheduled_work_items: scheduled_work_items as u64,
                operations: instruction_count,
                ..KernelSolveWork::default()
            },
            program: SolverStateProgram {
                terms,
                consumer_offsets,
                consumers,
                outputs,
            },
            cells,
            pending,
            queued,
            replayable,
            self_replayable,
            active_operation: None,
            binding_dependencies: vec![Vec::new(); variable_count],
            binding_dependents: vec![Vec::new(); variable_count],
            equivalence_next: vec![None; variable_count],
            equivalence_head: variable_ids.clone(),
            equivalence_tail: variable_ids,
            schedule_seen: vec![0; variable_count],
            schedule_generation: 0,
            schedule_stack: Vec::new(),
            summary_memo: Vec::new(),
            summary_active: Vec::new(),
            resolve_cache_seen: Vec::new(),
            resolve_cache_values: Vec::new(),
            resolve_active: vec![0; variable_count],
            resolve_generation: 0,
            occurs_active: vec![0; variable_count],
            occurs_generation: 0,
            term_visit_seen: Vec::new(),
            variable_visit_seen: vec![0; variable_count],
            term_visit_stack: Vec::new(),
            term_variable_buffer: Vec::new(),
            term_visit_generation: 0,
        };
        let execution = SolverExecution {
            operations,
            residual_frames,
            work_items,
            static_equalities: static_equalities.into_boxed_slice(),
        };
        (solver, execution)
    }

    fn solve(mut self, execution: &SolverExecution) -> Result<ComponentArtifact, KernelSolveError> {
        for operation in execution.static_equalities.iter().copied() {
            self.activate_static_equality(execution, operation)?;
        }
        while let Some(operation) = self.pending.pop_front() {
            self.queued[operation.0 as usize] = false;
            self.activate(execution, operation)?;
        }

        let output_specs = self.program.outputs.clone();
        let mut outputs = Vec::with_capacity(output_specs.len());
        for output in output_specs.iter() {
            let variable = self.program.terms.variable(output.variable);
            let term = self.resolve_term(variable);
            self.work.term_materializations = self.work.term_materializations.saturating_add(1);
            outputs.push(ArtifactOutput {
                id: output.id,
                flow_type: FlowType {
                    mode: output.mode,
                    ty: self.program.terms.export_checked_type(term),
                },
            });
        }
        let term_work = self.program.terms.work();
        self.work.term_intern_requests = term_work.intern_requests;
        self.work.term_intern_hits = term_work.intern_hits;
        self.work.structural_widen_requests = term_work.structural_widen_requests;
        self.work.structural_widen_hits = term_work.structural_widen_hits;
        Ok(ComponentArtifact::new(
            outputs.into_boxed_slice(),
            self.work,
        ))
    }

    fn activate(
        &mut self,
        execution: &SolverExecution,
        operation: OperationId,
    ) -> Result<(), KernelSolveError> {
        self.active_operation = Some(operation);
        let result = match execution.work_items[operation.0 as usize] {
            ProgramOperationRef::Direct(direct) => {
                let operation_kind = execution.operations[direct as usize].as_ref();
                self.work.activations = self.work.activations.saturating_add(1);
                self.count_operation_activation(operation_kind);
                self.evaluate(operation_kind)
            }
            ProgramOperationRef::ResidualFrame { frame } => {
                let frame_value = &execution.residual_frames[frame as usize];
                let mut result = Ok(());
                for module_operation in frame_value.module.initial_order.iter().copied() {
                    let ProgramOperationRef::Direct(residual) =
                        frame_value.module.work_items[module_operation.0 as usize]
                    else {
                        unreachable!("residual modules cannot contain nested physical frames")
                    };
                    let operation_kind = frame_value.module.operations[residual as usize].as_ref();
                    if matches!(operation_kind, KernelOperation::Unify { .. }) {
                        continue;
                    }
                    self.work.activations = self.work.activations.saturating_add(1);
                    self.count_operation_activation(operation_kind);
                    if let Err(error) =
                        self.evaluate_residual(frame as usize, frame_value, operation_kind)
                    {
                        result = Err(error);
                        break;
                    }
                }
                result
            }
            ProgramOperationRef::Residual {
                frame,
                operation: residual,
            } => {
                let frame_value = &execution.residual_frames[frame as usize];
                let operation_kind = frame_value.module.operations[residual as usize].as_ref();
                self.work.activations = self.work.activations.saturating_add(1);
                self.count_operation_activation(operation_kind);
                self.evaluate_residual(frame as usize, frame_value, operation_kind)
            }
        };
        self.active_operation = None;
        result
    }

    fn activate_static_equality(
        &mut self,
        execution: &SolverExecution,
        operation: StaticEqualityRef,
    ) -> Result<(), KernelSolveError> {
        self.work.activations = self.work.activations.saturating_add(1);
        match operation {
            StaticEqualityRef::Direct(operation) => {
                let operation = execution.operations[operation as usize].as_ref();
                self.count_operation_activation(operation);
                self.evaluate(operation)
            }
            StaticEqualityRef::Residual { frame, operation } => {
                let frame_value = &execution.residual_frames[frame as usize];
                let operation = frame_value.module.operations[operation as usize].as_ref();
                self.count_operation_activation(operation);
                self.evaluate_residual(frame as usize, frame_value, operation)
            }
        }
    }

    fn count_operation_activation(&mut self, operation: &KernelOperation) {
        let counter = match operation {
            KernelOperation::Unify { .. } => &mut self.work.unify_activations,
            KernelOperation::Publish { .. } => &mut self.work.publish_activations,
            KernelOperation::Projection { .. }
            | KernelOperation::CollectionItemProjection { .. } => {
                &mut self.work.projection_activations
            }
            KernelOperation::Collection { .. } => &mut self.work.publish_activations,
            KernelOperation::Select { .. } => &mut self.work.select_activations,
            KernelOperation::Record { .. } | KernelOperation::SummaryCall { .. } => {
                &mut self.work.record_activations
            }
        };
        *counter = counter.saturating_add(1);
    }

    fn evaluate(&mut self, operation: &KernelOperation) -> Result<(), KernelSolveError> {
        match operation {
            KernelOperation::Unify { left, right } => {
                self.work.union_operations = self.work.union_operations.saturating_add(1);
                self.unify_terms(*left, *right);
            }
            KernelOperation::Publish {
                output,
                inputs,
                mode,
            } => self.publish(*output, inputs, *mode)?,
            KernelOperation::Projection {
                provider,
                field,
                consumer,
            } => self.project(*provider, *field, *consumer),
            KernelOperation::CollectionItemProjection { provider, consumer } => {
                self.project_collection_item(*provider, *consumer)
            }
            KernelOperation::Collection {
                output,
                kind,
                inputs,
                values,
            } => self.collection(*output, *kind, inputs, values),
            KernelOperation::Select {
                output,
                selector,
                arms,
            } => self.select(*output, *selector, arms),
            KernelOperation::Record {
                output,
                tag,
                entries,
            } => self.record(*output, *tag, entries)?,
            KernelOperation::SummaryCall {
                output,
                program,
                inputs,
            } => self.summary_call(*output, program, inputs)?,
        }
        Ok(())
    }

    fn evaluate_residual(
        &mut self,
        frame_index: usize,
        frame: &ResidualOperationFrame,
        operation: &KernelOperation,
    ) -> Result<(), KernelSolveError> {
        let variable = |variable: TypeVariableId| frame.variables[variable.0 as usize];
        match operation {
            KernelOperation::Unify { left, right } => {
                self.work.union_operations = self.work.union_operations.saturating_add(1);
                let left = self.import_frame_term(frame_index, frame, *left);
                let right = self.import_frame_term(frame_index, frame, *right);
                self.unify_terms(left, right);
            }
            KernelOperation::Publish {
                output,
                inputs,
                mode,
            } => self.publish_residual(frame_index, frame, variable(*output), inputs, *mode)?,
            KernelOperation::Projection {
                provider,
                field,
                consumer,
            } => {
                let field = field.map(|field| self.import_frame_name(frame_index, frame, field));
                self.project(variable(*provider), field, variable(*consumer));
            }
            KernelOperation::CollectionItemProjection { provider, consumer } => {
                self.project_collection_item(variable(*provider), variable(*consumer));
            }
            KernelOperation::Collection {
                output,
                kind,
                inputs,
                values,
            } => self.collection_residual(
                frame_index,
                frame,
                variable(*output),
                *kind,
                inputs,
                values,
            ),
            KernelOperation::Select {
                output,
                selector,
                arms,
            } => self.select_residual(
                frame_index,
                frame,
                variable(*output),
                variable(*selector),
                arms,
            ),
            KernelOperation::Record {
                output,
                tag,
                entries,
            } => self.record_residual(frame_index, frame, variable(*output), *tag, entries)?,
            KernelOperation::SummaryCall { .. } => {
                return Err(KernelSolveError::new(
                    "parametric summary calls cannot execute inside residual modules",
                ));
            }
        }
        Ok(())
    }

    fn publish_residual(
        &mut self,
        frame_index: usize,
        frame: &ResidualOperationFrame,
        output: TypeVariableId,
        inputs: &[TypeTermId],
        mode: PublishMode,
    ) -> Result<(), KernelSolveError> {
        match mode {
            PublishMode::Unify => {
                let output = self.program.terms.variable(output);
                for input in inputs {
                    let input = self.import_frame_term(frame_index, frame, *input);
                    self.unify_terms(output, input);
                }
            }
            PublishMode::Union => {
                let mut resolved = Vec::with_capacity(inputs.len());
                for input in inputs {
                    let input = self.import_frame_term(frame_index, frame, *input);
                    resolved.push(self.resolve_term_head(input));
                }
                let provider = self.program.terms.union(resolved);
                self.replace_binding(output, provider, true);
            }
            PublishMode::StructuralWiden => {
                let mut provider = None;
                for input in inputs {
                    let input = self.import_frame_term(frame_index, frame, *input);
                    let input = self.resolve_term(input);
                    if matches!(self.program.terms.term(input), TypeTerm::Variable(_)) {
                        continue;
                    }
                    provider = Some(match provider {
                        None => input,
                        Some(current) => self.program.terms.structural_widen(current, input),
                    });
                }
                let provider = provider.unwrap_or_else(|| self.program.terms.absent());
                self.replace_binding(output, provider, true);
            }
            PublishMode::Replace => {
                let [provider] = inputs else {
                    return Err(KernelSolveError::new(format!(
                        "authoritative residual publication for variable {} has {} providers instead of one",
                        output.0,
                        inputs.len()
                    )));
                };
                let provider = self.import_frame_term(frame_index, frame, *provider);
                let provider = self.resolve_term_head(provider);
                self.replace_binding(output, provider, true);
            }
        }
        Ok(())
    }

    fn collection_residual(
        &mut self,
        frame_index: usize,
        frame: &ResidualOperationFrame,
        output: TypeVariableId,
        kind: KernelCollectionOperationKind,
        inputs: &[TypeTermId],
        values: &[TypeTermId],
    ) {
        let inputs = inputs
            .iter()
            .map(|input| self.import_frame_term(frame_index, frame, *input))
            .collect::<Vec<_>>();
        let values = values
            .iter()
            .map(|value| self.import_frame_term(frame_index, frame, *value))
            .collect::<Vec<_>>();
        self.collection(output, kind, &inputs, &values);
    }

    fn select_residual(
        &mut self,
        frame_index: usize,
        frame: &ResidualOperationFrame,
        output: TypeVariableId,
        selector: TypeVariableId,
        arms: &[KernelSelectArm],
    ) {
        let selector_term = self.program.terms.variable(selector);
        let selector = self.resolve_term(selector_term);
        let singleton = matches!(
            self.program.terms.term(selector),
            TypeTerm::VariantSet(variants) if variants.len() == 1
        );
        let mut candidates = Vec::new();
        for arm in arms {
            if singleton && !self.pattern_accepts(selector, &arm.pattern) {
                continue;
            }
            let candidate = self.import_frame_term(frame_index, frame, arm.output);
            let candidate = self.resolve_term(candidate);
            if matches!(self.program.terms.term(candidate), TypeTerm::Absent) {
                continue;
            }
            candidates.push(candidate);
            if singleton {
                break;
            }
        }
        let provider = self.join_select_candidates(candidates);
        self.replace_binding(output, provider, true);
    }

    fn record_residual(
        &mut self,
        frame_index: usize,
        frame: &ResidualOperationFrame,
        output: TypeVariableId,
        tag: Option<NameId>,
        entries: &[KernelRecordEntry],
    ) -> Result<(), KernelSolveError> {
        let mut fields = Vec::<(NameId, TypeTermId)>::new();
        for entry in entries {
            match entry {
                KernelRecordEntry::Field { name, value } => {
                    let name = self.import_frame_name(frame_index, frame, *name);
                    let value = self.import_frame_term(frame_index, frame, *value);
                    let value = self.resolve_term_head(value);
                    insert_record_field(&mut fields, name, value);
                }
                KernelRecordEntry::Spread { value } => {
                    let value = self.import_frame_term(frame_index, frame, *value);
                    let value = self.resolve_term_head(value);
                    self.merge_record_spread(value, &mut fields)?;
                }
            }
        }
        let object = self.program.terms.object(fields, false);
        let provider = if let Some(tag) = tag {
            let tag = self.import_frame_name(frame_index, frame, tag);
            let tag = self.program.terms.name(tag).to_owned();
            let variant = self.program.terms.tagged_variant(tag, object);
            self.program.terms.variant_set([variant])
        } else {
            object
        };
        self.replace_binding(output, provider, true);
        Ok(())
    }

    fn import_frame_term(
        &mut self,
        _frame_index: usize,
        frame: &ResidualOperationFrame,
        term: TypeTermId,
    ) -> TypeTermId {
        frame.terms[term.0 as usize].expect("residual operation term was linked")
    }

    fn import_frame_name(
        &mut self,
        _frame_index: usize,
        frame: &ResidualOperationFrame,
        name: NameId,
    ) -> NameId {
        frame.names[name.0 as usize].expect("residual operation name was linked")
    }

    fn publish(
        &mut self,
        output: TypeVariableId,
        inputs: &[TypeTermId],
        mode: PublishMode,
    ) -> Result<(), KernelSolveError> {
        match mode {
            PublishMode::Unify => {
                let output = self.program.terms.variable(output);
                for input in inputs {
                    self.unify_terms(output, *input);
                }
            }
            PublishMode::Union => {
                let resolved = inputs
                    .iter()
                    .map(|input| self.resolve_term_head(*input))
                    .collect::<Vec<_>>();
                let provider = self.program.terms.union(resolved);
                self.replace_binding(output, provider, true);
            }
            PublishMode::StructuralWiden => {
                let mut resolved_inputs = Vec::with_capacity(inputs.len());
                for input in inputs {
                    let input = self.resolve_term(*input);
                    if !matches!(self.program.terms.term(input), TypeTerm::Variable(_)) {
                        resolved_inputs.push(input);
                    }
                }
                let mut inputs = resolved_inputs.into_iter();
                let mut provider = inputs.next().unwrap_or_else(|| self.program.terms.absent());
                for input in inputs {
                    provider = self.program.terms.structural_widen(provider, input);
                }
                self.replace_binding(output, provider, true);
            }
            PublishMode::Replace => {
                let [provider] = inputs else {
                    return Err(KernelSolveError::new(format!(
                        "authoritative publication for variable {} has {} providers instead of one",
                        output.0,
                        inputs.len()
                    )));
                };
                let provider = self.resolve_term_head(*provider);
                self.replace_binding(output, provider, true);
            }
        }
        Ok(())
    }

    fn collection(
        &mut self,
        output: TypeVariableId,
        kind: KernelCollectionOperationKind,
        inputs: &[TypeTermId],
        values: &[TypeTermId],
    ) {
        let provider = self.collection_type(kind, inputs, values);
        self.replace_binding(output, provider, true);
    }

    fn collection_type(
        &mut self,
        kind: KernelCollectionOperationKind,
        inputs: &[TypeTermId],
        values: &[TypeTermId],
    ) -> TypeTermId {
        let first = if inputs.is_empty() {
            match kind {
                KernelCollectionOperationKind::List => self.program.terms.open_object(),
                KernelCollectionOperationKind::Set | KernelCollectionOperationKind::Map => {
                    self.program.terms.unknown()
                }
            }
        } else {
            self.structural_widen_inputs(inputs)
        };
        let provider = match kind {
            KernelCollectionOperationKind::List => self.program.terms.list(first),
            KernelCollectionOperationKind::Set => self.program.terms.set(first),
            KernelCollectionOperationKind::Map => {
                let value = if values.is_empty() {
                    self.program.terms.unknown()
                } else {
                    self.structural_widen_inputs(values)
                };
                self.program.terms.map(first, value)
            }
        };
        provider
    }

    fn structural_widen_inputs(&mut self, inputs: &[TypeTermId]) -> TypeTermId {
        let mut provider = None;
        for input in inputs {
            let input = self.resolve_term(*input);
            if matches!(self.program.terms.term(input), TypeTerm::Variable(_)) {
                continue;
            }
            provider = Some(match provider {
                None => input,
                Some(current) => self.program.terms.structural_widen(current, input),
            });
        }
        provider.unwrap_or_else(|| self.program.terms.absent())
    }

    fn project(
        &mut self,
        provider: TypeVariableId,
        field: Option<NameId>,
        consumer: TypeVariableId,
    ) {
        let provider = self.root(provider);
        let authoritative = self.cells[provider.0 as usize].authoritative_provider;
        let provider_term = self.program.terms.variable(provider);
        // Projection must retain nested variable identities. Recursively
        // materializing the whole provider here turns an unresolved scaffold
        // into a detached value snapshot; a later sibling path would then
        // shape the snapshot rather than the original formal tree.
        let resolved = self.resolve_term_head(provider_term);
        // A provider whose current epoch is still a bare variable has not
        // proved that any projection is missing. Defer it until the actual
        // value or structural shape arrives; invocation-local requirement
        // equations may meanwhile constrain their detached occurrence.
        if authoritative && matches!(self.program.terms.term(resolved), TypeTerm::Variable(_)) {
            return;
        }
        let projected = match field {
            None if matches!(self.program.terms.term(resolved), TypeTerm::Variable(_)) => None,
            None => Some(resolved),
            Some(field) => self.project_field(resolved, field),
        };
        match projected {
            Some(projected) if authoritative => {
                self.replace_binding(consumer, projected, true);
            }
            Some(projected) => self.bind_equal(consumer, projected),
            None if authoritative => {
                let path = field
                    .map(|field| self.program.terms.name(field).to_owned())
                    .unwrap_or_else(|| "<value>".to_owned());
                let missing = self
                    .program
                    .terms
                    .unresolved_shape(format!("authoritative provider omits projection `{path}`"));
                self.replace_binding(consumer, missing, true);
            }
            None => {
                let Some(field) = field else {
                    return;
                };
                let consumer_term = self.program.terms.variable(consumer);
                let scaffold = self.program.terms.object([(field, consumer_term)], true);
                self.bind_equal(provider, scaffold);
            }
        }
    }

    fn project_collection_item(&mut self, provider: TypeVariableId, consumer: TypeVariableId) {
        let provider = self.root(provider);
        let authoritative = self.cells[provider.0 as usize].authoritative_provider;
        let provider_term = self.program.terms.variable(provider);
        let resolved = self.resolve_term_head(provider_term);
        if authoritative && matches!(self.program.terms.term(resolved), TypeTerm::Variable(_)) {
            return;
        }
        let projected = self.collection_item_type(resolved);
        match projected {
            Some(projected) if authoritative => {
                self.replace_binding(consumer, projected, true);
            }
            Some(projected) => self.bind_equal(consumer, projected),
            None if authoritative => {
                let missing = self
                    .program
                    .terms
                    .unresolved_shape("authoritative provider is not a collection");
                self.replace_binding(consumer, missing, true);
            }
            None => {}
        }
    }

    fn collection_item_type(&mut self, provider: TypeTermId) -> Option<TypeTermId> {
        let provider = self.resolve_term_head(provider);
        match self.program.terms.term(provider).clone() {
            TypeTerm::List(item) | TypeTerm::Set(item) => Some(item),
            TypeTerm::Union(members) => {
                let items = members
                    .iter()
                    .filter_map(|member| self.collection_item_type(*member))
                    .collect::<Vec<_>>();
                (!items.is_empty()).then(|| self.program.terms.union(items))
            }
            TypeTerm::Variable(_) | TypeTerm::Unknown | TypeTerm::UnresolvedShape(_) => None,
            _ => None,
        }
    }

    fn select(
        &mut self,
        output: TypeVariableId,
        selector: TypeVariableId,
        arms: &[KernelSelectArm],
    ) {
        let selector_term = self.program.terms.variable(selector);
        let selector = self.resolve_term(selector_term);
        let singleton = matches!(
            self.program.terms.term(selector),
            TypeTerm::VariantSet(variants) if variants.len() == 1
        );
        let mut candidates = Vec::new();
        for arm in arms {
            if singleton && !self.pattern_accepts(selector, &arm.pattern) {
                continue;
            }
            let candidate = self.resolve_term(arm.output);
            if matches!(self.program.terms.term(candidate), TypeTerm::Absent) {
                continue;
            }
            candidates.push(candidate);
            if singleton {
                break;
            }
        }
        let provider = self.join_select_candidates(candidates);
        self.replace_binding(output, provider, true);
    }

    /// Preserve unresolved arm identities until the invocation frame closes.
    /// Once every candidate is concrete, use the language's structural branch
    /// join instead of retaining a union of compatible lists/records.
    fn join_select_candidates(&mut self, candidates: Vec<TypeTermId>) -> TypeTermId {
        if candidates.is_empty() {
            return self.program.terms.absent();
        }
        if candidates
            .iter()
            .copied()
            .any(|candidate| self.term_contains_variable(candidate))
        {
            return self.program.terms.union(candidates);
        }
        let mut candidates = candidates.into_iter();
        let first = candidates
            .next()
            .expect("non-empty select candidates were checked");
        candidates.fold(first, |current, candidate| {
            self.program.terms.structural_widen(current, candidate)
        })
    }

    fn pattern_accepts(&self, selector: TypeTermId, pattern: &KernelPattern) -> bool {
        match pattern {
            KernelPattern::Wildcard | KernelPattern::Binding { .. } => true,
            KernelPattern::Number => matches!(self.program.terms.term(selector), TypeTerm::Number),
            KernelPattern::Text => matches!(self.program.terms.term(selector), TypeTerm::Text),
            KernelPattern::Bits { width } => {
                matches!(self.program.terms.term(selector), TypeTerm::Bits(actual) if actual == width)
            }
            KernelPattern::Tag { name, .. } => {
                matches!(self.program.terms.term(selector), TypeTerm::VariantSet(variants) if variants.iter().any(|variant| self.program.terms.name(variant.tag()) == name.as_ref()))
            }
            KernelPattern::Invalid => false,
        }
    }

    fn record(
        &mut self,
        output: TypeVariableId,
        tag: Option<NameId>,
        entries: &[KernelRecordEntry],
    ) -> Result<(), KernelSolveError> {
        let mut fields = Vec::<(NameId, TypeTermId)>::new();
        for entry in entries {
            match entry {
                KernelRecordEntry::Field { name, value } => {
                    let value = self.resolve_term_head(*value);
                    insert_record_field(&mut fields, *name, value);
                }
                KernelRecordEntry::Spread { value } => {
                    let value = self.resolve_term_head(*value);
                    self.merge_record_spread(value, &mut fields)?;
                }
            }
        }
        let object = self.program.terms.object(fields, false);
        let provider = if let Some(tag) = tag {
            let tag = self.program.terms.name(tag).to_owned();
            let variant = self.program.terms.tagged_variant(tag, object);
            self.program.terms.variant_set([variant])
        } else {
            object
        };
        self.replace_binding(output, provider, true);
        Ok(())
    }

    fn summary_call(
        &mut self,
        output: TypeVariableId,
        program: &KernelSummaryProgram,
        inputs: &[KernelSummaryCallInput],
    ) -> Result<(), KernelSolveError> {
        self.work.summary_call_activations = self.work.summary_call_activations.saturating_add(1);
        let mut memo = std::mem::take(&mut self.summary_memo);
        memo.clear();
        memo.resize(program.nodes.len(), None);
        let mut active = std::mem::take(&mut self.summary_active);
        active.clear();
        active.resize(program.nodes.len(), false);
        let result =
            self.evaluate_summary_value(program, inputs, program.result, &mut memo, &mut active);
        self.summary_memo = memo;
        self.summary_active = active;
        let result = result?;
        self.replace_binding(output, result, true);
        Ok(())
    }

    fn evaluate_summary_value(
        &mut self,
        program: &KernelSummaryProgram,
        inputs: &[KernelSummaryCallInput],
        value: crate::KernelSummaryValueId,
        memo: &mut [Option<TypeTermId>],
        active: &mut [bool],
    ) -> Result<TypeTermId, KernelSolveError> {
        let index = value.0 as usize;
        if let Some(value) = memo.get(index).copied().flatten() {
            return Ok(value);
        }
        let Some(node) = program.nodes.get(index) else {
            return Err(KernelSolveError::new(format!(
                "kernel summary references unavailable value {}",
                value.0
            )));
        };
        if active[index] {
            return Err(KernelSolveError::new(format!(
                "kernel summary contains a value cycle through {}",
                value.0
            )));
        }
        active[index] = true;
        self.work.summary_node_evaluations = self.work.summary_node_evaluations.saturating_add(1);
        let evaluated = (|| match node {
            KernelSummaryNode::Input(input_index) => {
                let input = inputs.get(*input_index as usize).ok_or_else(|| {
                    KernelSolveError::new(format!(
                        "kernel summary input {input_index} is out of range for {} inputs",
                        inputs.len()
                    ))
                })?;
                match input {
                    KernelSummaryCallInput::Term(term) => Ok(self.resolve_term_head(*term)),
                    KernelSummaryCallInput::Projection { provider, steps } => {
                        if steps.is_empty() {
                            return Err(KernelSolveError::new(format!(
                                "kernel summary input {input_index} has an empty projection program"
                            )));
                        }
                        let mut provider = *provider;
                        for step in steps {
                            self.project(provider, step.field, step.consumer);
                            provider = step.consumer;
                        }
                        let provider = self.program.terms.variable(provider);
                        Ok(self.resolve_term_head(provider))
                    }
                }
            }
            KernelSummaryNode::Term(term) => Ok(*term),
            KernelSummaryNode::Constrain { value, expected } => {
                let actual = self.evaluate_summary_value(program, inputs, *value, memo, active)?;
                self.unify_terms(actual, *expected);
                Ok(self.resolve_term_head(actual))
            }
            KernelSummaryNode::Sequence {
                inputs: dependencies,
                result,
            } => {
                for dependency in dependencies {
                    self.evaluate_summary_value(program, inputs, *dependency, memo, active)?;
                }
                self.evaluate_summary_value(program, inputs, *result, memo, active)
            }
            KernelSummaryNode::Collection {
                kind,
                inputs: item_values,
                values: map_values,
            } => {
                let mut items = Vec::with_capacity(item_values.len());
                for value in item_values {
                    items.push(self.evaluate_summary_value(program, inputs, *value, memo, active)?);
                }
                let mut values = Vec::with_capacity(map_values.len());
                for value in map_values {
                    values
                        .push(self.evaluate_summary_value(program, inputs, *value, memo, active)?);
                }
                Ok(self.collection_type(*kind, &items, &values))
            }
            KernelSummaryNode::Select { selector, arms } => {
                let selector =
                    self.evaluate_summary_value(program, inputs, *selector, memo, active)?;
                let selector = self.resolve_term(selector);
                let singleton = matches!(
                    self.program.terms.term(selector),
                    TypeTerm::VariantSet(variants) if variants.len() == 1
                );
                if singleton {
                    for arm in arms {
                        if !self.pattern_accepts(selector, &arm.pattern) {
                            continue;
                        }
                        let candidate =
                            self.evaluate_summary_value(program, inputs, arm.output, memo, active)?;
                        let candidate = self.resolve_term(candidate);
                        if !matches!(self.program.terms.term(candidate), TypeTerm::Absent) {
                            return Ok(candidate);
                        }
                    }
                    return Ok(self.program.terms.absent());
                }
                let mut candidates = Vec::new();
                for arm in arms {
                    let candidate =
                        self.evaluate_summary_value(program, inputs, arm.output, memo, active)?;
                    let candidate = self.resolve_term(candidate);
                    if matches!(self.program.terms.term(candidate), TypeTerm::Absent) {
                        continue;
                    }
                    candidates.push(candidate);
                }
                Ok(self.join_select_candidates(candidates))
            }
            KernelSummaryNode::Record { tag, entries } => {
                let mut fields = Vec::<(NameId, TypeTermId)>::new();
                for entry in entries {
                    match entry {
                        KernelSummaryRecordEntry::Field { name, value } => {
                            let value =
                                self.evaluate_summary_value(program, inputs, *value, memo, active)?;
                            let value = self.resolve_term_head(value);
                            insert_record_field(&mut fields, *name, value);
                        }
                        KernelSummaryRecordEntry::Spread { value } => {
                            let value =
                                self.evaluate_summary_value(program, inputs, *value, memo, active)?;
                            let value = self.resolve_term_head(value);
                            self.merge_record_spread(value, &mut fields)?;
                        }
                    }
                }
                let object = self.program.terms.object(fields, false);
                if let Some(tag) = tag {
                    let tag = self.program.terms.name(*tag).to_owned();
                    let variant = self.program.terms.tagged_variant(tag, object);
                    Ok(self.program.terms.variant_set([variant]))
                } else {
                    Ok(object)
                }
            }
        })();
        active[index] = false;
        let evaluated = evaluated?;
        memo[index] = Some(evaluated);
        Ok(evaluated)
    }

    fn merge_record_spread(
        &mut self,
        spread: TypeTermId,
        fields: &mut Vec<(NameId, TypeTermId)>,
    ) -> Result<(), KernelSolveError> {
        match self.program.terms.term(spread).clone() {
            TypeTerm::Object {
                fields: spread_fields,
                ..
            } => {
                for field in spread_fields {
                    insert_record_field(fields, field.name, field.ty);
                }
            }
            TypeTerm::Union(members) => {
                for member in members {
                    self.merge_record_spread(member, fields)?;
                }
            }
            TypeTerm::VariantSet(variants)
                if variants
                    .iter()
                    .any(|variant| self.program.terms.name(variant.tag()) == "UNPLUGGED") => {}
            TypeTerm::Variable(_) | TypeTerm::Unknown | TypeTerm::UnresolvedShape(_) => {}
            invalid => {
                return Err(KernelSolveError::new(format!(
                    "kernel record spread expects a record value, found {invalid:?}"
                )));
            }
        }
        Ok(())
    }

    fn project_field(&mut self, provider: TypeTermId, field: NameId) -> Option<TypeTermId> {
        let provider = self.resolve_term_head(provider);
        match self.program.terms.term(provider).clone() {
            TypeTerm::Object { fields, .. } => fields
                .iter()
                .find(|candidate| candidate.name == field)
                .map(|candidate| candidate.ty),
            TypeTerm::Union(members) => {
                let projected = members
                    .iter()
                    .filter_map(|member| self.project_field(*member, field))
                    .collect::<Vec<_>>();
                (!projected.is_empty()).then(|| self.program.terms.union(projected))
            }
            TypeTerm::VariantSet(variants) => {
                let projected = variants
                    .iter()
                    .filter_map(|variant| match variant {
                        VariantTerm::Tagged { fields, .. } => self.project_field(*fields, field),
                        VariantTerm::Tag(_) => None,
                    })
                    .collect::<Vec<_>>();
                (!projected.is_empty()).then(|| self.program.terms.union(projected))
            }
            _ => None,
        }
    }

    fn resolve_term_head(&mut self, mut term: TypeTermId) -> TypeTermId {
        loop {
            let TypeTerm::Variable(variable) = self.program.terms.term(term) else {
                return term;
            };
            let root = self.root(*variable);
            match self.cells[root.0 as usize].binding {
                Some(binding) => term = binding,
                None => return self.program.terms.variable(root),
            }
        }
    }

    fn unify_terms(&mut self, left: TypeTermId, right: TypeTermId) {
        let left = self.resolve_term_head(left);
        let right = self.resolve_term_head(right);
        if left == right {
            return;
        }
        let left_term = self.program.terms.term(left).clone();
        let right_term = self.program.terms.term(right).clone();
        match (left_term, right_term) {
            (TypeTerm::Variable(_), TypeTerm::Unknown | TypeTerm::UnresolvedShape(_))
            | (TypeTerm::Unknown | TypeTerm::UnresolvedShape(_), TypeTerm::Variable(_)) => {}
            (TypeTerm::Variable(variable), _) => self.bind_equal(variable, right),
            (_, TypeTerm::Variable(variable)) => self.bind_equal(variable, left),
            (TypeTerm::Object { fields: left, .. }, TypeTerm::Object { fields: right, .. }) => {
                for left in left.iter() {
                    if let Some(right) = right.iter().find(|right| right.name == left.name) {
                        self.unify_terms(left.ty, right.ty);
                    }
                }
            }
            (TypeTerm::List(left), TypeTerm::List(right))
            | (TypeTerm::Set(left), TypeTerm::Set(right)) => self.unify_terms(left, right),
            (
                TypeTerm::Map {
                    key: left_key,
                    value: left_value,
                },
                TypeTerm::Map {
                    key: right_key,
                    value: right_value,
                },
            ) => {
                self.unify_terms(left_key, right_key);
                self.unify_terms(left_value, right_value);
            }
            (
                TypeTerm::Function {
                    args: left_args,
                    result: left_result,
                    ..
                },
                TypeTerm::Function {
                    args: right_args,
                    result: right_result,
                    ..
                },
            ) if left_args.len() == right_args.len() => {
                for (left, right) in left_args.iter().zip(right_args.iter()) {
                    self.unify_terms(*left, *right);
                }
                self.unify_terms(left_result, right_result);
            }
            // A union is a directional value surface. Equating it with a
            // concrete requirement must not collapse every branch together.
            (TypeTerm::Union(_), _) | (_, TypeTerm::Union(_)) => {}
            _ => {}
        }
    }

    fn bind_equal(&mut self, variable: TypeVariableId, incoming: TypeTermId) {
        let variable = self.root(variable);
        if let TypeTerm::Variable(other) = self.program.terms.term(incoming) {
            self.union_variables(variable, *other);
            return;
        }
        let incoming = self.resolve_term_head(incoming);
        if self.occurs(variable, incoming) {
            return;
        }
        let current = self.cells[variable.0 as usize].binding;
        let merged = match current {
            None => incoming,
            Some(current) => self.merge_equal_terms(current, incoming),
        };
        if current == Some(merged) {
            return;
        }
        self.replace_binding_dependencies(variable, merged);
        self.cells[variable.0 as usize].binding = Some(merged);
        self.touch(variable);
    }

    fn replace_binding(
        &mut self,
        variable: TypeVariableId,
        provider: TypeTermId,
        authoritative: bool,
    ) {
        let variable = self.root(variable);
        let provider = self.resolve_term_head(provider);
        if self.occurs(variable, provider) {
            return;
        }
        let mut changed = self.cells[variable.0 as usize].binding != Some(provider);
        if authoritative && !self.cells[variable.0 as usize].authoritative_provider {
            self.cells[variable.0 as usize].authoritative_provider = true;
            changed = true;
        }
        if !changed {
            return;
        }
        self.replace_binding_dependencies(variable, provider);
        self.cells[variable.0 as usize].binding = Some(provider);
        self.touch(variable);
    }

    fn merge_equal_terms(&mut self, left: TypeTermId, right: TypeTermId) -> TypeTermId {
        if let TypeTerm::Variable(variable) = self.program.terms.term(left) {
            let variable = *variable;
            self.bind_equal(variable, right);
            let root = self.root(variable);
            return self.program.terms.variable(root);
        }
        if let TypeTerm::Variable(variable) = self.program.terms.term(right) {
            let variable = *variable;
            self.bind_equal(variable, left);
            let root = self.root(variable);
            return self.program.terms.variable(root);
        }
        let left = self.resolve_term_head(left);
        let right = self.resolve_term_head(right);
        if left == right {
            return left;
        }
        let left_term = self.program.terms.term(left).clone();
        let right_term = self.program.terms.term(right).clone();
        match (left_term, right_term) {
            (TypeTerm::Variable(_), _) | (_, TypeTerm::Variable(_)) => {
                unreachable!("term heads were resolved above")
            }
            (
                TypeTerm::Object {
                    fields: left_fields,
                    open: left_open,
                },
                TypeTerm::Object {
                    fields: right_fields,
                    open: right_open,
                },
            ) => {
                let mut fields = left_fields.into_vec();
                for right in right_fields {
                    if let Some(index) = fields.iter().position(|left| left.name == right.name) {
                        fields[index].ty = self.merge_equal_terms(fields[index].ty, right.ty);
                    } else {
                        fields.push(right);
                    }
                }
                self.program.terms.object(
                    fields.into_iter().map(|field| (field.name, field.ty)),
                    left_open || right_open,
                )
            }
            (TypeTerm::List(left), TypeTerm::List(right)) => {
                let item = self.merge_equal_terms(left, right);
                self.program.terms.list(item)
            }
            (TypeTerm::Set(left), TypeTerm::Set(right)) => {
                let item = self.merge_equal_terms(left, right);
                self.program.terms.set(item)
            }
            (
                TypeTerm::Map {
                    key: left_key,
                    value: left_value,
                },
                TypeTerm::Map {
                    key: right_key,
                    value: right_value,
                },
            ) => {
                let key = self.merge_equal_terms(left_key, right_key);
                let value = self.merge_equal_terms(left_value, right_value);
                self.program.terms.map(key, value)
            }
            (
                TypeTerm::Function {
                    args: left_args,
                    result_mode: left_mode,
                    result: left_result,
                },
                TypeTerm::Function {
                    args: right_args,
                    result_mode: right_mode,
                    result: right_result,
                },
            ) if left_args.len() == right_args.len() && left_mode == right_mode => {
                let args = left_args
                    .iter()
                    .zip(right_args.iter())
                    .map(|(left, right)| self.merge_equal_terms(*left, *right))
                    .collect::<Vec<_>>();
                let result = self.merge_equal_terms(left_result, right_result);
                self.program.terms.function(args, left_mode, result)
            }
            _ => self.program.terms.structural_widen(left, right),
        }
    }

    fn union_variables(&mut self, left: TypeVariableId, right: TypeVariableId) {
        let mut left = self.root(left);
        let mut right = self.root(right);
        if left == right {
            return;
        }
        if self.cells[left.0 as usize].rank < self.cells[right.0 as usize].rank {
            std::mem::swap(&mut left, &mut right);
        }
        let right_binding = self.cells[right.0 as usize].binding.take();
        self.clear_binding_dependencies(right);
        let right_contextual = self.cells[right.0 as usize].contextual_hole;
        let right_authoritative = self.cells[right.0 as usize].authoritative_provider;
        self.cells[right.0 as usize].parent = left;
        self.cells[left.0 as usize].contextual_hole |= right_contextual;
        self.cells[left.0 as usize].authoritative_provider |= right_authoritative;
        if self.cells[left.0 as usize].rank == self.cells[right.0 as usize].rank {
            self.cells[left.0 as usize].rank = self.cells[left.0 as usize].rank.saturating_add(1);
        }
        let left_tail = self.equivalence_tail[left.0 as usize];
        let right_head = self.equivalence_head[right.0 as usize];
        let right_tail = self.equivalence_tail[right.0 as usize];
        self.equivalence_next[left_tail.0 as usize] = Some(right_head);
        self.equivalence_tail[left.0 as usize] = right_tail;
        self.touch(left);
        self.touch(right);
        if let Some(right_binding) = right_binding {
            self.bind_equal(left, right_binding);
        }
    }

    fn root(&mut self, variable: TypeVariableId) -> TypeVariableId {
        let parent = self.cells[variable.0 as usize].parent;
        if parent == variable {
            return variable;
        }
        let root = self.root(parent);
        self.cells[variable.0 as usize].parent = root;
        root
    }

    fn root_readonly(&self, mut variable: TypeVariableId) -> TypeVariableId {
        loop {
            let parent = self.cells[variable.0 as usize].parent;
            if parent == variable {
                return variable;
            }
            variable = parent;
        }
    }

    fn resolve_term(&mut self, term: TypeTermId) -> TypeTermId {
        if !self.program.terms.has_variable(term) {
            return term;
        }
        self.resolve_generation = next_generation(
            &mut self.resolve_generation,
            &mut self.resolve_cache_seen,
            &mut self.resolve_active,
        );
        let term_count = self.program.terms.len();
        if self.resolve_cache_seen.len() < term_count {
            self.resolve_cache_seen.resize(term_count, 0);
            self.resolve_cache_values
                .resize(term_count, self.program.terms.absent());
        }
        self.resolve_term_inner(term, self.resolve_generation)
    }

    fn resolve_term_inner(&mut self, term: TypeTermId, generation: u32) -> TypeTermId {
        if !self.program.terms.has_variable(term) {
            return term;
        }
        let source = self.program.terms.term(term).clone();
        if !matches!(source, TypeTerm::Variable(_))
            && self.resolve_cache_seen[term.0 as usize] == generation
        {
            return self.resolve_cache_values[term.0 as usize];
        }
        let resolved = match source {
            TypeTerm::Variable(variable) => {
                let root = self.root(variable);
                if self.resolve_active[root.0 as usize] == generation {
                    return self.program.terms.variable(root);
                }
                self.resolve_active[root.0 as usize] = generation;
                let binding = self.cells[root.0 as usize].binding;
                let resolved = match binding {
                    Some(binding) => self.resolve_term_inner(binding, generation),
                    None => self.program.terms.variable(root),
                };
                self.resolve_active[root.0 as usize] = 0;
                resolved
            }
            TypeTerm::VariantSet(variants) => {
                let variants = variants
                    .into_vec()
                    .into_iter()
                    .map(|variant| match variant {
                        VariantTerm::Tag(tag) => VariantTerm::Tag(tag),
                        VariantTerm::Tagged { tag, fields } => VariantTerm::Tagged {
                            tag,
                            fields: self.resolve_term_inner(fields, generation),
                        },
                    })
                    .collect::<Vec<_>>();
                self.program.terms.variant_set_preserving_order(variants)
            }
            TypeTerm::Object { fields, open } => {
                let fields = fields
                    .into_vec()
                    .into_iter()
                    .map(|field| (field.name, self.resolve_term_inner(field.ty, generation)))
                    .collect::<Vec<_>>();
                self.program.terms.object(fields, open)
            }
            TypeTerm::List(item) => {
                let item = self.resolve_term_inner(item, generation);
                self.program.terms.list(item)
            }
            TypeTerm::Set(item) => {
                let item = self.resolve_term_inner(item, generation);
                self.program.terms.set(item)
            }
            TypeTerm::Map { key, value } => {
                let key = self.resolve_term_inner(key, generation);
                let value = self.resolve_term_inner(value, generation);
                self.program.terms.map(key, value)
            }
            TypeTerm::Function {
                args,
                result_mode,
                result,
            } => {
                let args = args
                    .iter()
                    .map(|argument| self.resolve_term_inner(*argument, generation))
                    .collect::<Vec<_>>();
                let result = self.resolve_term_inner(result, generation);
                self.program.terms.function(args, result_mode, result)
            }
            TypeTerm::Union(members) => {
                let members = members
                    .iter()
                    .map(|member| self.resolve_term_inner(*member, generation))
                    .collect::<Vec<_>>();
                self.program.terms.union(members)
            }
            TypeTerm::Text
            | TypeTerm::Number
            | TypeTerm::Bytes(_)
            | TypeTerm::Absent
            | TypeTerm::OpenObjectPlaceholder
            | TypeTerm::RenderContract
            | TypeTerm::UnresolvedShape(_)
            | TypeTerm::Unknown
            | TypeTerm::Bits(_) => term,
        };
        if !matches!(self.program.terms.term(term), TypeTerm::Variable(_)) {
            self.resolve_cache_seen[term.0 as usize] = generation;
            self.resolve_cache_values[term.0 as usize] = resolved;
        }
        resolved
    }

    fn occurs(&mut self, variable: TypeVariableId, term: TypeTermId) -> bool {
        self.occurs_generation = next_generation(
            &mut self.occurs_generation,
            &mut self.occurs_active,
            &mut [],
        );
        term_occurs(
            &self.program.terms,
            &self.cells,
            &mut self.occurs_active,
            self.occurs_generation,
            variable,
            term,
        )
    }

    fn replace_binding_dependencies(&mut self, parent: TypeVariableId, binding: TypeTermId) {
        let parent = self.root(parent);
        self.clear_binding_dependencies(parent);
        self.collect_term_variables(binding);
        let mut dependencies = std::mem::take(&mut self.term_variable_buffer);
        for dependency in dependencies.iter_mut() {
            *dependency = self.root(*dependency);
        }
        dependencies.sort_unstable();
        dependencies.dedup();
        for dependency in dependencies.iter().copied() {
            let dependency = self.root(dependency);
            if dependency != parent {
                self.insert_binding_dependent(dependency, parent);
                self.binding_dependencies[parent.0 as usize].push(dependency);
            }
        }
        dependencies.clear();
        self.term_variable_buffer = dependencies;
    }

    fn collect_term_variables(&mut self, term: TypeTermId) {
        self.term_visit_generation = next_generation(
            &mut self.term_visit_generation,
            &mut self.term_visit_seen,
            &mut self.variable_visit_seen,
        );
        let term_count = self.program.terms.len();
        if self.term_visit_seen.len() < term_count {
            self.term_visit_seen.resize(term_count, 0);
        }
        self.term_visit_stack.clear();
        self.term_variable_buffer.clear();
        collect_term_variables_dense(
            &self.program.terms,
            term,
            &mut self.term_visit_seen,
            &mut self.variable_visit_seen,
            self.term_visit_generation,
            &mut self.term_visit_stack,
            &mut self.term_variable_buffer,
        );
    }

    fn term_contains_variable(&mut self, term: TypeTermId) -> bool {
        self.collect_term_variables(term);
        !self.term_variable_buffer.is_empty()
    }

    fn clear_binding_dependencies(&mut self, parent: TypeVariableId) {
        let dependencies = std::mem::take(&mut self.binding_dependencies[parent.0 as usize]);
        for dependency in dependencies {
            let dependents = &mut self.binding_dependents[dependency.0 as usize];
            if let Ok(index) = dependents.binary_search(&parent) {
                dependents.remove(index);
                self.work.dynamic_dependency_edges =
                    self.work.dynamic_dependency_edges.saturating_sub(1);
            }
        }
    }

    fn insert_binding_dependent(&mut self, dependency: TypeVariableId, dependent: TypeVariableId) {
        let dependents = &mut self.binding_dependents[dependency.0 as usize];
        if let Err(index) = dependents.binary_search(&dependent) {
            dependents.insert(index, dependent);
            self.work.dynamic_dependency_edges =
                self.work.dynamic_dependency_edges.saturating_add(1);
        }
    }

    fn touch(&mut self, variable: TypeVariableId) {
        self.work.mutations = self.work.mutations.saturating_add(1);
        self.schedule_variable(variable);
    }

    fn schedule_variable(&mut self, variable: TypeVariableId) {
        self.schedule_generation = self.schedule_generation.wrapping_add(1);
        if self.schedule_generation == 0 {
            self.schedule_seen.fill(0);
            self.schedule_generation = 1;
        }
        self.schedule_stack.clear();
        let root = self.root_readonly(variable);
        let mut member = Some(self.equivalence_head[root.0 as usize]);
        while let Some(variable) = member {
            self.schedule_stack.push(variable);
            member = self.equivalence_next[variable.0 as usize];
        }
        while let Some(dependency) = self.schedule_stack.pop() {
            let root = self.root_readonly(dependency);
            for dependency in [dependency, root] {
                let seen = &mut self.schedule_seen[dependency.0 as usize];
                if *seen == self.schedule_generation {
                    continue;
                }
                *seen = self.schedule_generation;
                for consumer in self.program.consumers(dependency) {
                    let operation = consumer.operation;
                    let index = operation.0 as usize;
                    if self.active_operation == Some(operation) && !self.self_replayable[index] {
                        continue;
                    }
                    if self.replayable[index] && !self.queued[index] {
                        self.queued[index] = true;
                        self.pending.push_back(operation);
                    }
                }
                self.schedule_stack.extend(
                    self.binding_dependents[dependency.0 as usize]
                        .iter()
                        .rev()
                        .copied(),
                );
            }
        }
    }
}

fn next_generation(generation: &mut u32, first: &mut [u32], second: &mut [u32]) -> u32 {
    *generation = generation.wrapping_add(1);
    if *generation == 0 {
        first.fill(0);
        second.fill(0);
        *generation = 1;
    }
    *generation
}

fn readonly_cell_root(cells: &[VariableCell], mut variable: TypeVariableId) -> TypeVariableId {
    loop {
        let parent = cells[variable.0 as usize].parent;
        if parent == variable {
            return variable;
        }
        variable = parent;
    }
}

fn term_occurs(
    terms: &crate::TypeTermArena,
    cells: &[VariableCell],
    active: &mut [u32],
    generation: u32,
    variable: TypeVariableId,
    term: TypeTermId,
) -> bool {
    if !terms.has_variable(term) {
        return false;
    }
    match terms.term(term) {
        TypeTerm::Variable(candidate) => {
            let candidate = readonly_cell_root(cells, *candidate);
            if candidate == variable {
                return true;
            }
            if active[candidate.0 as usize] == generation {
                return false;
            }
            active[candidate.0 as usize] = generation;
            let occurs = cells[candidate.0 as usize].binding.is_some_and(|binding| {
                term_occurs(terms, cells, active, generation, variable, binding)
            });
            active[candidate.0 as usize] = 0;
            occurs
        }
        TypeTerm::VariantSet(variants) => variants.iter().any(|variant| match variant {
            VariantTerm::Tagged { fields, .. } => {
                term_occurs(terms, cells, active, generation, variable, *fields)
            }
            VariantTerm::Tag(_) => false,
        }),
        TypeTerm::Object { fields, .. } => fields
            .iter()
            .any(|field| term_occurs(terms, cells, active, generation, variable, field.ty)),
        TypeTerm::List(item) | TypeTerm::Set(item) => {
            term_occurs(terms, cells, active, generation, variable, *item)
        }
        TypeTerm::Function { args, result, .. } => {
            args.iter()
                .any(|argument| term_occurs(terms, cells, active, generation, variable, *argument))
                || term_occurs(terms, cells, active, generation, variable, *result)
        }
        TypeTerm::Union(members) => members
            .iter()
            .any(|member| term_occurs(terms, cells, active, generation, variable, *member)),
        TypeTerm::Map { key, value } => {
            term_occurs(terms, cells, active, generation, variable, *key)
                || term_occurs(terms, cells, active, generation, variable, *value)
        }
        TypeTerm::Text
        | TypeTerm::Number
        | TypeTerm::Bytes(_)
        | TypeTerm::Absent
        | TypeTerm::OpenObjectPlaceholder
        | TypeTerm::RenderContract
        | TypeTerm::UnresolvedShape(_)
        | TypeTerm::Unknown
        | TypeTerm::Bits(_) => false,
    }
}

fn collect_term_variables_dense(
    terms: &crate::TypeTermArena,
    term: TypeTermId,
    term_seen: &mut [u32],
    variable_seen: &mut [u32],
    generation: u32,
    stack: &mut Vec<TypeTermId>,
    output: &mut Vec<TypeVariableId>,
) {
    stack.push(term);
    while let Some(term) = stack.pop() {
        let index = term.0 as usize;
        if term_seen[index] == generation || !terms.has_variable(term) {
            continue;
        }
        term_seen[index] = generation;
        match terms.term(term) {
            TypeTerm::Variable(variable) => {
                let index = variable.0 as usize;
                if variable_seen[index] != generation {
                    variable_seen[index] = generation;
                    output.push(*variable);
                }
            }
            TypeTerm::VariantSet(variants) => {
                stack.extend(variants.iter().filter_map(|variant| match variant {
                    VariantTerm::Tagged { fields, .. } => Some(*fields),
                    VariantTerm::Tag(_) => None,
                }));
            }
            TypeTerm::Object { fields, .. } => {
                stack.extend(fields.iter().map(|field| field.ty));
            }
            TypeTerm::List(item) | TypeTerm::Set(item) => stack.push(*item),
            TypeTerm::Function { args, result, .. } => {
                stack.push(*result);
                stack.extend(args.iter().copied());
            }
            TypeTerm::Union(members) => stack.extend(members.iter().copied()),
            TypeTerm::Map { key, value } => {
                stack.push(*value);
                stack.push(*key);
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
}

fn insert_record_field(fields: &mut Vec<(NameId, TypeTermId)>, name: NameId, value: TypeTermId) {
    if let Some((_, current)) = fields.iter_mut().find(|(candidate, _)| *candidate == name) {
        *current = value;
    } else {
        fields.push((name, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComponentProgramBuilder, KernelSummarySelectArm, PublishMode};
    use boon_checked::{FlowMode, ObjectShape, Type, Variant};
    use std::sync::Arc;

    #[test]
    fn shared_summary_select_does_not_evaluate_unselected_requirements() {
        let mut builder = ComponentProgramBuilder::new();
        let selector = builder.new_authoritative_provider();
        let actual = builder.new_contextual_hole();
        let output = builder.new_authoritative_provider();
        let true_tag = builder.terms_mut().variant_tag("True");
        let true_type = builder.terms_mut().variant_set([true_tag]);
        builder.add_publish(selector, [true_type], PublishMode::Replace);

        let text = builder.terms().text();
        let number = builder.terms().number();
        let summary = Arc::new(KernelSummaryProgram {
            nodes: vec![
                KernelSummaryNode::Input(0),
                KernelSummaryNode::Input(1),
                KernelSummaryNode::Term(text),
                KernelSummaryNode::Term(number),
                KernelSummaryNode::Constrain {
                    value: crate::KernelSummaryValueId(1),
                    expected: number,
                },
                KernelSummaryNode::Sequence {
                    inputs: vec![crate::KernelSummaryValueId(4)].into_boxed_slice(),
                    result: crate::KernelSummaryValueId(3),
                },
                KernelSummaryNode::Select {
                    selector: crate::KernelSummaryValueId(0),
                    arms: vec![
                        KernelSummarySelectArm {
                            pattern: KernelPattern::Tag {
                                name: "True".into(),
                                fields: Box::new([]),
                            },
                            output: crate::KernelSummaryValueId(2),
                        },
                        KernelSummarySelectArm {
                            pattern: KernelPattern::Wildcard,
                            output: crate::KernelSummaryValueId(5),
                        },
                    ]
                    .into_boxed_slice(),
                },
            ]
            .into_boxed_slice(),
            result: crate::KernelSummaryValueId(6),
        });
        let selector = builder.variable_term(selector);
        let projected = builder.new_variable();
        let value = builder.terms_mut().intern_name("value");
        builder.add_summary_call(
            output,
            summary,
            [
                KernelSummaryCallInput::Term(selector),
                KernelSummaryCallInput::Projection {
                    provider: actual,
                    steps: vec![crate::KernelSummaryProjectionStep {
                        field: Some(value),
                        consumer: projected,
                    }]
                    .into_boxed_slice(),
                },
            ],
        );
        let actual_output = builder.add_output(actual, FlowMode::Continuous);
        let result_output = builder.add_output(output, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        assert!(matches!(
            artifact.output(actual_output).unwrap().flow_type.ty,
            Type::Var(_)
        ));
        assert_eq!(
            artifact.output(result_output).unwrap().flow_type.ty,
            Type::Text
        );
    }

    #[test]
    fn demanded_summary_projection_shapes_its_private_formal_requirement() {
        let mut builder = ComponentProgramBuilder::new();
        let actual = builder.new_contextual_hole();
        let output = builder.new_authoritative_provider();
        let projected = builder.new_variable();
        let value = builder.terms_mut().intern_name("value");
        let number = builder.terms().number();
        let text = builder.terms().text();
        let summary = Arc::new(KernelSummaryProgram {
            nodes: vec![
                KernelSummaryNode::Input(0),
                KernelSummaryNode::Constrain {
                    value: crate::KernelSummaryValueId(0),
                    expected: number,
                },
                KernelSummaryNode::Term(text),
                KernelSummaryNode::Sequence {
                    inputs: vec![crate::KernelSummaryValueId(1)].into_boxed_slice(),
                    result: crate::KernelSummaryValueId(2),
                },
            ]
            .into_boxed_slice(),
            result: crate::KernelSummaryValueId(3),
        });
        builder.add_summary_call(
            output,
            summary,
            [KernelSummaryCallInput::Projection {
                provider: actual,
                steps: vec![crate::KernelSummaryProjectionStep {
                    field: Some(value),
                    consumer: projected,
                }]
                .into_boxed_slice(),
            }],
        );
        let actual_output = builder.add_output(actual, FlowMode::Continuous);
        let result_output = builder.add_output(output, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        let Type::Object(actual) = &artifact.output(actual_output).unwrap().flow_type.ty else {
            panic!("selected projected requirement must shape the open formal")
        };
        assert!(actual.open);
        assert_eq!(actual.fields["value"], Type::Number);
        assert_eq!(
            artifact.output(result_output).unwrap().flow_type.ty,
            Type::Text
        );
    }

    #[test]
    fn authoritative_replacement_removes_stale_binding_dependency_edges() {
        let mut builder = ComponentProgramBuilder::new();
        let parent = builder.new_authoritative_provider();
        let first_leaf = builder.new_variable();
        let second_leaf = builder.new_variable();
        let value = builder.terms_mut().intern_name("value");
        let first_leaf_term = builder.variable_term(first_leaf);
        let first = builder
            .terms_mut()
            .object([(value, first_leaf_term)], false);
        let second_leaf_term = builder.variable_term(second_leaf);
        let second = builder
            .terms_mut()
            .object([(value, second_leaf_term)], false);
        let (mut solver, _) = ComponentSolver::new(builder.finish());

        solver.replace_binding(parent, first, true);
        assert_eq!(solver.binding_dependents[first_leaf.0 as usize], [parent]);
        assert_eq!(solver.work.dynamic_dependency_edges, 1);

        solver.replace_binding(parent, second, true);
        assert!(solver.binding_dependents[first_leaf.0 as usize].is_empty());
        assert_eq!(solver.binding_dependents[second_leaf.0 as usize], [parent]);
        assert_eq!(solver.work.dynamic_dependency_edges, 1);
    }

    #[test]
    fn select_structurally_widens_closed_branch_outputs() {
        let mut builder = ComponentProgramBuilder::new();
        let selector = builder.new_authoritative_provider();
        let output = builder.new_authoritative_provider();
        let true_variant = builder.terms_mut().variant_tag("True");
        let false_variant = builder.terms_mut().variant_tag("False");
        let selector_type = builder
            .terms_mut()
            .variant_set([true_variant, false_variant]);
        builder.add_publish(selector, [selector_type], PublishMode::Replace);

        let kind = builder.terms_mut().intern_name("kind");
        let header_variant = builder.terms_mut().variant_tag("Header");
        let header = builder.terms_mut().variant_set([header_variant]);
        let empty_variant = builder.terms_mut().variant_tag("Empty");
        let empty = builder.terms_mut().variant_set([empty_variant]);
        let header = builder.terms_mut().object([(kind, header)], false);
        let header = builder.terms_mut().list(header);
        let empty = builder.terms_mut().object([(kind, empty)], false);
        let empty = builder.terms_mut().list(empty);
        builder.add_select(
            output,
            selector,
            [
                KernelSelectArm {
                    pattern: KernelPattern::Tag {
                        name: "True".into(),
                        fields: Box::new([]),
                    },
                    output: header,
                },
                KernelSelectArm {
                    pattern: KernelPattern::Tag {
                        name: "False".into(),
                        fields: Box::new([]),
                    },
                    output: empty,
                },
            ],
        );
        let output = builder.add_output(output, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        let Type::List(item) = &artifact.output(output).unwrap().flow_type.ty else {
            panic!("WHEN branch join must retain one list")
        };
        let Type::Object(shape) = item.as_ref() else {
            panic!("WHEN list items must structurally widen")
        };
        assert_eq!(
            shape.fields["kind"],
            Type::VariantSet(
                vec![
                    Variant::Tag("Empty".to_owned()),
                    Variant::Tag("Header".to_owned())
                ]
                .into()
            )
        );
    }

    #[test]
    fn packed_collection_join_keeps_precise_producers() {
        let mut builder = ComponentProgramBuilder::new();
        let header = builder.new_variable();
        let empty = builder.new_variable();
        let list = builder.new_authoritative_provider();
        let kind = builder.terms_mut().intern_name("kind");
        let header_variant = builder.terms_mut().variant_tag("Header");
        let header_tag = builder.terms_mut().variant_set([header_variant]);
        let empty_variant = builder.terms_mut().variant_tag("Empty");
        let empty_tag = builder.terms_mut().variant_set([empty_variant]);
        let header_record = builder.terms_mut().object([(kind, header_tag)], false);
        let empty_record = builder.terms_mut().object([(kind, empty_tag)], false);
        builder.add_publish(header, [header_record], PublishMode::Replace);
        builder.add_publish(empty, [empty_record], PublishMode::Replace);
        let header_term = builder.variable_term(header);
        let empty_term = builder.variable_term(empty);
        builder.add_collection(
            list,
            KernelCollectionOperationKind::List,
            [header_term, empty_term],
            [],
        );
        let output = builder.add_output(list, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        let Type::List(item) = &artifact.output(output).unwrap().flow_type.ty else {
            panic!("collection output must be a list")
        };
        let Type::Object(shape) = item.as_ref() else {
            panic!("heterogeneous records structurally widen to one record")
        };
        assert_eq!(
            shape.fields["kind"],
            Type::VariantSet(
                vec![
                    Variant::Tag("Empty".to_owned()),
                    Variant::Tag("Header".to_owned())
                ]
                .into()
            )
        );
        assert_eq!(artifact.work.operations, 3);
        assert!(artifact.work.activations < 20);
    }

    #[test]
    fn packed_map_widens_keys_and_values_in_one_operation() {
        let mut builder = ComponentProgramBuilder::new();
        let output = builder.new_authoritative_provider();
        let key_a = builder.terms_mut().variant_tag("KeyA");
        let key_a = builder.terms_mut().variant_set([key_a]);
        let key_b = builder.terms_mut().variant_tag("KeyB");
        let key_b = builder.terms_mut().variant_set([key_b]);
        let value_a = builder.terms_mut().variant_tag("ValueA");
        let value_a = builder.terms_mut().variant_set([value_a]);
        let value_b = builder.terms_mut().variant_tag("ValueB");
        let value_b = builder.terms_mut().variant_set([value_b]);
        builder.add_collection(
            output,
            KernelCollectionOperationKind::Map,
            [key_a, key_b],
            [value_a, value_b],
        );
        let output = builder.add_output(output, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        assert_eq!(
            artifact.output(output).unwrap().flow_type.ty,
            Type::Map {
                key: Box::new(Type::VariantSet(
                    vec![
                        Variant::Tag("KeyA".to_owned()),
                        Variant::Tag("KeyB".to_owned()),
                    ]
                    .into(),
                )),
                value: Box::new(Type::VariantSet(
                    vec![
                        Variant::Tag("ValueA".to_owned()),
                        Variant::Tag("ValueB".to_owned()),
                    ]
                    .into(),
                )),
            }
        );
        assert_eq!(artifact.work.operations, 1);
    }

    #[test]
    fn record_overlay_orders_a_late_spread_without_backflow() {
        let mut builder = ComponentProgramBuilder::new();
        let provider = builder.new_authoritative_provider();
        let output = builder.new_authoritative_provider();
        let family = builder.terms_mut().intern_name("family");
        let size = builder.terms_mut().intern_name("size");
        let color = builder.terms_mut().intern_name("color");
        let provider_term = builder.variable_term(provider);
        let override_family = builder.terms().number();
        let color_value = builder.terms().text();
        builder.add_record(
            output,
            None,
            [
                KernelRecordEntry::Spread {
                    value: provider_term,
                },
                KernelRecordEntry::Field {
                    name: family,
                    value: override_family,
                },
                KernelRecordEntry::Field {
                    name: color,
                    value: color_value,
                },
            ],
        );
        let provider_family = builder.terms().text();
        let provider_size = builder.terms().number();
        let provider_value = builder
            .terms_mut()
            .object([(family, provider_family), (size, provider_size)], false);
        builder.add_publish(provider, [provider_value], PublishMode::Replace);
        let provider_output = builder.add_output(provider, FlowMode::Continuous);
        let overlay_output = builder.add_output(output, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        assert_eq!(
            artifact.output(provider_output).unwrap().flow_type.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("family".to_owned(), Type::Text),
                    ("size".to_owned(), Type::Number),
                ],
                false,
            ))
        );
        assert_eq!(
            artifact.output(overlay_output).unwrap().flow_type.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("family".to_owned(), Type::Number),
                    ("size".to_owned(), Type::Number),
                    ("color".to_owned(), Type::Text),
                ],
                false,
            ))
        );
        assert_eq!(artifact.work.activations, artifact.work.operations);
    }

    #[test]
    fn cyclic_structural_widen_defers_unresolved_providers_until_they_settle() {
        let mut builder = ComponentProgramBuilder::new();
        let selected = builder.new_authoritative_provider();
        let available = builder.new_authoritative_provider();
        let workspace_a_variant = builder.terms_mut().variant_tag("WorkspaceA");
        let workspace_a = builder.terms_mut().variant_set([workspace_a_variant]);
        let repo_variant = builder.terms_mut().variant_tag("Repo");
        let repo = builder.terms_mut().variant_set([repo_variant]);
        let workspace_b_variant = builder.terms_mut().variant_tag("WorkspaceB");
        let workspace_b = builder.terms_mut().variant_set([workspace_b_variant]);
        let available_term = builder.variable_term(available);
        builder.add_publish(
            selected,
            [workspace_a, available_term],
            PublishMode::StructuralWiden,
        );
        let selected_term = builder.variable_term(selected);
        builder.add_publish(
            available,
            [selected_term, repo, workspace_b],
            PublishMode::StructuralWiden,
        );
        let selected_output = builder.add_output(selected, FlowMode::Continuous);
        let available_output = builder.add_output(available, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        let expected = Type::VariantSet(
            vec![
                Variant::Tag("Repo".to_owned()),
                Variant::Tag("WorkspaceA".to_owned()),
                Variant::Tag("WorkspaceB".to_owned()),
            ]
            .into(),
        );
        assert_eq!(
            artifact.output(selected_output).unwrap().flow_type.ty,
            expected
        );
        assert_eq!(
            artifact.output(available_output).unwrap().flow_type.ty,
            expected
        );
        assert_eq!(artifact.work.operations, 2);
        assert!(artifact.work.activations < 10);
    }

    #[test]
    fn nested_projection_replays_after_a_late_provider_change() {
        let mut builder = ComponentProgramBuilder::new();
        let tags = builder.new_authoritative_provider();
        let provider = builder.new_authoritative_provider();
        let a = builder.terms_mut().intern_name("a");
        let b = builder.terms_mut().intern_name("b");
        let first_variant = builder.terms_mut().variant_tag("First");
        let first = builder.terms_mut().variant_set([first_variant]);
        let second_variant = builder.terms_mut().variant_tag("Second");
        let second = builder.terms_mut().variant_set([second_variant]);
        builder.add_publish(tags, [first, second], PublishMode::Union);
        let tags_term = builder.variable_term(tags);
        let inner = builder.terms_mut().object([(b, tags_term)], false);
        let outer = builder.terms_mut().object([(a, inner)], false);
        builder.add_publish(provider, [outer], PublishMode::Replace);
        let projected = builder.add_projection(provider, [a, b]);
        let output = builder.add_output(projected, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        assert_eq!(
            artifact.output(output).unwrap().flow_type.ty,
            Type::VariantSet(
                vec![
                    Variant::Tag("First".to_owned()),
                    Variant::Tag("Second".to_owned()),
                ]
                .into()
            )
        );
    }

    #[test]
    fn repeated_nested_projections_share_the_same_unresolved_leaf() {
        let mut builder = ComponentProgramBuilder::new();
        let provider = builder.new_contextual_hole();
        let requirements = builder.new_contextual_hole();
        let store = builder.terms_mut().intern_name("store");
        let elements = builder.terms_mut().intern_name("elements");
        let distinct_first = builder.terms_mut().intern_name("distinct_first");
        let repeated = builder.terms_mut().intern_name("repeated");
        let distinct_third = builder.terms_mut().intern_name("distinct_third");
        let path = [store, elements, repeated];
        let first_path = [store, elements, distinct_first];
        let third_path = [store, elements, distinct_third];
        let _first_provider = builder.add_projection(provider, first_path);
        let _first_requirement =
            requirement_test_projection(&mut builder, requirements, &first_path);
        let first_provider = builder.add_projection(provider, path);
        let first_requirement = requirement_test_projection(&mut builder, requirements, &path);
        let first = builder.new_variable();
        let first_term = builder.variable_term(first);
        let first_provider = builder.variable_term(first_provider);
        builder.add_unify(first_term, first_provider);
        let first_term = builder.variable_term(first);
        let first_requirement = builder.variable_term(first_requirement);
        builder.add_unify(first_term, first_requirement);
        let _third_provider = builder.add_projection(provider, third_path);
        let _third_requirement =
            requirement_test_projection(&mut builder, requirements, &third_path);
        let second_provider = builder.add_projection(provider, path);
        let second_requirement = requirement_test_projection(&mut builder, requirements, &path);
        let second = builder.new_variable();
        let second_term = builder.variable_term(second);
        let second_provider = builder.variable_term(second_provider);
        builder.add_unify(second_term, second_provider);
        let second_term = builder.variable_term(second);
        let second_requirement = builder.variable_term(second_requirement);
        builder.add_unify(second_term, second_requirement);
        let first_output = builder.add_output(first, FlowMode::Continuous);
        let second_output = builder.add_output(second, FlowMode::Continuous);
        let record = builder.new_authoritative_provider();
        let first_name = builder.terms_mut().intern_name("first");
        let second_name = builder.terms_mut().intern_name("second");
        let first_term = builder.variable_term(first);
        let second_term = builder.variable_term(second);
        let record_term = builder.terms_mut().object(
            [(first_name, first_term), (second_name, second_term)],
            false,
        );
        builder.add_publish(record, [record_term], PublishMode::Replace);
        let record_output = builder.add_output(record, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        assert_eq!(
            artifact.output(first_output).unwrap().flow_type.ty,
            artifact.output(second_output).unwrap().flow_type.ty,
        );
        let Type::Object(record) = &artifact.output(record_output).unwrap().flow_type.ty else {
            panic!("projection record must remain an object")
        };
        assert_eq!(record.fields["first"], record.fields["second"]);
    }

    fn requirement_test_projection(
        builder: &mut ComponentProgramBuilder,
        root: TypeVariableId,
        path: &[crate::NameId],
    ) -> TypeVariableId {
        let mut provider = root;
        for field in path {
            let consumer = builder.new_variable();
            let consumer_term = builder.variable_term(consumer);
            let scaffold = builder.terms_mut().object([(*field, consumer_term)], true);
            let provider_term = builder.variable_term(provider);
            builder.add_unify(provider_term, scaffold);
            provider = consumer;
        }
        provider
    }

    #[test]
    fn missing_authoritative_projection_invalidates_the_occurrence() {
        let mut builder = ComponentProgramBuilder::new();
        let provider = builder.new_authoritative_provider();
        let field = builder.terms_mut().intern_name("missing");
        let empty = builder.terms_mut().object([], false);
        builder.add_publish(provider, [empty], PublishMode::Replace);
        let projected = builder.add_projection(provider, [field]);
        let output = builder.add_output(projected, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        assert!(matches!(
            &artifact.output(output).unwrap().flow_type.ty,
            Type::UnresolvedShape { reason } if reason.contains("missing")
        ));
    }

    #[test]
    fn object_projection_can_shape_an_unresolved_formal() {
        let mut builder = ComponentProgramBuilder::new();
        let formal = builder.new_variable();
        let name = builder.terms_mut().intern_name("name");
        let projected = builder.add_projection(formal, [name]);
        let text = builder.terms().text();
        let projected_term = builder.variable_term(projected);
        builder.add_unify(projected_term, text);
        let output = builder.add_output(formal, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        assert_eq!(
            artifact.output(output).unwrap().flow_type.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [("name".to_owned(), Type::Text)],
                true,
            ))
        );
    }

    #[test]
    fn consumers_of_a_non_root_alias_replay_when_the_equivalence_class_changes() {
        let mut builder = ComponentProgramBuilder::new();
        let provider = builder.new_authoritative_provider();
        let alias = builder.new_variable();
        let occurrence = builder.new_variable();
        builder.add_projection_into(provider, [], occurrence);
        let alias_term = builder.variable_term(alias);
        let provider_term = builder.variable_term(provider);
        builder.add_unify(alias_term, provider_term);
        let ready_variant = builder.terms_mut().variant_tag("Ready");
        let ready = builder.terms_mut().variant_set([ready_variant]);
        let occurrence_term = builder.variable_term(occurrence);
        builder.add_publish(
            provider,
            [ready, occurrence_term],
            PublishMode::StructuralWiden,
        );
        let output = builder.add_output(occurrence, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        assert_eq!(
            artifact.output(output).unwrap().flow_type.ty,
            Type::VariantSet(vec![Variant::Tag("Ready".to_owned())].into()),
            "a consumer indexed under the non-root variable must observe a root mutation"
        );
    }

    #[test]
    fn static_equality_is_installed_before_directional_providers() {
        let mut builder = ComponentProgramBuilder::new();
        let formal = builder.new_variable();
        let actual = builder.new_authoritative_provider();
        let name = builder.terms_mut().intern_name("name");
        let text = builder.terms().text();
        let actual_value = builder.terms_mut().object([(name, text)], false);
        builder.add_publish(actual, [actual_value], PublishMode::Replace);
        let projected = builder.add_projection(formal, [name]);
        let formal_term = builder.variable_term(formal);
        let actual_term = builder.variable_term(actual);
        builder.add_unify(formal_term, actual_term);
        let output = builder.add_output(projected, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        assert_eq!(artifact.output(output).unwrap().flow_type.ty, Type::Text);
        assert_eq!(artifact.work.operations, 3);
        assert_eq!(artifact.work.activations, 3);
        assert_eq!(artifact.work.unify_activations, 1);
    }

    #[test]
    fn competing_authoritative_publications_fail_the_single_writer_invariant() {
        let mut builder = ComponentProgramBuilder::new();
        let output = builder.new_authoritative_provider();
        let text = builder.terms().text();
        let number = builder.terms().number();
        builder.add_publish(output, [text], PublishMode::Replace);
        builder.add_publish(output, [number], PublishMode::Replace);
        let result = builder.add_output(output, FlowMode::Continuous);

        let error = solve_component(builder.finish()).expect_err("publishers must be rejected");
        assert!(error.to_string().contains("multiple directional writers"));
        let _ = result;
    }
}
