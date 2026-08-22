use crate::{TypeTerm, TypeTermArena, TypeTermId, TypeVariableId};
use boon_checked::FlowMode;
use boon_contract::{ProjectTextSnapshot, SymbolId};
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
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
    Field { name: SymbolId, value: TypeTermId },
    Spread { value: TypeTermId },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KernelSummaryValueId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelSummaryRecordEntry {
    Field {
        name: SymbolId,
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
        fields: Box<[SymbolId]>,
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
        tag: Option<SymbolId>,
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
    pub field: Option<SymbolId>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackedRange {
    start: u32,
    len: u32,
}

impl PackedRange {
    fn from_bounds(start: usize, end: usize, label: &str) -> Self {
        let start = u32::try_from(start).unwrap_or_else(|_| panic!("{label} start exceeds u32"));
        let len = u32::try_from(end - start as usize)
            .unwrap_or_else(|_| panic!("{label} length exceeds u32"));
        Self { start, len }
    }

    fn bounds(self) -> std::ops::Range<usize> {
        let start = self.start as usize;
        start..start + self.len as usize
    }
}

/// Fixed-width operation rows plus shared operand columns.
///
/// Construction writes these same columns directly, so a finished component
/// owns no per-operation operand boxes and pays no temporary boxed form. The
/// hot solver reads borrowed slices through `KernelOperationRef`.
#[derive(Clone, Debug)]
pub(crate) struct PackedOperationTable {
    rows: Box<[PackedOperation]>,
    terms: Box<[TypeTermId]>,
    names: Box<[SymbolId]>,
    patterns: Box<[KernelPattern]>,
    select_arms: Box<[KernelSelectArm]>,
    record_entries: Box<[KernelRecordEntry]>,
    summary_programs: Box<[Arc<KernelSummaryProgram>]>,
    summary_inputs: Box<[KernelSummaryCallInput]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PackedOperation {
    Unify {
        left: TypeTermId,
        right: TypeTermId,
    },
    Alias {
        provider: TypeVariableId,
        consumer: TypeVariableId,
    },
    Publish {
        output: TypeVariableId,
        inputs: PackedRange,
        mode: PublishMode,
    },
    Projection {
        provider: TypeVariableId,
        field: Option<SymbolId>,
        consumer: TypeVariableId,
    },
    PatternProjection {
        provider: TypeVariableId,
        pattern: u32,
        fields: PackedRange,
        consumer: TypeVariableId,
    },
    CollectionProjection {
        provider: TypeVariableId,
        kind: KernelCollectionProjectionKind,
        consumer: TypeVariableId,
    },
    Collection {
        output: TypeVariableId,
        kind: KernelCollectionOperationKind,
        inputs: PackedRange,
        values: PackedRange,
    },
    Select {
        output: TypeVariableId,
        selector: TypeVariableId,
        selector_parameter_derived: bool,
        arms: PackedRange,
    },
    Record {
        output: TypeVariableId,
        tag: Option<SymbolId>,
        entries: PackedRange,
    },
    SummaryCall {
        output: TypeVariableId,
        program: u32,
        inputs: PackedRange,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum KernelOperationRef<'a> {
    Unify {
        left: TypeTermId,
        right: TypeTermId,
    },
    Alias {
        provider: TypeVariableId,
        consumer: TypeVariableId,
    },
    Publish {
        output: TypeVariableId,
        inputs: &'a [TypeTermId],
        mode: PublishMode,
    },
    Projection {
        provider: TypeVariableId,
        field: Option<SymbolId>,
        consumer: TypeVariableId,
    },
    PatternProjection {
        provider: TypeVariableId,
        pattern: &'a KernelPattern,
        fields: &'a [SymbolId],
        consumer: TypeVariableId,
    },
    CollectionProjection {
        provider: TypeVariableId,
        kind: KernelCollectionProjectionKind,
        consumer: TypeVariableId,
    },
    Collection {
        output: TypeVariableId,
        kind: KernelCollectionOperationKind,
        inputs: &'a [TypeTermId],
        values: &'a [TypeTermId],
    },
    Select {
        output: TypeVariableId,
        selector: TypeVariableId,
        selector_parameter_derived: bool,
        arms: &'a [KernelSelectArm],
    },
    Record {
        output: TypeVariableId,
        tag: Option<SymbolId>,
        entries: &'a [KernelRecordEntry],
    },
    SummaryCall {
        output: TypeVariableId,
        program: &'a Arc<KernelSummaryProgram>,
        inputs: &'a [KernelSummaryCallInput],
    },
}

#[derive(Debug, Default)]
struct PackedOperationBuilder {
    rows: Vec<PackedOperation>,
    terms: Vec<TypeTermId>,
    names: Vec<SymbolId>,
    patterns: Vec<KernelPattern>,
    select_arms: Vec<KernelSelectArm>,
    record_entries: Vec<KernelRecordEntry>,
    summary_programs: Vec<Arc<KernelSummaryProgram>>,
    summary_inputs: Vec<KernelSummaryCallInput>,
}

impl PackedOperationBuilder {
    fn get(&self, index: usize) -> KernelOperationRef<'_> {
        operation_ref(
            &self.rows,
            &self.terms,
            &self.names,
            &self.patterns,
            &self.select_arms,
            &self.record_entries,
            &self.summary_programs,
            &self.summary_inputs,
            index,
        )
    }

    fn push(&mut self, operation: PackedOperation) -> OperationId {
        let id = OperationId(
            u32::try_from(self.rows.len()).expect("kernel operation count exceeds u32"),
        );
        self.rows.push(operation);
        id
    }

    fn append_terms(&mut self, values: impl IntoIterator<Item = TypeTermId>) -> PackedRange {
        append_column(&mut self.terms, values, "kernel operation term column")
    }

    fn append_names(&mut self, values: impl IntoIterator<Item = SymbolId>) -> PackedRange {
        append_column(&mut self.names, values, "kernel operation name column")
    }

    fn push_pattern(&mut self, pattern: KernelPattern) -> u32 {
        let id =
            u32::try_from(self.patterns.len()).expect("kernel operation pattern count exceeds u32");
        self.patterns.push(pattern);
        id
    }

    fn append_select_arms(
        &mut self,
        arms: impl IntoIterator<Item = KernelSelectArm>,
    ) -> PackedRange {
        append_column(
            &mut self.select_arms,
            arms,
            "kernel operation select-arm column",
        )
    }

    fn append_record_entries(
        &mut self,
        entries: impl IntoIterator<Item = KernelRecordEntry>,
    ) -> PackedRange {
        append_column(
            &mut self.record_entries,
            entries,
            "kernel operation record-entry column",
        )
    }

    fn push_summary_program(&mut self, program: Arc<KernelSummaryProgram>) -> u32 {
        let id = u32::try_from(self.summary_programs.len())
            .expect("kernel summary program count exceeds u32");
        self.summary_programs.push(program);
        id
    }

    fn append_summary_inputs(
        &mut self,
        inputs: impl IntoIterator<Item = KernelSummaryCallInput>,
    ) -> PackedRange {
        append_column(
            &mut self.summary_inputs,
            inputs,
            "kernel summary input column",
        )
    }

    fn finish(self) -> PackedOperationTable {
        PackedOperationTable {
            rows: self.rows.into_boxed_slice(),
            terms: self.terms.into_boxed_slice(),
            names: self.names.into_boxed_slice(),
            patterns: self.patterns.into_boxed_slice(),
            select_arms: self.select_arms.into_boxed_slice(),
            record_entries: self.record_entries.into_boxed_slice(),
            summary_programs: self.summary_programs.into_boxed_slice(),
            summary_inputs: self.summary_inputs.into_boxed_slice(),
        }
    }
}

impl PackedOperationTable {
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn get(&self, index: usize) -> KernelOperationRef<'_> {
        operation_ref(
            &self.rows,
            &self.terms,
            &self.names,
            &self.patterns,
            &self.select_arms,
            &self.record_entries,
            &self.summary_programs,
            &self.summary_inputs,
            index,
        )
    }

    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = KernelOperationRef<'_>> + '_ {
        (0..self.len()).map(|index| self.get(index))
    }
}

#[allow(clippy::too_many_arguments)]
fn operation_ref<'a>(
    rows: &[PackedOperation],
    terms: &'a [TypeTermId],
    names: &'a [SymbolId],
    patterns: &'a [KernelPattern],
    select_arms: &'a [KernelSelectArm],
    record_entries: &'a [KernelRecordEntry],
    summary_programs: &'a [Arc<KernelSummaryProgram>],
    summary_inputs: &'a [KernelSummaryCallInput],
    index: usize,
) -> KernelOperationRef<'a> {
    let row = *rows.get(index).expect("kernel operation index is in range");
    match row {
        PackedOperation::Unify { left, right } => KernelOperationRef::Unify { left, right },
        PackedOperation::Alias { provider, consumer } => {
            KernelOperationRef::Alias { provider, consumer }
        }
        PackedOperation::Publish {
            output,
            inputs,
            mode,
        } => KernelOperationRef::Publish {
            output,
            inputs: &terms[inputs.bounds()],
            mode,
        },
        PackedOperation::Projection {
            provider,
            field,
            consumer,
        } => KernelOperationRef::Projection {
            provider,
            field,
            consumer,
        },
        PackedOperation::PatternProjection {
            provider,
            pattern,
            fields,
            consumer,
        } => KernelOperationRef::PatternProjection {
            provider,
            pattern: &patterns[pattern as usize],
            fields: &names[fields.bounds()],
            consumer,
        },
        PackedOperation::CollectionProjection {
            provider,
            kind,
            consumer,
        } => KernelOperationRef::CollectionProjection {
            provider,
            kind,
            consumer,
        },
        PackedOperation::Collection {
            output,
            kind,
            inputs,
            values,
        } => KernelOperationRef::Collection {
            output,
            kind,
            inputs: &terms[inputs.bounds()],
            values: &terms[values.bounds()],
        },
        PackedOperation::Select {
            output,
            selector,
            selector_parameter_derived,
            arms,
        } => KernelOperationRef::Select {
            output,
            selector,
            selector_parameter_derived,
            arms: &select_arms[arms.bounds()],
        },
        PackedOperation::Record {
            output,
            tag,
            entries,
        } => KernelOperationRef::Record {
            output,
            tag,
            entries: &record_entries[entries.bounds()],
        },
        PackedOperation::SummaryCall {
            output,
            program,
            inputs,
        } => KernelOperationRef::SummaryCall {
            output,
            program: &summary_programs[program as usize],
            inputs: &summary_inputs[inputs.bounds()],
        },
    }
}

fn append_column<T>(
    column: &mut Vec<T>,
    values: impl IntoIterator<Item = T>,
    label: &str,
) -> PackedRange {
    let start = column.len();
    column.extend(values);
    PackedRange::from_bounds(start, column.len(), label)
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
    pub(crate) operations: PackedOperationTable,
    pub(crate) residual_frames: Box<[ResidualOperationFrame]>,
    pub(crate) work_items: Box<[ProgramOperationRef]>,
    pub(crate) instruction_count: u64,
    pub(crate) initial_order: Box<[OperationId]>,
    pub(crate) acyclic_initial_operations: u64,
    pub(crate) dependency_offsets: Box<[u32]>,
    pub(crate) dependencies: Box<[TypeVariableId]>,
    /// Dense variables written by each scheduled work item. A coarse acyclic
    /// residual frame may write many variables while still occupying one
    /// scheduler slot, so this cannot be reconstructed from the operation
    /// enum alone after linking.
    pub(crate) output_offsets: Box<[u32]>,
    pub(crate) operation_outputs: Box<[TypeVariableId]>,
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

#[derive(Debug)]
pub struct ComponentProgramBuilder {
    terms: TypeTermArena,
    variables: Vec<VariableSpec>,
    operations: PackedOperationBuilder,
    residual_frames: Vec<ResidualOperationFrame>,
    work_order: Vec<BuilderWorkItem>,
    outputs: Vec<ProgramOutput>,
}

impl ComponentProgramBuilder {
    pub fn with_text(text: ProjectTextSnapshot) -> Self {
        Self {
            terms: TypeTermArena::with_text(text),
            variables: Vec::new(),
            operations: PackedOperationBuilder::default(),
            residual_frames: Vec::new(),
            work_order: Vec::new(),
            outputs: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn new() -> Self {
        let terms = TypeTermArena::for_test_symbols([
            "Initial",
            "Updated",
            "True",
            "False",
            "value",
            "kind",
            "Header",
            "Empty",
            "KeyA",
            "KeyB",
            "ValueA",
            "ValueB",
            "family",
            "size",
            "color",
            "NotARecord",
            "WorkspaceA",
            "WorkspaceB",
            "Repo",
            "previous",
            "current",
            "a",
            "b",
            "First",
            "Second",
            "store",
            "elements",
            "distinct_first",
            "repeated",
            "distinct_third",
            "first",
            "second",
            "missing",
            "present",
            "name",
            "state",
            "click_time",
            "segments",
            "fixed",
            "label",
            "Ready",
            "Dark",
        ]);
        Self {
            terms,
            variables: Vec::new(),
            operations: PackedOperationBuilder::default(),
            residual_frames: Vec::new(),
            work_order: Vec::new(),
            outputs: Vec::new(),
        }
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
        self.push_operation(PackedOperation::Unify { left, right })
    }

    pub fn add_alias(&mut self, provider: TypeVariableId, consumer: TypeVariableId) -> OperationId {
        self.push_operation(PackedOperation::Alias { provider, consumer })
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
        let inputs = self.operations.append_terms(inputs);
        self.push_operation(PackedOperation::Publish {
            output,
            inputs,
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
        path: impl IntoIterator<Item = SymbolId>,
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
        path: impl IntoIterator<Item = SymbolId>,
        consumer: TypeVariableId,
    ) {
        let mut path = path.into_iter().peekable();
        let Some(first) = path.next() else {
            self.push_operation(PackedOperation::Projection {
                provider,
                field: None,
                consumer,
            });
            return;
        };
        let mut provider = provider;
        let mut field = first;
        loop {
            let next = if path.peek().is_none() {
                consumer
            } else {
                self.new_variable()
            };
            self.push_operation(PackedOperation::Projection {
                provider,
                field: Some(field),
                consumer: next,
            });
            provider = next;
            let Some(next_field) = path.next() else {
                break;
            };
            field = next_field;
        }
    }

    pub fn add_pattern_projection_into(
        &mut self,
        provider: TypeVariableId,
        pattern: KernelPattern,
        fields: impl IntoIterator<Item = SymbolId>,
        consumer: TypeVariableId,
    ) -> OperationId {
        let pattern = self.operations.push_pattern(pattern);
        let fields = self.operations.append_names(fields);
        self.push_operation(PackedOperation::PatternProjection {
            provider,
            pattern,
            fields,
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
        let arms = self.operations.append_select_arms(arms);
        self.push_operation(PackedOperation::Select {
            output,
            selector,
            selector_parameter_derived,
            arms,
        })
    }

    pub fn add_collection_item_projection(
        &mut self,
        provider: TypeVariableId,
        consumer: TypeVariableId,
    ) -> OperationId {
        self.mark_authoritative(consumer);
        self.push_operation(PackedOperation::CollectionProjection {
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
        self.push_operation(PackedOperation::CollectionProjection {
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
        self.push_operation(PackedOperation::CollectionProjection {
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
        let inputs = self.operations.append_terms(inputs);
        let values = self.operations.append_terms(values);
        self.push_operation(PackedOperation::Collection {
            output,
            kind,
            inputs,
            values,
        })
    }

    pub fn add_record(
        &mut self,
        output: TypeVariableId,
        tag: Option<SymbolId>,
        entries: impl IntoIterator<Item = KernelRecordEntry>,
    ) -> OperationId {
        self.mark_authoritative(output);
        let entries = self.operations.append_record_entries(entries);
        self.push_operation(PackedOperation::Record {
            output,
            tag,
            entries,
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
        let program = self.operations.push_summary_program(program);
        let inputs = self
            .operations
            .append_summary_inputs(inputs.into_iter().map(Into::into));
        self.push_operation(PackedOperation::SummaryCall {
            output,
            program,
            inputs,
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
        });
        self.work_order.push(BuilderWorkItem::Residual(frame));
        frame
    }

    pub fn finish(mut self) -> ComponentProgram {
        // Link module-local immutable terms once. Symbols are already
        // coordinates in the single project text authority, so residual
        // frames need no parallel name-remapping allocation.
        let target_terms = &mut self.terms;
        for frame in &mut self.residual_frames {
            assert!(
                target_terms
                    .text_snapshot()
                    .same_authority(frame.module.terms.text_snapshot()),
                "residual modules must share the owning component text authority"
            );
            let mut term_cache = vec![None; frame.module.terms.len()];
            for operation in frame.module.operations.iter() {
                link_residual_operation_terms(
                    operation,
                    &frame.module.terms,
                    target_terms,
                    &frame.variables,
                    &mut term_cache,
                );
            }
            frame.terms = term_cache.into();
        }

        // Size the topology first, then write each final column exactly once.
        // The previous Vec<BTreeSet<_>> reverse index allocated one tree node
        // per edge, one Arc per operation, and one boxed output slice per work
        // item. Dense IDs already give us deterministic sort/dedup order, so
        // none of those pointer-owning staging structures are needed.
        let mut work_item_count = 0_usize;
        let mut dependency_count = 0_usize;
        let mut output_count = 0_usize;
        let mut instruction_count = 0_u64;
        let mut consumer_counts = vec![0_u32; self.variables.len()];
        visit_program_topology(
            &self.work_order,
            &self.operations,
            &self.residual_frames,
            &self.terms,
            |_, instructions, dependencies, outputs| {
                work_item_count = work_item_count
                    .checked_add(1)
                    .expect("kernel work-item count exceeds usize");
                dependency_count = dependency_count
                    .checked_add(dependencies.len())
                    .expect("kernel dependency edge count exceeds usize");
                output_count = output_count
                    .checked_add(outputs.len())
                    .expect("kernel operation-output edge count exceeds usize");
                instruction_count = instruction_count.saturating_add(instructions);
                for dependency in dependencies {
                    let count = consumer_counts
                        .get_mut(dependency.0 as usize)
                        .expect("kernel operation references an undeclared variable");
                    *count = count
                        .checked_add(1)
                        .expect("kernel variable consumer count exceeds u32");
                }
            },
        );
        u32::try_from(work_item_count).expect("kernel operation count exceeds u32");
        u32::try_from(dependency_count).expect("kernel dependency edge count exceeds u32");
        u32::try_from(output_count).expect("kernel operation-output edge count exceeds u32");

        let mut work_items = Vec::with_capacity(work_item_count);
        let mut dependency_offsets = Vec::with_capacity(work_item_count + 1);
        let mut forward_dependencies = Vec::with_capacity(dependency_count);
        let mut output_offsets = Vec::with_capacity(work_item_count + 1);
        let mut flat_operation_outputs = Vec::with_capacity(output_count);
        dependency_offsets.push(0);
        output_offsets.push(0);
        visit_program_topology(
            &self.work_order,
            &self.operations,
            &self.residual_frames,
            &self.terms,
            |reference, _, dependencies, outputs| {
                work_items.push(reference);
                forward_dependencies.extend_from_slice(dependencies);
                dependency_offsets.push(
                    u32::try_from(forward_dependencies.len())
                        .expect("kernel dependency edge count exceeds u32"),
                );
                flat_operation_outputs.extend_from_slice(outputs);
                output_offsets.push(
                    u32::try_from(flat_operation_outputs.len())
                        .expect("kernel operation-output edge count exceeds u32"),
                );
            },
        );
        debug_assert_eq!(work_items.len(), work_item_count);
        debug_assert_eq!(forward_dependencies.len(), dependency_count);
        debug_assert_eq!(flat_operation_outputs.len(), output_count);

        let mut consumer_offsets = Vec::with_capacity(consumer_counts.len() + 1);
        consumer_offsets.push(0_u32);
        for count in consumer_counts {
            let next = consumer_offsets
                .last()
                .copied()
                .expect("kernel consumer offsets contain zero")
                .checked_add(count)
                .expect("kernel consumer edge count exceeds u32");
            consumer_offsets.push(next);
        }
        debug_assert_eq!(
            consumer_offsets.last().copied().unwrap_or(0) as usize,
            dependency_count,
        );
        let mut consumer_cursors = consumer_offsets[..self.variables.len()].to_vec();
        let mut consumers = vec![
            ProgramConsumer {
                operation: OperationId(0),
            };
            dependency_count
        ];
        for operation in 0..work_items.len() {
            let operation_id =
                OperationId(u32::try_from(operation).expect("kernel operation count exceeds u32"));
            let start = dependency_offsets[operation] as usize;
            let end = dependency_offsets[operation + 1] as usize;
            for dependency in &forward_dependencies[start..end] {
                let cursor = &mut consumer_cursors[dependency.0 as usize];
                consumers[*cursor as usize] = ProgramConsumer {
                    operation: operation_id,
                };
                *cursor = cursor
                    .checked_add(1)
                    .expect("kernel consumer cursor exceeds u32");
            }
        }
        let (initial_order, acyclic_initial_operations) = initial_operation_order(
            self.variables.len(),
            &output_offsets,
            &flat_operation_outputs,
            &dependency_offsets,
            &forward_dependencies,
        );
        // Residual modules are retained behind `Arc<ComponentProgram>`. Their
        // construction scratch is not executable state and must not multiply
        // retained capacities by the module count. The main solver starts a
        // fresh, separately measured scratch phase after this boundary too.
        self.terms.clear_scratch_storage();
        ComponentProgram {
            terms: self.terms,
            variables: self.variables.into_boxed_slice(),
            operations: self.operations.finish(),
            residual_frames: self.residual_frames.into_boxed_slice(),
            work_items: work_items.into_boxed_slice(),
            instruction_count,
            initial_order: initial_order.into_boxed_slice(),
            acyclic_initial_operations,
            dependency_offsets: dependency_offsets.into_boxed_slice(),
            dependencies: forward_dependencies.into_boxed_slice(),
            output_offsets: output_offsets.into_boxed_slice(),
            operation_outputs: flat_operation_outputs.into_boxed_slice(),
            consumer_offsets: consumer_offsets.into_boxed_slice(),
            consumers: consumers.into_boxed_slice(),
            outputs: self.outputs.into_boxed_slice(),
        }
    }

    fn push_operation(&mut self, operation: PackedOperation) -> OperationId {
        let id = self.operations.push(operation);
        self.work_order.push(BuilderWorkItem::Direct(id.0));
        id
    }
}

fn visit_program_topology(
    work_order: &[BuilderWorkItem],
    operations: &PackedOperationBuilder,
    residual_frames: &[ResidualOperationFrame],
    terms: &TypeTermArena,
    mut visit: impl FnMut(ProgramOperationRef, u64, &[TypeVariableId], &[TypeVariableId]),
) {
    let mut dependencies = Vec::<TypeVariableId>::new();
    let mut outputs = Vec::<TypeVariableId>::new();
    let mut publish = |reference: ProgramOperationRef,
                       instruction_count: u64,
                       dependencies: &mut Vec<TypeVariableId>,
                       outputs: &mut Vec<TypeVariableId>| {
        dependencies.sort_unstable();
        dependencies.dedup();
        outputs.sort_unstable();
        outputs.dedup();
        visit(reference, instruction_count, dependencies, outputs);
        dependencies.clear();
        outputs.clear();
    };

    for item in work_order {
        match *item {
            BuilderWorkItem::Direct(index) => {
                let operation = operations.get(index as usize);
                if let Some(output) = operation_ref_output(operation, None) {
                    outputs.push(output);
                }
                collect_operation_variables(operation, terms, &mut dependencies);
                publish(
                    ProgramOperationRef::Direct(index),
                    1,
                    &mut dependencies,
                    &mut outputs,
                );
            }
            BuilderWorkItem::Residual(frame_index) => {
                let frame = &residual_frames[frame_index as usize];
                let fully_acyclic = frame.module.acyclic_initial_operation_count()
                    == frame.module.operation_count() as u64;
                if fully_acyclic {
                    for operation in frame.module.operations.iter() {
                        if let Some(output) =
                            operation_ref_output(operation, Some(&frame.variables))
                        {
                            outputs.push(output);
                        }
                    }
                    for operation_index in 0..frame.module.operations.len() {
                        let module_operation = OperationId(
                            u32::try_from(operation_index)
                                .expect("kernel residual operation count exceeds u32"),
                        );
                        dependencies.extend(
                            frame
                                .module
                                .operation_dependencies(module_operation)
                                .iter()
                                .map(|dependency| frame.variables[dependency.0 as usize]),
                        );
                    }
                    dependencies.sort_unstable();
                    dependencies.dedup();
                    outputs.sort_unstable();
                    outputs.dedup();
                    // The module's authored topological order owns every
                    // value it writes internally. Only formal/imported roots
                    // are frame-level subscriptions.
                    dependencies.retain(|dependency| outputs.binary_search(dependency).is_err());
                    publish(
                        ProgramOperationRef::ResidualFrame { frame: frame_index },
                        frame.module.operation_count() as u64,
                        &mut dependencies,
                        &mut outputs,
                    );
                } else {
                    // Cyclic residual modules retain instruction-grained
                    // scheduling. They are the small exceptional tail.
                    for operation_index in 0..frame.module.operations.len() {
                        let operation = frame.module.operations.get(operation_index);
                        if let Some(output) =
                            operation_ref_output(operation, Some(&frame.variables))
                        {
                            outputs.push(output);
                        }
                        let module_operation = OperationId(
                            u32::try_from(operation_index)
                                .expect("kernel residual operation count exceeds u32"),
                        );
                        dependencies.extend(
                            frame
                                .module
                                .operation_dependencies(module_operation)
                                .iter()
                                .map(|dependency| frame.variables[dependency.0 as usize]),
                        );
                        publish(
                            ProgramOperationRef::Residual {
                                frame: frame_index,
                                operation: module_operation.0,
                            },
                            1,
                            &mut dependencies,
                            &mut outputs,
                        );
                    }
                }
            }
        }
    }
}

pub(crate) fn operation_ref_output(
    operation: KernelOperationRef<'_>,
    variables: Option<&[TypeVariableId]>,
) -> Option<TypeVariableId> {
    let output = match operation {
        KernelOperationRef::Publish { output, .. }
        | KernelOperationRef::Select { output, .. }
        | KernelOperationRef::Record { output, .. }
        | KernelOperationRef::Collection { output, .. }
        | KernelOperationRef::SummaryCall { output, .. } => Some(output),
        KernelOperationRef::Alias { consumer, .. }
        | KernelOperationRef::Projection { consumer, .. }
        | KernelOperationRef::PatternProjection { consumer, .. }
        | KernelOperationRef::CollectionProjection { consumer, .. } => Some(consumer),
        KernelOperationRef::Unify { .. } => None,
    }?;
    Some(variables.map_or(output, |variables| variables[output.0 as usize]))
}

fn initial_operation_order(
    variable_count: usize,
    output_offsets: &[u32],
    outputs: &[TypeVariableId],
    dependency_offsets: &[u32],
    dependencies: &[TypeVariableId],
) -> (Vec<OperationId>, u64) {
    let operation_count = output_offsets.len().saturating_sub(1);
    let mut writers = vec![None::<usize>; variable_count];
    for operation in 0..operation_count {
        let start = output_offsets[operation] as usize;
        let end = output_offsets[operation + 1] as usize;
        for output in &outputs[start..end] {
            let writer = &mut writers[output.0 as usize];
            if writer.is_none() {
                *writer = Some(operation);
            }
        }
    }
    let mut outgoing_counts = vec![0_u32; operation_count];
    let mut indegree = vec![0_u32; operation_count];
    let mut dependency_writers = Vec::<usize>::new();
    for consumer in 0..operation_count {
        let start = dependency_offsets[consumer] as usize;
        let end = dependency_offsets[consumer + 1] as usize;
        for dependency in &dependencies[start..end] {
            let Some(writer) = writers[dependency.0 as usize] else {
                continue;
            };
            if writer == consumer {
                continue;
            }
            dependency_writers.push(writer);
        }
        dependency_writers.sort_unstable();
        dependency_writers.dedup();
        for writer in dependency_writers.drain(..) {
            outgoing_counts[writer] = outgoing_counts[writer]
                .checked_add(1)
                .expect("kernel operation edge count exceeds u32");
            indegree[consumer] = indegree[consumer].saturating_add(1);
        }
    }
    let mut outgoing_offsets = Vec::with_capacity(operation_count + 1);
    outgoing_offsets.push(0_u32);
    for count in outgoing_counts {
        let next = outgoing_offsets
            .last()
            .copied()
            .unwrap_or(0)
            .checked_add(count)
            .expect("kernel operation edge count exceeds u32");
        outgoing_offsets.push(next);
    }
    let mut outgoing_cursors = outgoing_offsets[..operation_count].to_vec();
    let mut outgoing = vec![0_u32; outgoing_offsets.last().copied().unwrap_or(0) as usize];
    for consumer in 0..operation_count {
        let start = dependency_offsets[consumer] as usize;
        let end = dependency_offsets[consumer + 1] as usize;
        for dependency in &dependencies[start..end] {
            let Some(writer) = writers[dependency.0 as usize] else {
                continue;
            };
            if writer == consumer {
                continue;
            }
            dependency_writers.push(writer);
        }
        dependency_writers.sort_unstable();
        dependency_writers.dedup();
        for writer in dependency_writers.drain(..) {
            let cursor = &mut outgoing_cursors[writer];
            outgoing[*cursor as usize] =
                u32::try_from(consumer).expect("kernel operation count exceeds u32");
            *cursor = cursor
                .checked_add(1)
                .expect("kernel operation edge cursor exceeds u32");
        }
    }
    let mut ready = BinaryHeap::with_capacity(operation_count);
    ready.extend(
        indegree
            .iter()
            .enumerate()
            .filter_map(|(operation, indegree)| (*indegree == 0).then_some(Reverse(operation))),
    );
    let mut ordered = Vec::with_capacity(operation_count);
    let mut emitted = vec![false; operation_count];
    while let Some(Reverse(operation)) = ready.pop() {
        if emitted[operation] {
            continue;
        }
        emitted[operation] = true;
        ordered.push(OperationId(
            u32::try_from(operation).expect("kernel operation count exceeds u32"),
        ));
        let start = outgoing_offsets[operation] as usize;
        let end = outgoing_offsets[operation + 1] as usize;
        for consumer in &outgoing[start..end] {
            let consumer = *consumer as usize;
            indegree[consumer] -= 1;
            if indegree[consumer] == 0 {
                ready.push(Reverse(consumer));
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
    operation: KernelOperationRef<'_>,
    source: &TypeTermArena,
    target: &mut TypeTermArena,
    variables: &[TypeVariableId],
    term_cache: &mut [Option<TypeTermId>],
) {
    match operation {
        KernelOperationRef::Unify { left, right } => {
            link_residual_term(left, source, target, variables, term_cache);
            link_residual_term(right, source, target, variables, term_cache);
        }
        KernelOperationRef::Alias { .. } => {}
        KernelOperationRef::Publish { inputs, .. } => {
            for input in inputs {
                link_residual_term(*input, source, target, variables, term_cache);
            }
        }
        KernelOperationRef::Projection { .. } | KernelOperationRef::PatternProjection { .. } => {}
        KernelOperationRef::CollectionProjection { .. } => {}
        KernelOperationRef::Collection { inputs, values, .. } => {
            for input in inputs.iter().chain(values.iter()) {
                link_residual_term(*input, source, target, variables, term_cache);
            }
        }
        KernelOperationRef::Select { arms, .. } => {
            for arm in arms {
                link_residual_term(arm.output, source, target, variables, term_cache);
            }
        }
        KernelOperationRef::Record { entries, .. } => {
            for entry in entries {
                match entry {
                    KernelRecordEntry::Field { value, .. }
                    | KernelRecordEntry::Spread { value } => {
                        link_residual_term(*value, source, target, variables, term_cache);
                    }
                }
            }
        }
        KernelOperationRef::SummaryCall { .. } => {
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
) {
    target.import_rebased_term(source, term, variables, term_cache);
}

fn collect_operation_variables(
    operation: KernelOperationRef<'_>,
    terms: &TypeTermArena,
    output: &mut Vec<TypeVariableId>,
) {
    match operation {
        KernelOperationRef::Unify { left, right } => {
            collect_term_variables(left, terms, output);
            collect_term_variables(right, terms, output);
        }
        KernelOperationRef::Alias { provider, consumer } => {
            output.push(provider);
            output.push(consumer);
        }
        KernelOperationRef::Publish {
            output: variable,
            inputs,
            ..
        } => {
            output.push(variable);
            for input in inputs {
                collect_term_variables(*input, terms, output);
            }
        }
        KernelOperationRef::Projection {
            provider, consumer, ..
        } => {
            output.push(provider);
            output.push(consumer);
        }
        KernelOperationRef::PatternProjection {
            provider, consumer, ..
        } => {
            output.push(provider);
            output.push(consumer);
        }
        KernelOperationRef::CollectionProjection {
            provider, consumer, ..
        } => {
            output.push(provider);
            output.push(consumer);
        }
        KernelOperationRef::Collection {
            output: variable,
            inputs,
            values,
            ..
        } => {
            output.push(variable);
            for input in inputs.iter().chain(values.iter()) {
                collect_term_variables(*input, terms, output);
            }
        }
        KernelOperationRef::Select {
            output: variable,
            selector,
            arms,
            ..
        } => {
            output.push(variable);
            output.push(selector);
            for arm in arms {
                collect_term_variables(arm.output, terms, output);
            }
        }
        KernelOperationRef::Record {
            output: variable,
            entries,
            ..
        } => {
            output.push(variable);
            for entry in entries {
                let value = match entry {
                    KernelRecordEntry::Field { value, .. }
                    | KernelRecordEntry::Spread { value } => *value,
                };
                collect_term_variables(value, terms, output);
            }
        }
        KernelOperationRef::SummaryCall {
            output: variable,
            inputs,
            ..
        } => {
            output.push(variable);
            for input in inputs {
                match input {
                    KernelSummaryCallInput::Term(term) => {
                        collect_term_variables(*term, terms, output);
                    }
                    KernelSummaryCallInput::Projection { provider, .. } => {
                        output.push(*provider);
                    }
                }
            }
        }
    }
}

pub(crate) fn collect_term_variables(
    term: TypeTermId,
    terms: &TypeTermArena,
    output: &mut Vec<TypeVariableId>,
) {
    match terms.term(term) {
        TypeTerm::Variable(variable) => {
            output.push(variable);
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
            collect_term_variables(item, terms, output);
        }
        TypeTerm::Function { args, result, .. } => {
            for argument in args {
                collect_term_variables(*argument, terms, output);
            }
            collect_term_variables(result, terms, output);
        }
        TypeTerm::Union(members) => {
            for member in members {
                collect_term_variables(*member, terms, output);
            }
        }
        TypeTerm::Map { key, value } => {
            collect_term_variables(key, terms, output);
            collect_term_variables(value, terms, output);
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
    fn finished_program_drops_phase_local_type_scratch() {
        let mut builder = ComponentProgramBuilder::new();
        let value = builder.terms_mut().intern_name("value");
        let number = builder.terms().number();
        let _record = builder.terms_mut().object([(value, number)], false);
        assert!(builder.terms().work().scratch_retained_capacity_bytes > 0);

        let program = builder.finish();
        assert_eq!(program.terms().work().scratch_retained_capacity_bytes, 0);
    }

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
        let text = module.terms().text_snapshot().clone();

        let mut builder = ComponentProgramBuilder::with_text(text);
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

    #[test]
    fn initial_order_prefers_lowest_ready_id_and_keeps_an_ascending_cycle_tail() {
        let mut builder = ComponentProgramBuilder::new();
        let independent = builder.new_variable();
        let cycle_left = builder.new_variable();
        let cycle_right = builder.new_variable();
        let cycle_left_term = builder.variable_term(cycle_left);
        let cycle_right_term = builder.variable_term(cycle_right);
        let text = builder.terms().text();
        builder.add_publish(independent, [text], PublishMode::Replace);
        builder.add_publish(cycle_left, [cycle_right_term], PublishMode::Replace);
        builder.add_publish(cycle_right, [cycle_left_term], PublishMode::Replace);
        let program = builder.finish();

        assert_eq!(
            program.initial_order.as_ref(),
            [OperationId(0), OperationId(1), OperationId(2)]
        );
        assert_eq!(program.acyclic_initial_operation_count(), 1);
    }

    #[test]
    fn packed_topology_columns_are_byte_for_byte_deterministic() {
        let build = || {
            let mut builder = ComponentProgramBuilder::new();
            let left = builder.new_variable();
            let right = builder.new_variable();
            let output = builder.new_variable();
            let left_term = builder.variable_term(left);
            let right_term = builder.variable_term(right);
            let text = builder.terms().text();
            let number = builder.terms().number();
            builder.add_publish(left, [text], PublishMode::Replace);
            builder.add_publish(right, [number], PublishMode::Replace);
            builder.add_publish(
                output,
                [left_term, right_term, left_term],
                PublishMode::Union,
            );
            builder.finish()
        };
        let left = build();
        let right = build();

        assert_eq!(left.work_items, right.work_items);
        assert_eq!(left.initial_order, right.initial_order);
        assert_eq!(left.dependency_offsets, right.dependency_offsets);
        assert_eq!(left.dependencies, right.dependencies);
        assert_eq!(left.output_offsets, right.output_offsets);
        assert_eq!(left.operation_outputs, right.operation_outputs);
        assert_eq!(left.consumer_offsets, right.consumer_offsets);
        assert_eq!(left.consumers, right.consumers);
    }
}
