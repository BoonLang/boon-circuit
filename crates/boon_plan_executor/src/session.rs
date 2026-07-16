use boon_plan::{
    EffectInvocationId, FieldId, InitialValueKind, ListId, ListInitializerKind, ListStorageSlot,
    MachinePlan, PlanConstantId, PlanConstantValue, PlanDerivedExpression, PlanDerivedKind,
    PlanExpressionKind, PlanListOperationKind, PlanListProjection, PlanListRemovePredicate, PlanOp,
    PlanOpId, PlanOpKind, PlanRowCallArg, PlanRowExpression, PlanRowSelectPattern, PlanSourceGuard,
    RootOutputDemand, ScalarStorageSlot, ScopeId, SourceId, SourcePayloadField, SourceRoute,
    StateId, ValueRef,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Value {
    Null,
    Bool(bool),
    Number(i64),
    Text(String),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
    MappedRow {
        id: RowId,
        fields: BTreeMap<String, Value>,
    },
    Row {
        id: RowId,
        fields: BTreeMap<FieldId, Value>,
    },
    Error {
        code: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RowId {
    pub list: ListId,
    pub key: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourcePayload {
    pub text: Option<String>,
    pub key: Option<String>,
    pub address: Option<String>,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceEvent {
    pub sequence: u64,
    pub source: SourceId,
    pub target: Option<RowId>,
    pub payload: SourcePayload,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ValueTarget {
    State(StateId),
    Field(FieldId),
    RowField { row: RowId, field: FieldId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowSnapshot {
    pub id: RowId,
    pub fields: BTreeMap<FieldId, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Delta {
    SetValue {
        target: ValueTarget,
        value: Value,
    },
    InsertRow {
        row: RowSnapshot,
    },
    RemoveRow {
        row: RowId,
    },
    BindSource {
        row: RowId,
        source: SourceId,
        binding_id: u64,
    },
    UnbindSource {
        row: RowId,
        source: SourceId,
        binding_id: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthorityDelta {
    SetRoot {
        state: StateId,
        value: Value,
    },
    SetRowField {
        row: RowId,
        field: FieldId,
        value: Value,
    },
    ReplaceList {
        list_id: ListId,
        authority: ListAuthority,
    },
    InsertRow {
        row: RowAuthority,
        index: u64,
        next_key: u64,
    },
    RemoveRow {
        row: RowId,
        next_key: u64,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TurnMetrics {
    pub dirty_state_count: usize,
    pub dirty_field_count: usize,
    pub recomputed_field_count: usize,
    pub changed_row_count: usize,
    pub dependency_fanout_count: usize,
    pub index_lookup_count: usize,
    pub index_candidate_count: usize,
    pub list_find_scan_count: usize,
    pub work_unit_count: u64,
    pub recomputed_targets: Vec<ValueTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Turn {
    pub sequence: u64,
    pub source_sequence: Option<u64>,
    pub deltas: Vec<Delta>,
    pub authority_deltas: Vec<AuthorityDelta>,
    pub durable_changes: Vec<boon_persistence::DurableChange>,
    pub outbox_changes: Vec<boon_persistence::DurableOutboxChange>,
    pub metrics: TurnMetrics,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub states: BTreeMap<StateId, Value>,
    pub fields: BTreeMap<FieldId, Value>,
    pub lists: BTreeMap<ListId, Vec<RowSnapshot>>,
}

/// Authoritative scalar state as distinct from a derived inspection value.
///
/// `touched` is persisted even when `value` equals the current program default.
/// This prevents a later default change from overwriting an explicit user choice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScalarAuthority {
    pub touched: bool,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowAuthority {
    pub id: RowId,
    pub fields: BTreeMap<FieldId, Value>,
    pub touched_fields: BTreeSet<FieldId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListAuthority {
    pub touched: bool,
    pub next_key: u64,
    pub rows: Vec<RowAuthority>,
}

/// Runtime-ID authority image used at the Session boundary.
///
/// Durable storage translates the runtime IDs through `MachinePlan::persistence`;
/// derived values, indexes, source bindings, and currentness caches never enter
/// this image.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthoritySnapshot {
    pub through_turn_sequence: u64,
    pub states: BTreeMap<StateId, ScalarAuthority>,
    pub lists: BTreeMap<ListId, ListAuthority>,
}

impl Snapshot {
    pub fn value(&self, target: ValueTarget) -> Option<&Value> {
        match target {
            ValueTarget::State(state) => self.states.get(&state),
            ValueTarget::Field(field) => self.fields.get(&field),
            ValueTarget::RowField { row, field } => self
                .lists
                .get(&row.list)?
                .iter()
                .find(|candidate| candidate.id == row)?
                .fields
                .get(&field),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionOptions {
    pub require_monotonic_sequences: bool,
    /// Deterministic executor work allowed for one startup, source turn, or
    /// host-owned currentness transaction. Trusted applications leave this
    /// unbounded; capability hosts set it for restricted programs.
    pub max_work_units_per_transaction: Option<u64>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            require_monotonic_sequences: true,
            max_work_units_per_transaction: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidPlan(String),
    InvalidEvent(String),
    Unsupported { op: PlanOpId, detail: String },
    Cycle { field: FieldId, row: Option<RowId> },
    WorkBudgetExceeded { limit: u64, attempted: u64 },
    Evaluation(String),
    NotDemanded(FieldId),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPlan(detail) => write!(formatter, "invalid MachinePlan: {detail}"),
            Self::InvalidEvent(detail) => write!(formatter, "invalid source event: {detail}"),
            Self::Unsupported { op, detail } => {
                write!(formatter, "unsupported plan op {}: {detail}", op.0)
            }
            Self::Cycle { field, row } => match row {
                Some(row) => write!(
                    formatter,
                    "derived cycle at field {} in row {}:{}:{}",
                    field.0, row.list.0, row.key, row.generation
                ),
                None => write!(formatter, "derived cycle at root field {}", field.0),
            },
            Self::WorkBudgetExceeded { limit, attempted } => write!(
                formatter,
                "executor work budget exceeded: attempted {attempted} units with a {limit}-unit transaction limit"
            ),
            Self::Evaluation(detail) => write!(formatter, "evaluation failed: {detail}"),
            Self::NotDemanded(field) => {
                write!(
                    formatter,
                    "root field {} is not in the demand plan",
                    field.0
                )
            }
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Currentness {
    Current,
    Dirty,
    Evaluating,
}

#[derive(Clone, Debug)]
struct DerivedCell {
    currentness: Currentness,
    value: Option<Value>,
}

impl Default for DerivedCell {
    fn default() -> Self {
        Self {
            currentness: Currentness::Dirty,
            value: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Row {
    fields: BTreeMap<FieldId, Value>,
    derived: BTreeMap<FieldId, Currentness>,
    bindings: BTreeMap<SourceId, u64>,
}

#[derive(Clone, Debug, Default)]
struct ListState {
    rows: BTreeMap<RowId, Row>,
    order: Vec<RowId>,
    next_key: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ScalarKey {
    Null,
    Bool(bool),
    Number(i64),
    Text(String),
    Bytes(Vec<u8>),
}

impl ScalarKey {
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Null => Some(Self::Null),
            Value::Bool(value) => Some(Self::Bool(*value)),
            Value::Number(value) => Some(Self::Number(*value)),
            Value::Text(value) => Some(Self::Text(value.clone())),
            Value::Bytes(value) => Some(Self::Bytes(value.clone())),
            Value::List(_)
            | Value::Record(_)
            | Value::MappedRow { .. }
            | Value::Row { .. }
            | Value::Error { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct IndexKey {
    list: ListId,
    field: FieldId,
    value: ScalarKey,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Consumer {
    Root(FieldId),
    Row(RowId, FieldId),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DynamicDependency {
    RowField(RowId, FieldId),
    Query(IndexKey),
    List(ListId),
}

#[derive(Clone, Debug, Default)]
struct DynamicDependencies {
    by_row_field: BTreeMap<(RowId, FieldId), BTreeSet<Consumer>>,
    by_query: BTreeMap<IndexKey, BTreeSet<Consumer>>,
    by_list: BTreeMap<ListId, BTreeSet<Consumer>>,
    by_consumer: BTreeMap<Consumer, BTreeSet<DynamicDependency>>,
}

impl DynamicDependencies {
    fn clear(&mut self, consumer: Consumer) {
        let Some(dependencies) = self.by_consumer.remove(&consumer) else {
            return;
        };
        for dependency in dependencies {
            match dependency {
                DynamicDependency::RowField(row, field) => {
                    remove_consumer(&mut self.by_row_field, &(row, field), consumer)
                }
                DynamicDependency::Query(key) => {
                    remove_consumer(&mut self.by_query, &key, consumer)
                }
                DynamicDependency::List(list) => {
                    remove_consumer(&mut self.by_list, &list, consumer)
                }
            }
        }
    }

    fn insert(&mut self, consumer: Consumer, dependency: DynamicDependency) {
        self.by_consumer
            .entry(consumer)
            .or_default()
            .insert(dependency.clone());
        match dependency {
            DynamicDependency::RowField(row, field) => {
                self.by_row_field
                    .entry((row, field))
                    .or_default()
                    .insert(consumer);
            }
            DynamicDependency::Query(key) => {
                self.by_query.entry(key).or_default().insert(consumer);
            }
            DynamicDependency::List(list) => {
                self.by_list.entry(list).or_default().insert(consumer);
            }
        }
    }
}

fn remove_consumer<K: Ord + Clone>(
    map: &mut BTreeMap<K, BTreeSet<Consumer>>,
    key: &K,
    consumer: Consumer,
) {
    let remove_key = map.get_mut(key).is_some_and(|consumers| {
        consumers.remove(&consumer);
        consumers.is_empty()
    });
    if remove_key {
        map.remove(key);
    }
}

#[derive(Clone, Debug, Default)]
struct Dependencies {
    root_by_state: BTreeMap<StateId, BTreeSet<FieldId>>,
    root_by_field: BTreeMap<FieldId, BTreeSet<FieldId>>,
    root_by_list: BTreeMap<ListId, BTreeSet<FieldId>>,
    row_by_field: BTreeMap<(ListId, FieldId), BTreeSet<FieldId>>,
    row_by_root_state: BTreeMap<StateId, BTreeSet<(ListId, FieldId)>>,
    row_by_root_field: BTreeMap<FieldId, BTreeSet<(ListId, FieldId)>>,
    row_by_list: BTreeMap<ListId, BTreeSet<(ListId, FieldId)>>,
}

#[derive(Clone, Debug)]
struct Metadata {
    constants: BTreeMap<PlanConstantId, Value>,
    root_computations: BTreeMap<FieldId, Arc<PlanOp>>,
    row_computations: BTreeMap<FieldId, Arc<PlanOp>>,
    row_field_owner: BTreeMap<FieldId, ListId>,
    indexed_state_field: BTreeMap<StateId, FieldId>,
    indexed_state_owner: BTreeMap<StateId, ListId>,
    list_by_scope: BTreeMap<ScopeId, ListId>,
    list_labels: BTreeMap<ListId, String>,
    list_fields_by_name: BTreeMap<(ListId, String), Vec<FieldId>>,
    list_fields_by_exact_name: BTreeMap<String, Vec<(ListId, FieldId)>>,
    root_field_by_exact_name: BTreeMap<String, Vec<FieldId>>,
    root_field_by_name: BTreeMap<String, Vec<FieldId>>,
    root_state_by_exact_name: BTreeMap<String, Vec<StateId>>,
    root_state_by_name: BTreeMap<String, Vec<StateId>>,
    routes: BTreeMap<SourceId, SourceRoute>,
    internal_effect_result_sources: BTreeSet<SourceId>,
    updates_by_source: BTreeMap<SourceId, Vec<Arc<PlanOp>>>,
    mutations: Vec<Arc<PlanOp>>,
    source_derived_by_source: BTreeMap<SourceId, BTreeSet<FieldId>>,
    published: BTreeSet<FieldId>,
    dependencies: Dependencies,
}

impl Metadata {
    fn new(plan: &MachinePlan) -> Result<Self, Error> {
        let constants = plan
            .constants
            .iter()
            .map(|constant| Ok((constant.id, constant_value(&constant.value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        let field_labels = debug_labels(&plan.debug_map.fields, "field:")
            .into_iter()
            .map(|(id, label)| (FieldId(id), label))
            .collect::<BTreeMap<_, _>>();
        let mut row_field_owner = BTreeMap::new();
        let mut list_fields_by_name = BTreeMap::<(ListId, String), Vec<FieldId>>::new();
        let mut list_fields_by_exact_name = BTreeMap::<String, Vec<(ListId, FieldId)>>::new();
        for slot in &plan.storage_layout.list_slots {
            let persistence = plan
                .persistence
                .lists
                .iter()
                .find(|memory| memory.runtime_slot == slot.id);
            for field in &slot.row_field_ids {
                if let Some(previous) = row_field_owner.insert(*field, slot.list_id)
                    && previous != slot.list_id
                {
                    return Err(Error::InvalidPlan(format!(
                        "field {} belongs to lists {} and {}",
                        field.0, previous.0, slot.list_id.0
                    )));
                }
                if let Some(label) = field_labels.get(field) {
                    list_fields_by_name
                        .entry((slot.list_id, label.clone()))
                        .or_default()
                        .push(*field);
                    list_fields_by_name
                        .entry((slot.list_id, local_name(label).to_owned()))
                        .or_default()
                        .push(*field);
                }
                if let Some(leaf) = persistence.and_then(|memory| {
                    memory
                        .row_fields
                        .iter()
                        .find(|leaf| leaf.runtime_field_id == Some(*field))
                }) {
                    list_fields_by_name
                        .entry((slot.list_id, leaf.semantic_path.clone()))
                        .or_default()
                        .push(*field);
                    list_fields_by_name
                        .entry((slot.list_id, local_name(&leaf.semantic_path).to_owned()))
                        .or_default()
                        .push(*field);
                    list_fields_by_exact_name
                        .entry(leaf.semantic_path.clone())
                        .or_default()
                        .push((slot.list_id, *field));
                }
            }
        }
        for fields in list_fields_by_name.values_mut() {
            fields.sort();
            fields.dedup();
        }
        for fields in list_fields_by_exact_name.values_mut() {
            fields.sort();
            fields.dedup();
        }

        let list_labels = debug_labels(&plan.debug_map.list_slots, "list:")
            .into_iter()
            .map(|(id, label)| (ListId(id), label))
            .collect::<BTreeMap<_, _>>();
        let state_labels = debug_labels(&plan.debug_map.state_slots, "state:")
            .into_iter()
            .map(|(id, label)| (StateId(id), label))
            .collect::<BTreeMap<_, _>>();
        let mut root_state_by_exact_name = BTreeMap::<String, Vec<StateId>>::new();
        let mut root_state_by_name = BTreeMap::<String, Vec<StateId>>::new();
        for slot in plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| !slot.indexed)
        {
            if let Some(label) = state_labels.get(&slot.state_id) {
                root_state_by_exact_name
                    .entry(label.clone())
                    .or_default()
                    .push(slot.state_id);
                for name in debug_name_variants(label) {
                    root_state_by_name
                        .entry(name)
                        .or_default()
                        .push(slot.state_id);
                }
            }
        }
        for states in root_state_by_exact_name.values_mut() {
            states.sort();
            states.dedup();
        }
        for states in root_state_by_name.values_mut() {
            states.sort();
            states.dedup();
        }
        let mut list_by_scope = BTreeMap::new();
        for slot in &plan.storage_layout.list_slots {
            if let Some(scope) = slot.scope_id
                && let Some(previous) = list_by_scope.insert(scope, slot.list_id)
                && previous != slot.list_id
            {
                return Err(Error::InvalidPlan(format!(
                    "scope {} owns lists {} and {}",
                    scope.0, previous.0, slot.list_id.0
                )));
            }
        }
        let mut indexed_state_field = BTreeMap::new();
        let mut indexed_state_owner = BTreeMap::new();
        for slot in plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| slot.indexed)
        {
            let scope = slot.scope_id.ok_or_else(|| {
                Error::InvalidPlan(format!("indexed state {} has no scope", slot.state_id.0))
            })?;
            let list = *list_by_scope.get(&scope).ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "indexed state {} scope {} has no list",
                    slot.state_id.0, scope.0
                ))
            })?;
            let label = state_labels.get(&slot.state_id).ok_or_else(|| {
                Error::InvalidPlan(format!("state {} has no debug label", slot.state_id.0))
            })?;
            let field = resolve_named_field(&list_fields_by_name, list, label)
                .or_else(|| resolve_named_field(&list_fields_by_name, list, local_name(label)));
            let field = field.ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "indexed state {} `{label}` has no FieldId in list {}",
                    slot.state_id.0, list.0
                ))
            })?;
            indexed_state_field.insert(slot.state_id, field);
            indexed_state_owner.insert(slot.state_id, list);
        }

        let mut root_computations = BTreeMap::new();
        let mut row_computations = BTreeMap::new();
        let mut source_derived_by_source = BTreeMap::<SourceId, BTreeSet<FieldId>>::new();
        let mut updates_by_source = BTreeMap::<SourceId, Vec<Arc<PlanOp>>>::new();
        let mut mutations = Vec::new();
        for op in plan.regions.iter().flat_map(|region| &region.ops) {
            match &op.kind {
                PlanOpKind::DerivedValue { derived_kind, .. } => {
                    let Some(ValueRef::Field(field)) = op.output else {
                        return Err(Error::InvalidPlan(format!(
                            "derived op {} has no field output",
                            op.id.0
                        )));
                    };
                    if op.indexed {
                        row_computations.insert(field, Arc::new(op.clone()));
                    } else {
                        root_computations.insert(field, Arc::new(op.clone()));
                        if *derived_kind == PlanDerivedKind::SourceEventTransform {
                            for source in op.inputs.iter().filter_map(|input| match input {
                                ValueRef::Source(source) => Some(*source),
                                _ => None,
                            }) {
                                source_derived_by_source
                                    .entry(source)
                                    .or_default()
                                    .insert(field);
                            }
                        }
                    }
                }
                PlanOpKind::ListProjection { .. } => {
                    let Some(ValueRef::Field(field)) = op.output else {
                        return Err(Error::InvalidPlan(format!(
                            "list projection op {} has no field output",
                            op.id.0
                        )));
                    };
                    root_computations.insert(field, Arc::new(op.clone()));
                }
                PlanOpKind::UpdateBranch { .. } => {
                    let op = Arc::new(op.clone());
                    for source in op.inputs.iter().filter_map(|input| match input {
                        ValueRef::Source(source) => Some(*source),
                        _ => None,
                    }) {
                        updates_by_source
                            .entry(source)
                            .or_default()
                            .push(Arc::clone(&op));
                    }
                }
                PlanOpKind::ListOperation {
                    operation_kind,
                    retain,
                    count,
                    ..
                } => match operation_kind {
                    PlanListOperationKind::Append | PlanListOperationKind::Remove => {
                        mutations.push(Arc::new(op.clone()));
                    }
                    PlanListOperationKind::Retain => {
                        let Some(ValueRef::Field(field)) =
                            retain.as_ref().map(|retain| &retain.target)
                        else {
                            return Err(Error::InvalidPlan(format!(
                                "list retain op {} has no field target",
                                op.id.0
                            )));
                        };
                        root_computations.insert(*field, Arc::new(op.clone()));
                    }
                    PlanListOperationKind::Count => {
                        let Some(ValueRef::Field(field)) =
                            count.as_ref().map(|count| &count.target)
                        else {
                            return Err(Error::InvalidPlan(format!(
                                "list count op {} has no field target",
                                op.id.0
                            )));
                        };
                        root_computations.insert(*field, Arc::new(op.clone()));
                    }
                },
                PlanOpKind::SourceRoute
                | PlanOpKind::StateInitialize { .. }
                | PlanOpKind::DependencyEdge => {}
            }
        }
        for ops in updates_by_source.values_mut() {
            ops.sort_by_key(|op| op.id);
        }
        mutations.sort_by_key(|op| op.id);

        let published: BTreeSet<FieldId> = match &plan.demand.root_derived_outputs {
            RootOutputDemand::All => root_computations.keys().copied().collect(),
            RootOutputDemand::Selected(fields) => fields.iter().copied().collect(),
        };

        let mut root_field_by_exact_name = BTreeMap::<String, Vec<FieldId>>::new();
        let mut root_field_by_name = BTreeMap::<String, Vec<FieldId>>::new();
        for field in root_computations.keys() {
            if let Some(label) = field_labels.get(field) {
                root_field_by_exact_name
                    .entry(label.clone())
                    .or_default()
                    .push(*field);
                for name in debug_name_variants(label) {
                    root_field_by_name.entry(name).or_default().push(*field);
                }
            }
        }
        for fields in root_field_by_exact_name.values_mut() {
            fields.sort();
            fields.dedup();
        }
        for fields in root_field_by_name.values_mut() {
            fields.sort();
            fields.dedup();
        }

        let routes = plan
            .source_routes
            .iter()
            .map(|route| (route.source_id, route.clone()))
            .collect::<BTreeMap<_, _>>();
        if routes.len() != plan.source_routes.len() {
            return Err(Error::InvalidPlan("duplicate source id".to_owned()));
        }
        let internal_effect_result_sources = plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .filter_map(|op| match &op.kind {
                PlanOpKind::UpdateBranch {
                    effect:
                        Some(boon_plan::EffectInvocationPlan {
                            result: boon_plan::EffectResultRoute::CorrelatedSources { variants },
                            ..
                        }),
                    ..
                } => Some(variants.as_slice()),
                _ => None,
            })
            .flatten()
            .map(|route| route.source_id)
            .collect::<BTreeSet<_>>();

        let mut metadata = Self {
            constants,
            root_computations,
            row_computations,
            row_field_owner,
            indexed_state_field,
            indexed_state_owner,
            list_by_scope,
            list_labels,
            list_fields_by_name,
            list_fields_by_exact_name,
            root_field_by_exact_name,
            root_field_by_name,
            root_state_by_exact_name,
            root_state_by_name,
            routes,
            internal_effect_result_sources,
            updates_by_source,
            mutations,
            source_derived_by_source,
            published,
            dependencies: Dependencies::default(),
        };
        metadata.dependencies = metadata.build_dependencies();
        Ok(metadata)
    }

    fn build_dependencies(&self) -> Dependencies {
        let mut dependencies = Dependencies::default();
        for (output, op) in &self.root_computations {
            if source_event_transform_op(op) {
                continue;
            }
            for input in &op.inputs {
                match input {
                    ValueRef::State(state) if !self.indexed_state_owner.contains_key(state) => {
                        dependencies
                            .root_by_state
                            .entry(*state)
                            .or_default()
                            .insert(*output);
                    }
                    ValueRef::Field(field) if !self.row_field_owner.contains_key(field) => {
                        dependencies
                            .root_by_field
                            .entry(*field)
                            .or_default()
                            .insert(*output);
                    }
                    ValueRef::List(list) => {
                        dependencies
                            .root_by_list
                            .entry(*list)
                            .or_default()
                            .insert(*output);
                    }
                    _ => {}
                }
            }
            if matches!(
                op.kind,
                PlanOpKind::ListOperation {
                    operation_kind: PlanListOperationKind::Retain | PlanListOperationKind::Count,
                    ..
                }
            ) && let Some(ValueRef::List(list)) = op.output
            {
                dependencies
                    .root_by_list
                    .entry(list)
                    .or_default()
                    .insert(*output);
            }
        }
        for (output, op) in &self.row_computations {
            let Some(owner) = self.row_field_owner.get(output).copied() else {
                continue;
            };
            for input in &op.inputs {
                match input {
                    ValueRef::State(state) => {
                        if self.indexed_state_owner.get(state) == Some(&owner) {
                            if let Some(field) = self.indexed_state_field.get(state) {
                                dependencies
                                    .row_by_field
                                    .entry((owner, *field))
                                    .or_default()
                                    .insert(*output);
                            }
                        } else if !self.indexed_state_owner.contains_key(state) {
                            dependencies
                                .row_by_root_state
                                .entry(*state)
                                .or_default()
                                .insert((owner, *output));
                        }
                    }
                    ValueRef::Field(field) => {
                        if self.row_field_owner.get(field) == Some(&owner) {
                            dependencies
                                .row_by_field
                                .entry((owner, *field))
                                .or_default()
                                .insert(*output);
                        } else if !self.row_field_owner.contains_key(field) {
                            dependencies
                                .row_by_root_field
                                .entry(*field)
                                .or_default()
                                .insert((owner, *output));
                        }
                    }
                    // List/get and List/find install precise runtime dependencies. A
                    // List/ref installs a broad runtime dependency when evaluated.
                    ValueRef::List(_) => {}
                    _ => {}
                }
            }
        }
        dependencies
    }

    fn list_field(&self, list: ListId, name: &str) -> Result<FieldId, Error> {
        let fields = self
            .list_fields_by_name
            .get(&(list, name.to_owned()))
            .or_else(|| {
                self.list_fields_by_name
                    .get(&(list, local_name(name).to_owned()))
            })
            .ok_or_else(|| Error::InvalidPlan(format!("list {} has no field `{name}`", list.0)))?;
        match fields.as_slice() {
            [field] => Ok(*field),
            _ => Err(Error::InvalidPlan(format!(
                "list {} field name `{name}` is ambiguous across FieldIds {:?}",
                list.0, fields
            ))),
        }
    }

    fn root_field(&self, name: &str) -> Result<FieldId, Error> {
        if let Some(field) = unique_root_name(&self.root_field_by_exact_name, name, "field")? {
            return Ok(field);
        }
        if !name.starts_with("store.")
            && let Some(field) = unique_root_name(
                &self.root_field_by_exact_name,
                &format!("store.{name}"),
                "field",
            )?
        {
            return Ok(field);
        }
        let fields = self
            .root_field_by_name
            .get(name)
            .or_else(|| self.root_field_by_name.get(local_name(name)))
            .ok_or_else(|| Error::InvalidPlan(format!("no root field `{name}`")))?;
        match fields.as_slice() {
            [field] => Ok(*field),
            _ => Err(Error::InvalidPlan(format!(
                "root field name `{name}` is ambiguous across FieldIds {:?}",
                fields
            ))),
        }
    }

    fn list_storage_field(&self, list: ListId, name: &str) -> Result<FieldId, Error> {
        let list_label = self
            .list_labels
            .get(&list)
            .ok_or_else(|| Error::InvalidPlan(format!("list {} has no debug label", list.0)))?;
        self.list_field(list, &format!("{list_label}.$input${name}"))
            .or_else(|_| self.list_field(list, &format!("{list_label}.{name}")))
    }

    fn any_list_field(&self, name: &str) -> Result<(ListId, FieldId), Error> {
        let collect = |candidate: &str| {
            self.list_fields_by_name
                .iter()
                .filter(|((_, field_name), fields)| field_name == candidate && fields.len() == 1)
                .map(|((list, _), fields)| (*list, fields[0]))
                .collect::<BTreeSet<_>>()
        };
        let mut matches = collect(name);
        if matches.is_empty() {
            matches = collect(local_name(name));
        }
        match matches.into_iter().collect::<Vec<_>>().as_slice() {
            [match_] => Ok(*match_),
            [] => Err(Error::InvalidPlan(format!(
                "no root value or list field `{name}`"
            ))),
            matches => Err(Error::InvalidPlan(format!(
                "list field `{name}` is ambiguous across {matches:?}"
            ))),
        }
    }

    fn exact_list_field(&self, name: &str) -> Result<Option<(ListId, FieldId)>, Error> {
        let Some(fields) = self.list_fields_by_exact_name.get(name) else {
            return Ok(None);
        };
        match fields.as_slice() {
            [field] => Ok(Some(*field)),
            fields => Err(Error::InvalidPlan(format!(
                "exact list field `{name}` is ambiguous across {fields:?}"
            ))),
        }
    }
}

fn debug_name_variants(label: &str) -> Vec<String> {
    let parts = label.split('.').collect::<Vec<_>>();
    (0..parts.len())
        .map(|start| parts[start..].join("."))
        .collect()
}

fn unique_root_name<T: Copy + std::fmt::Debug>(
    names: &BTreeMap<String, Vec<T>>,
    name: &str,
    kind: &str,
) -> Result<Option<T>, Error> {
    match names.get(name).map(Vec::as_slice) {
        Some([value]) => Ok(Some(*value)),
        Some(values) => Err(Error::InvalidPlan(format!(
            "root {kind} name `{name}` is ambiguous across {values:?}"
        ))),
        None => Ok(None),
    }
}

fn resolve_named_field(
    fields: &BTreeMap<(ListId, String), Vec<FieldId>>,
    list: ListId,
    name: &str,
) -> Option<FieldId> {
    match fields.get(&(list, name.to_owned())).map(Vec::as_slice) {
        Some([field]) => Some(*field),
        _ => None,
    }
}

fn debug_labels(entries: &[boon_plan::DebugEntry], prefix: &str) -> BTreeMap<usize, String> {
    entries
        .iter()
        .filter_map(|entry| {
            entry
                .id
                .strip_prefix(prefix)?
                .parse::<usize>()
                .ok()
                .map(|id| (id, entry.label.clone()))
        })
        .collect()
}

fn local_name(label: &str) -> &str {
    label.rsplit('.').next().unwrap_or(label)
}

fn constant_value(value: &PlanConstantValue) -> Result<Value, Error> {
    match value {
        PlanConstantValue::Text { value } | PlanConstantValue::Enum { value } => {
            Ok(Value::Text(value.clone()))
        }
        PlanConstantValue::Number { value } => Ok(Value::Number(*value)),
        PlanConstantValue::Byte { value } => Ok(Value::Number(i64::from(*value))),
        PlanConstantValue::Bool { value } => Ok(Value::Bool(*value)),
        PlanConstantValue::Bytes {
            byte_len,
            inline_bytes,
            ..
        } => {
            let bytes = inline_bytes.clone().ok_or_else(|| {
                Error::InvalidPlan("BYTES constant has no inline payload".to_owned())
            })?;
            if bytes.len() as u64 != *byte_len {
                return Err(Error::InvalidPlan(format!(
                    "BYTES constant length {} does not match byte_len {byte_len}",
                    bytes.len()
                )));
            }
            Ok(Value::Bytes(bytes))
        }
    }
}

fn stable_list_fields(
    list: &boon_plan::ListMemoryPlan,
) -> Result<BTreeMap<FieldId, boon_plan::MemoryLeafId>, Error> {
    let mut fields = BTreeMap::new();
    for leaf in &list.row_fields {
        let field = leaf.runtime_field_id.ok_or_else(|| {
            Error::InvalidPlan(format!(
                "persistence list {} leaf {} has no runtime FieldId",
                list.memory_id, leaf.leaf_id
            ))
        })?;
        if fields.insert(field, leaf.leaf_id).is_some() {
            return Err(Error::InvalidPlan(format!(
                "persistence list {} repeats runtime field {}",
                list.memory_id, field.0
            )));
        }
    }
    Ok(fields)
}

fn stored_list(
    memory: &boon_plan::ListMemoryPlan,
    authority: &ListAuthority,
    touched_only: bool,
) -> Result<boon_persistence::StoredList, Error> {
    let rows = authority
        .rows
        .iter()
        .filter(|row| !touched_only || !row.touched_fields.is_empty())
        .map(|row| stored_row(memory, row, touched_only))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(boon_persistence::StoredList {
        touched: authority.touched,
        next_key: if authority.touched {
            authority.next_key
        } else {
            0
        },
        rows,
    })
}

fn stored_row(
    memory: &boon_plan::ListMemoryPlan,
    row: &RowAuthority,
    touched_only: bool,
) -> Result<boon_persistence::StoredRow, Error> {
    let stable_fields = stable_list_fields(memory)?;
    let fields = row
        .fields
        .iter()
        .filter(|(field, _)| !touched_only || row.touched_fields.contains(field))
        .map(|(field, value)| {
            let stable = stable_fields.get(field).copied().ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "persistence list {} has no stable identity for field {}",
                    memory.memory_id, field.0
                ))
            })?;
            Ok((stable, stored_value(value)?))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    let touched_fields = row
        .touched_fields
        .iter()
        .map(|field| {
            stable_fields.get(field).copied().ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "persistence list {} has no stable identity for touched field {}",
                    memory.memory_id, field.0
                ))
            })
        })
        .collect::<Result<BTreeSet<_>, Error>>()?;
    Ok(boon_persistence::StoredRow {
        key: row.id.key,
        generation: row.id.generation,
        fields,
        touched_fields,
    })
}

pub(crate) fn stored_value(value: &Value) -> Result<boon_persistence::StoredValue, Error> {
    match value {
        Value::Null => Ok(boon_persistence::StoredValue::Null),
        Value::Bool(value) => Ok(boon_persistence::StoredValue::Bool(*value)),
        Value::Number(value) => Ok(boon_persistence::StoredValue::Number(*value)),
        Value::Text(value) => Ok(boon_persistence::StoredValue::Text(value.clone())),
        Value::Bytes(value) => Ok(boon_persistence::StoredValue::Bytes(value.clone())),
        Value::List(values) => values
            .iter()
            .map(stored_value)
            .collect::<Result<Vec<_>, _>>()
            .map(boon_persistence::StoredValue::List),
        Value::Record(fields) => {
            let mut stored = fields
                .iter()
                .filter(|(name, _)| name.as_str() != "$tag")
                .map(|(name, value)| Ok((name.clone(), stored_value(value)?)))
                .collect::<Result<BTreeMap<_, _>, Error>>()?;
            match fields.get("$tag") {
                Some(Value::Text(tag)) => Ok(boon_persistence::StoredValue::Variant {
                    tag: tag.clone(),
                    fields: std::mem::take(&mut stored),
                }),
                Some(_) => Err(Error::Evaluation(
                    "tagged runtime record has a non-text `$tag` field".to_owned(),
                )),
                None => Ok(boon_persistence::StoredValue::Record(stored)),
            }
        }
        Value::Error { code } => Ok(boon_persistence::StoredValue::Error {
            code: code.clone(),
            fields: BTreeMap::new(),
        }),
        Value::MappedRow { .. } | Value::Row { .. } => Err(Error::Evaluation(
            "row handles and derived mapped rows are not durable authority".to_owned(),
        )),
    }
}

fn stored_value_for_data_type(
    value: &Value,
    data_type: &boon_plan::DataTypePlan,
) -> Result<boon_persistence::StoredValue, Error> {
    if let (Value::Text(tag), boon_plan::DataTypePlan::Variant { variants }) = (value, data_type) {
        let variant = variants
            .iter()
            .find(|variant| variant.tag == *tag)
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "variant tag `{tag}` is not declared by the durable value schema"
                ))
            })?;
        if !variant.fields.is_empty() {
            return Err(Error::Evaluation(format!(
                "structured variant `{tag}` requires named fields"
            )));
        }
        return Ok(boon_persistence::StoredValue::Variant {
            tag: tag.clone(),
            fields: BTreeMap::new(),
        });
    }
    stored_value(value)
}

pub(crate) fn runtime_value(value: boon_persistence::StoredValue) -> Result<Value, Error> {
    match value {
        boon_persistence::StoredValue::Null => Ok(Value::Null),
        boon_persistence::StoredValue::Bool(value) => Ok(Value::Bool(value)),
        boon_persistence::StoredValue::Number(value) => Ok(Value::Number(value)),
        boon_persistence::StoredValue::Text(value) => Ok(Value::Text(value)),
        boon_persistence::StoredValue::Bytes(value) => Ok(Value::Bytes(value)),
        boon_persistence::StoredValue::List(values) => values
            .into_iter()
            .map(runtime_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        boon_persistence::StoredValue::Record(fields) => fields
            .into_iter()
            .map(|(name, value)| Ok((name, runtime_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()
            .map(Value::Record),
        boon_persistence::StoredValue::Variant { tag, fields } if fields.is_empty() => {
            Ok(Value::Text(tag))
        }
        boon_persistence::StoredValue::Variant { tag, mut fields } => {
            if fields
                .insert("$tag".to_owned(), boon_persistence::StoredValue::Text(tag))
                .is_some()
            {
                return Err(Error::Evaluation(
                    "stored variant contains reserved `$tag` field".to_owned(),
                ));
            }
            runtime_value(boon_persistence::StoredValue::Record(fields))
        }
        boon_persistence::StoredValue::Error { code, fields } => {
            if fields.is_empty() {
                Ok(Value::Error { code })
            } else {
                Err(Error::Evaluation(
                    "runtime cannot restore structured error authority".to_owned(),
                ))
            }
        }
    }
}

#[derive(Clone)]
enum AuthorityUndo {
    RootState {
        state: StateId,
        value: Option<Value>,
        touched: bool,
    },
    RowField {
        row: RowId,
        field: FieldId,
        value: Option<Value>,
        touched_field: bool,
        touched_list: bool,
    },
    AppendRow {
        row: RowId,
        previous_next_key: u64,
        touched_list: bool,
    },
    RemoveRow {
        row: RowId,
        value: Row,
        order_index: usize,
        previous_next_key: u64,
        touched_list: bool,
        touched_fields: BTreeSet<FieldId>,
    },
}

#[derive(Clone, Default)]
struct Work {
    emit: bool,
    deltas: Vec<Delta>,
    authority_deltas: Vec<AuthorityDelta>,
    outbox_changes: Vec<boon_persistence::DurableOutboxChange>,
    metrics: TurnMetrics,
    dirty_states: HashSet<StateId>,
    dirty_consumers: HashSet<Consumer>,
    changed_rows: HashSet<RowId>,
    suppress_row_deltas: HashSet<RowId>,
    recomputed_targets: HashSet<ValueTarget>,
    authority_undo: Vec<AuthorityUndo>,
    undo_root_states: HashSet<StateId>,
    undo_row_fields: HashSet<(RowId, FieldId)>,
    pending_settle: bool,
    previous_last_sequence: Option<u64>,
    previous_turn_sequence: u64,
    work_limit: Option<u64>,
    work_units: u64,
    enforce_work_limit: bool,
}

impl Work {
    fn with_limit(work_limit: Option<u64>) -> Self {
        Self {
            work_limit,
            enforce_work_limit: true,
            ..Self::default()
        }
    }

    fn begin_turn(&mut self, last_sequence: Option<u64>, turn_sequence: u64) {
        self.emit = true;
        self.deltas.clear();
        self.authority_deltas.clear();
        self.outbox_changes.clear();
        self.metrics = TurnMetrics::default();
        self.dirty_states.clear();
        self.dirty_consumers.clear();
        self.changed_rows.clear();
        self.suppress_row_deltas.clear();
        self.recomputed_targets.clear();
        self.authority_undo.clear();
        self.undo_root_states.clear();
        self.undo_row_fields.clear();
        self.pending_settle = false;
        self.previous_last_sequence = last_sequence;
        self.previous_turn_sequence = turn_sequence;
        self.work_units = 0;
        self.enforce_work_limit = true;
    }

    fn consume(&mut self, units: u64) -> Result<(), Error> {
        let attempted = self.work_units.saturating_add(units);
        if self.enforce_work_limit
            && self
                .work_limit
                .is_some_and(|work_limit| attempted > work_limit)
        {
            return Err(Error::WorkBudgetExceeded {
                limit: self.work_limit.unwrap_or_default(),
                attempted,
            });
        }
        self.work_units = attempted;
        Ok(())
    }

    fn allow_rollback(&mut self) {
        // Authority rollback must complete after a bounded evaluation aborts.
        // Static plan and storage limits still bound this recovery path.
        self.enforce_work_limit = false;
    }

    fn settle(&mut self) {
        self.authority_undo.clear();
        self.undo_root_states.clear();
        self.undo_row_fields.clear();
        self.pending_settle = false;
    }

    fn finish_metrics(&mut self) {
        self.metrics.dirty_state_count = self.dirty_states.len();
        self.metrics.dirty_field_count = self.dirty_consumers.len();
        self.metrics.changed_row_count = self.changed_rows.len();
        self.metrics.work_unit_count = self.work_units;
        self.metrics.recomputed_targets.clear();
        self.metrics
            .recomputed_targets
            .extend(self.recomputed_targets.iter().copied());
        self.metrics.recomputed_targets.sort_unstable();
    }
}

#[derive(Clone, Debug)]
enum EvalValue {
    Value(Value),
    Row(RowId),
    List(Vec<EvalValue>),
    Record(BTreeMap<String, EvalValue>),
    MappedRow {
        id: RowId,
        fields: BTreeMap<String, EvalValue>,
    },
}

#[derive(Clone)]
pub struct Session {
    plan: Arc<MachinePlan>,
    options: SessionOptions,
    metadata: Arc<Metadata>,
    root_states: BTreeMap<StateId, Value>,
    root_fields: BTreeMap<FieldId, DerivedCell>,
    lists: BTreeMap<ListId, ListState>,
    indexes: BTreeMap<IndexKey, BTreeSet<RowId>>,
    dynamic_dependencies: DynamicDependencies,
    last_sequence: Option<u64>,
    turn_sequence: u64,
    next_binding_id: u64,
    touched_root_states: BTreeSet<StateId>,
    touched_lists: BTreeSet<ListId>,
    touched_row_fields: BTreeSet<(RowId, FieldId)>,
    turn_work: Work,
}

pub struct SessionBuilder {
    session: Session,
    authority: Option<AuthoritySnapshot>,
}

impl SessionBuilder {
    pub fn new(plan: MachinePlan, options: SessionOptions) -> Result<Self, Error> {
        Self::new_shared(Arc::new(plan), options)
    }

    pub fn new_shared(plan: Arc<MachinePlan>, options: SessionOptions) -> Result<Self, Error> {
        if plan.version.major != boon_plan::PLAN_MAJOR_VERSION {
            return Err(Error::InvalidPlan(format!(
                "plan major version {} is not supported",
                plan.version.major
            )));
        }
        let metadata = Arc::new(Metadata::new(&plan)?);
        let turn_work = Work::with_limit(options.max_work_units_per_transaction);
        Ok(Self {
            session: Session {
                plan,
                options,
                metadata,
                root_states: BTreeMap::new(),
                root_fields: BTreeMap::new(),
                lists: BTreeMap::new(),
                indexes: BTreeMap::new(),
                dynamic_dependencies: DynamicDependencies::default(),
                last_sequence: None,
                turn_sequence: 0,
                next_binding_id: 1,
                touched_root_states: BTreeSet::new(),
                touched_lists: BTreeSet::new(),
                touched_row_fields: BTreeSet::new(),
                turn_work,
            },
            authority: None,
        })
    }

    pub fn restore(mut self, authority: AuthoritySnapshot) -> Self {
        self.authority = Some(authority);
        self
    }

    pub fn restore_durable(mut self, image: boon_persistence::RestoreImage) -> Result<Self, Error> {
        self.authority = Some(self.session.authority_from_durable(image)?);
        Ok(self)
    }

    pub fn build(mut self) -> Result<Session, Error> {
        let mut work = self.session.fresh_work();
        self.session.initialize_storage_defaults(&mut work)?;
        if let Some(authority) = self.authority.take() {
            self.session.install_authority(authority, &mut work)?;
        }
        self.session.initialize_root_field_defaults(&mut work)?;
        self.session.ensure_published_current(None, &mut work)?;
        Ok(self.session)
    }
}

impl Session {
    fn fresh_work(&self) -> Work {
        Work::with_limit(self.options.max_work_units_per_transaction)
    }

    pub fn new(plan: MachinePlan, options: SessionOptions) -> Result<Self, Error> {
        SessionBuilder::new(plan, options)?.build()
    }

    pub fn new_shared(plan: Arc<MachinePlan>, options: SessionOptions) -> Result<Self, Error> {
        SessionBuilder::new_shared(plan, options)?.build()
    }

    pub fn plan(&self) -> &MachinePlan {
        &self.plan
    }

    pub fn shared_plan(&self) -> Arc<MachinePlan> {
        Arc::clone(&self.plan)
    }

    pub fn list_rows(&self, list: ListId) -> Vec<RowId> {
        self.list_row_ids(list)
    }

    pub fn logical_row_count(&self) -> usize {
        self.lists.values().map(|list| list.order.len()).sum()
    }

    pub fn list_row_at(&self, list: ListId, index: usize) -> Option<RowId> {
        self.lists
            .get(&list)
            .and_then(|state| state.order.get(index))
            .copied()
    }

    pub fn list_row_snapshots(&self, list: ListId) -> Result<Vec<RowSnapshot>, Error> {
        let Some(state) = self.lists.get(&list) else {
            return Ok(Vec::new());
        };
        state
            .order
            .iter()
            .map(|row| self.row_snapshot(*row))
            .collect()
    }

    pub fn row_snapshot(&self, row: RowId) -> Result<RowSnapshot, Error> {
        let state = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "row {}:{}:{} does not exist",
                    row.list.0, row.key, row.generation
                ))
            })?;
        Ok(RowSnapshot {
            id: row,
            fields: state
                .fields
                .iter()
                .filter(|(field, _)| {
                    state.derived.get(field).is_none_or(|currentness| {
                        *currentness == Currentness::Current
                            || self.row_identity_has_raw_value(row, **field)
                    })
                })
                .map(|(field, value)| (*field, value.clone()))
                .collect(),
        })
    }

    pub fn find_row_by_text(&self, list: ListId, text: &str, occurrence: usize) -> Option<RowId> {
        self.lists
            .get(&list)?
            .order
            .iter()
            .filter_map(|row| {
                let state = self.lists.get(&list)?.rows.get(row)?;
                state
                    .fields
                    .values()
                    .any(|value| matches!(value, Value::Text(value) if value == text))
                    .then_some(*row)
            })
            .nth(occurrence)
    }

    pub fn settle_published(&mut self) -> Result<(), Error> {
        let mut work = self.fresh_work();
        self.ensure_published_current(None, &mut work)
    }

    pub fn document_plan(&self) -> Option<&boon_plan::DocumentPlan> {
        self.plan.document_plan()
    }

    pub fn initial_document_patch_batch(&self) -> Option<&boon_plan::DocumentInitialPatchBatch> {
        self.plan.initial_document_patch_batch()
    }

    pub fn document_binding_value_target(
        &self,
        binding: boon_plan::DocumentBindingId,
        row: Option<RowId>,
    ) -> Result<Option<ValueTarget>, Error> {
        let document = self
            .document_plan()
            .ok_or_else(|| Error::InvalidPlan("MachinePlan has no document plan".to_owned()))?;
        let binding = document
            .view_bindings
            .iter()
            .find(|candidate| candidate.id == binding)
            .ok_or_else(|| {
                Error::InvalidPlan(format!("document binding {} does not exist", binding.0))
            })?;
        match binding.target {
            boon_plan::DocumentBindingTarget::State { state } => {
                Ok(Some(ValueTarget::State(state)))
            }
            boon_plan::DocumentBindingTarget::Field { field } => {
                Ok(Some(ValueTarget::Field(field)))
            }
            boon_plan::DocumentBindingTarget::ScopedField { scope, field } => {
                let row = row.ok_or_else(|| {
                    Error::InvalidEvent(format!("document binding {} requires a row", binding.id.0))
                })?;
                let owner = self.metadata.list_by_scope.get(&scope).ok_or_else(|| {
                    Error::InvalidPlan(format!("document scope {} has no owning list", scope.0))
                })?;
                if row.list != *owner {
                    return Err(Error::InvalidEvent(format!(
                        "document binding {} row belongs to list {}, expected {}",
                        binding.id.0, row.list.0, owner.0
                    )));
                }
                Ok(Some(ValueTarget::RowField { row, field }))
            }
            boon_plan::DocumentBindingTarget::Source { .. }
            | boon_plan::DocumentBindingTarget::List { .. }
            | boon_plan::DocumentBindingTarget::Expression { .. } => Ok(None),
        }
    }

    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        let mut snapshot = Snapshot {
            states: self.root_states.clone(),
            fields: BTreeMap::new(),
            lists: BTreeMap::new(),
        };
        for field in &self.metadata.published {
            let cell = self.root_fields.get(field).ok_or_else(|| {
                Error::InvalidPlan(format!("demanded field {} has no computation", field.0))
            })?;
            if cell.currentness != Currentness::Current {
                return Err(Error::Evaluation(format!(
                    "demanded field {} is not current",
                    field.0
                )));
            }
            snapshot.fields.insert(
                *field,
                cell.value.clone().ok_or_else(|| {
                    Error::Evaluation(format!("demanded field {} has no value", field.0))
                })?,
            );
        }
        for (list, state) in &self.lists {
            let rows = state
                .order
                .iter()
                .map(|row_id| {
                    let row = state.rows.get(row_id).ok_or_else(|| {
                        Error::Evaluation(format!("list {} order contains a missing row", list.0))
                    })?;
                    Ok(RowSnapshot {
                        id: *row_id,
                        fields: row
                            .fields
                            .iter()
                            .filter(|(field, _)| {
                                row.derived.get(field).is_none_or(|currentness| {
                                    *currentness == Currentness::Current
                                        || self.row_identity_has_raw_value(*row_id, **field)
                                })
                            })
                            .map(|(field, value)| (*field, value.clone()))
                            .collect(),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            snapshot.lists.insert(*list, rows);
        }
        Ok(snapshot)
    }

    pub fn authority_snapshot(&self) -> Result<AuthoritySnapshot, Error> {
        let states = self
            .plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| !slot.indexed)
            .map(|slot| {
                let value = self
                    .root_states
                    .get(&slot.state_id)
                    .cloned()
                    .ok_or_else(|| {
                        Error::Evaluation(format!(
                            "authoritative state {} has no value",
                            slot.state_id.0
                        ))
                    })?;
                Ok((
                    slot.state_id,
                    ScalarAuthority {
                        touched: self.touched_root_states.contains(&slot.state_id),
                        value,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;

        let mut lists = BTreeMap::new();
        for slot in &self.plan.storage_layout.list_slots {
            lists.insert(slot.list_id, self.list_authority(slot.list_id)?);
        }

        Ok(AuthoritySnapshot {
            through_turn_sequence: self.turn_sequence,
            states,
            lists,
        })
    }

    fn list_authority(&self, list_id: ListId) -> Result<ListAuthority, Error> {
        let state = self.lists.get(&list_id).ok_or_else(|| {
            Error::Evaluation(format!("authoritative list {} is missing", list_id.0))
        })?;
        let authority_fields = self.authority_fields_for_list(list_id);
        let rows = state
            .order
            .iter()
            .map(|row_id| {
                let row = state.rows.get(row_id).ok_or_else(|| {
                    Error::Evaluation(format!(
                        "list {} order contains missing row {}:{}",
                        list_id.0, row_id.key, row_id.generation
                    ))
                })?;
                let fields = authority_fields
                    .iter()
                    .filter_map(|field| row.fields.get(field).cloned().map(|value| (*field, value)))
                    .collect();
                let touched_fields = authority_fields
                    .iter()
                    .filter(|field| self.touched_row_fields.contains(&(*row_id, **field)))
                    .copied()
                    .collect();
                Ok(RowAuthority {
                    id: *row_id,
                    fields,
                    touched_fields,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(ListAuthority {
            touched: self.touched_lists.contains(&list_id),
            next_key: state.next_key,
            rows,
        })
    }

    pub fn durable_restore_image(
        &self,
        epoch: u64,
        completed_migration_edges: BTreeSet<boon_plan::MigrationEdgeId>,
    ) -> Result<boon_persistence::RestoreImage, Error> {
        self.restore_image(epoch, completed_migration_edges, false)
    }

    pub fn semantic_value_image(&self) -> Result<boon_persistence::RestoreImage, Error> {
        self.restore_image(0, BTreeSet::new(), true)
    }

    fn restore_image(
        &self,
        epoch: u64,
        completed_migration_edges: BTreeSet<boon_plan::MigrationEdgeId>,
        include_untouched_values: bool,
    ) -> Result<boon_persistence::RestoreImage, Error> {
        let authority = self.authority_snapshot()?;
        let mut scalars = BTreeMap::new();
        for memory in self
            .plan
            .persistence
            .memory
            .iter()
            .filter(|memory| memory.kind == boon_plan::MemoryKind::Scalar)
        {
            let slot = self
                .plan
                .storage_layout
                .scalar_slots
                .iter()
                .find(|slot| slot.id == memory.runtime_slot && !slot.indexed)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "persistence scalar {} has no root runtime slot",
                        memory.memory_id
                    ))
                })?;
            let scalar = authority.states.get(&slot.state_id).ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "persistence scalar {} has no authority value",
                    memory.memory_id
                ))
            })?;
            if include_untouched_values || scalar.touched {
                scalars.insert(
                    memory.memory_id,
                    boon_persistence::StoredScalar {
                        touched: scalar.touched && !include_untouched_values,
                        value: stored_value(&scalar.value)?,
                    },
                );
            }
        }

        let mut lists = BTreeMap::new();
        for list_memory in &self.plan.persistence.lists {
            let slot = self
                .plan
                .storage_layout
                .list_slots
                .iter()
                .find(|slot| slot.id == list_memory.runtime_slot)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "persistence list {} has no runtime slot",
                        list_memory.memory_id
                    ))
                })?;
            let list = authority.lists.get(&slot.list_id).ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "persistence list {} has no authority value",
                    list_memory.memory_id
                ))
            })?;
            let mut stored = stored_list(
                list_memory,
                list,
                !include_untouched_values && !list.touched,
            )?;
            if include_untouched_values {
                stored.touched = false;
                stored.next_key = list.next_key;
                for row in &mut stored.rows {
                    row.touched_fields.clear();
                }
            } else if !stored.touched && stored.rows.is_empty() {
                continue;
            }
            lists.insert(list_memory.memory_id, stored);
        }

        Ok(boon_persistence::RestoreImage {
            application: self.plan.application.identity.clone(),
            schema_version: self.plan.persistence.schema_version,
            schema_hash: self.plan.persistence.schema_hash,
            epoch,
            through_turn_sequence: if include_untouched_values {
                0
            } else {
                authority.through_turn_sequence
            },
            scalars,
            lists,
            completed_migration_edges,
            outbox: BTreeMap::new(),
            content_artifact_manifest: boon_persistence::ContentArtifactManifest::default(),
        })
    }

    pub fn durable_changes(
        &self,
        deltas: &[AuthorityDelta],
    ) -> Result<Vec<boon_persistence::DurableChange>, Error> {
        deltas
            .iter()
            .map(|delta| match delta {
                AuthorityDelta::SetRoot { state, value } => {
                    let memory = self
                        .plan
                        .persistence
                        .memory
                        .iter()
                        .filter(|memory| memory.kind == boon_plan::MemoryKind::Scalar)
                        .find(|memory| {
                            self.plan.storage_layout.scalar_slots.iter().any(|slot| {
                                slot.id == memory.runtime_slot
                                    && !slot.indexed
                                    && slot.state_id == *state
                            })
                        })
                        .ok_or_else(|| {
                            Error::InvalidPlan(format!(
                                "root state {} has no stable persistence identity",
                                state.0
                            ))
                        })?;
                    Ok(boon_persistence::DurableChange::SetScalar {
                        memory_id: memory.memory_id,
                        value: boon_persistence::StoredScalar {
                            touched: true,
                            value: stored_value(value)?,
                        },
                    })
                }
                AuthorityDelta::SetRowField { row, field, value } => {
                    let memory = self.persistence_list(row.list)?;
                    let fields = stable_list_fields(memory)?;
                    let field_id = fields.get(field).copied().ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "list {} field {} has no stable persistence identity",
                            row.list.0, field.0
                        ))
                    })?;
                    Ok(boon_persistence::DurableChange::SetRowField {
                        memory_id: memory.memory_id,
                        row_key: row.key,
                        row_generation: row.generation,
                        field_id,
                        value: stored_value(value)?,
                    })
                }
                AuthorityDelta::ReplaceList { list_id, authority } => {
                    let memory = self.persistence_list(*list_id)?;
                    if !authority.touched {
                        return Err(Error::InvalidPlan(format!(
                            "replacement authority for list {} is not structurally touched",
                            list_id.0
                        )));
                    }
                    Ok(boon_persistence::DurableChange::SetList {
                        memory_id: memory.memory_id,
                        value: stored_list(memory, authority, false)?,
                    })
                }
                AuthorityDelta::InsertRow {
                    row,
                    index,
                    next_key,
                } => {
                    let memory = self.persistence_list(row.id.list)?;
                    Ok(boon_persistence::DurableChange::InsertRow {
                        memory_id: memory.memory_id,
                        index: *index,
                        row: stored_row(memory, row, false)?,
                        next_key: *next_key,
                    })
                }
                AuthorityDelta::RemoveRow { row, next_key } => {
                    let memory = self.persistence_list(row.list)?;
                    Ok(boon_persistence::DurableChange::RemoveRow {
                        memory_id: memory.memory_id,
                        row_key: row.key,
                        row_generation: row.generation,
                        next_key: *next_key,
                    })
                }
            })
            .collect()
    }

    fn persistence_list(&self, list_id: ListId) -> Result<&boon_plan::ListMemoryPlan, Error> {
        self.plan
            .persistence
            .lists
            .iter()
            .find(|memory| {
                self.plan
                    .storage_layout
                    .list_slots
                    .iter()
                    .any(|slot| slot.id == memory.runtime_slot && slot.list_id == list_id)
            })
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "list {} has no stable persistence identity",
                    list_id.0
                ))
            })
    }

    fn authority_from_durable(
        &self,
        image: boon_persistence::RestoreImage,
    ) -> Result<AuthoritySnapshot, Error> {
        if image.application != self.plan.application.identity {
            return Err(Error::InvalidPlan(
                "restore image application identity does not match MachinePlan".to_owned(),
            ));
        }
        if image.schema_version != self.plan.persistence.schema_version
            || image.schema_hash != self.plan.persistence.schema_hash
        {
            return Err(Error::InvalidPlan(
                "restore image schema does not match MachinePlan; migration activation is required"
                    .to_owned(),
            ));
        }

        let scalar_by_memory = self
            .plan
            .persistence
            .memory
            .iter()
            .filter(|memory| memory.kind == boon_plan::MemoryKind::Scalar)
            .map(|memory| (memory.memory_id, memory))
            .collect::<BTreeMap<_, _>>();
        if let Some(memory) = image
            .scalars
            .keys()
            .find(|memory| !scalar_by_memory.contains_key(memory))
        {
            return Err(Error::InvalidPlan(format!(
                "restore image contains unknown scalar memory {memory}"
            )));
        }
        let mut states = BTreeMap::new();
        for (memory_id, scalar) in image.scalars {
            let memory = scalar_by_memory[&memory_id];
            let slot = self
                .plan
                .storage_layout
                .scalar_slots
                .iter()
                .find(|slot| slot.id == memory.runtime_slot && !slot.indexed)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "restore scalar {memory_id} has no root runtime slot"
                    ))
                })?;
            if !scalar.touched {
                return Err(Error::InvalidPlan(format!(
                    "sparse restore scalar {memory_id} is present but not touched"
                )));
            }
            states.insert(
                slot.state_id,
                ScalarAuthority {
                    touched: true,
                    value: runtime_value(scalar.value)?,
                },
            );
        }

        let list_by_memory = self
            .plan
            .persistence
            .lists
            .iter()
            .map(|list| (list.memory_id, list))
            .collect::<BTreeMap<_, _>>();
        if let Some(memory) = image
            .lists
            .keys()
            .find(|memory| !list_by_memory.contains_key(memory))
        {
            return Err(Error::InvalidPlan(format!(
                "restore image contains unknown list memory {memory}"
            )));
        }
        let mut lists = BTreeMap::new();
        for (memory_id, stored) in image.lists {
            let memory = list_by_memory[&memory_id];
            let slot = self
                .plan
                .storage_layout
                .list_slots
                .iter()
                .find(|slot| slot.id == memory.runtime_slot)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!("restore list {memory_id} has no runtime slot"))
                })?;
            let structurally_touched = stored.touched;
            if !structurally_touched && stored.next_key != 0 {
                return Err(Error::InvalidPlan(format!(
                    "sparse row overrides for list {memory_id} must not replace its allocator"
                )));
            }
            let stable_fields = stable_list_fields(memory)?;
            let runtime_fields = stable_fields
                .into_iter()
                .map(|(field, stable)| (stable, field))
                .collect::<BTreeMap<_, _>>();
            let rows = stored
                .rows
                .into_iter()
                .map(|row| {
                    if !structurally_touched
                        && (row.touched_fields.is_empty()
                            || row
                                .fields
                                .keys()
                                .any(|field| !row.touched_fields.contains(field)))
                    {
                        return Err(Error::InvalidPlan(format!(
                            "sparse restore list {memory_id} contains non-override row data"
                        )));
                    }
                    let fields = row
                        .fields
                        .into_iter()
                        .map(|(stable, value)| {
                            let field = runtime_fields.get(&stable).copied().ok_or_else(|| {
                                Error::InvalidPlan(format!(
                                    "restore list {memory_id} contains unknown row leaf {stable}"
                                ))
                            })?;
                            Ok((field, runtime_value(value)?))
                        })
                        .collect::<Result<BTreeMap<_, _>, Error>>()?;
                    let touched_fields = row
                        .touched_fields
                        .into_iter()
                        .map(|stable| {
                            runtime_fields.get(&stable).copied().ok_or_else(|| {
                                Error::InvalidPlan(format!(
                                    "restore list {memory_id} touches unknown row leaf {stable}"
                                ))
                            })
                        })
                        .collect::<Result<BTreeSet<_>, Error>>()?;
                    Ok(RowAuthority {
                        id: RowId {
                            list: slot.list_id,
                            key: row.key,
                            generation: row.generation,
                        },
                        fields,
                        touched_fields,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            lists.insert(
                slot.list_id,
                ListAuthority {
                    touched: structurally_touched,
                    next_key: stored.next_key,
                    rows,
                },
            );
        }

        Ok(AuthoritySnapshot {
            through_turn_sequence: image.through_turn_sequence,
            states,
            lists,
        })
    }

    fn authority_fields_for_list(&self, list: ListId) -> BTreeSet<FieldId> {
        let indexed = self
            .metadata
            .indexed_state_field
            .iter()
            .filter_map(|(state, field)| {
                (self.metadata.indexed_state_owner.get(state) == Some(&list)).then_some(*field)
            });
        let constructor = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list)
            .into_iter()
            .flat_map(|slot| slot.row_field_ids.iter().copied())
            .filter(|field| !self.metadata.row_computations.contains_key(field));
        constructor.chain(indexed).collect()
    }

    pub fn project_current(
        &mut self,
        targets: &[ValueTarget],
    ) -> Result<BTreeMap<ValueTarget, Value>, Error> {
        let mut values = BTreeMap::new();
        let mut work = self.fresh_work();
        for target in targets {
            let value = match *target {
                ValueTarget::State(state) => self.root_states.get(&state).cloned(),
                ValueTarget::Field(field) => {
                    if !self.metadata.published.contains(&field) {
                        return Err(Error::NotDemanded(field));
                    }
                    Some(self.ensure_root_field(field, None, &mut work)?)
                }
                ValueTarget::RowField { row, field } => {
                    if self.metadata.row_computations.contains_key(&field) {
                        Some(self.ensure_row_field(row, field, None, &mut work)?)
                    } else {
                        Some(self.row_value(row, field)?)
                    }
                }
            };
            if let Some(value) = value {
                values.insert(*target, value);
            }
        }
        Ok(values)
    }

    /// Establishes a currentness barrier for an already-owned demand set
    /// without cloning its values. Hosts use this after rollback/restore before
    /// exposing retained output state again.
    pub fn ensure_current(&mut self, targets: &[ValueTarget]) -> Result<(), Error> {
        let mut work = self.fresh_work();
        self.ensure_demanded_current(targets, None, &mut work)
    }

    pub fn root_value_current(&mut self, name: &str) -> Result<Value, Error> {
        if let Some(field) =
            unique_root_name(&self.metadata.root_field_by_exact_name, name, "field")?
        {
            let mut work = self.fresh_work();
            return self.ensure_root_field(field, None, &mut work);
        }
        if let Some(state) =
            unique_root_name(&self.metadata.root_state_by_exact_name, name, "state")?
        {
            return self
                .root_states
                .get(&state)
                .cloned()
                .ok_or_else(|| Error::Evaluation(format!("root state `{name}` has no value")));
        }
        if !name.starts_with("store.") {
            let qualified = format!("store.{name}");
            if let Some(field) =
                unique_root_name(&self.metadata.root_field_by_exact_name, &qualified, "field")?
            {
                let mut work = self.fresh_work();
                return self.ensure_root_field(field, None, &mut work);
            }
            if let Some(state) =
                unique_root_name(&self.metadata.root_state_by_exact_name, &qualified, "state")?
            {
                return self.root_states.get(&state).cloned().ok_or_else(|| {
                    Error::Evaluation(format!("root state `{qualified}` has no value"))
                });
            }
        }

        let local = local_name(name);
        let fields = self.metadata.root_field_by_name.get(local);
        let states = self.metadata.root_state_by_name.get(local);
        match (fields.map(Vec::as_slice), states.map(Vec::as_slice)) {
            (Some([field]), None) => {
                let field = *field;
                let mut work = self.fresh_work();
                self.ensure_root_field(field, None, &mut work)
            }
            (None, Some([state])) => self
                .root_states
                .get(state)
                .cloned()
                .ok_or_else(|| Error::Evaluation(format!("root state `{name}` has no value"))),
            (None, None) => Err(Error::InvalidPlan(format!("no root value `{name}`"))),
            _ => Err(Error::InvalidPlan(format!(
                "root value name `{name}` is ambiguous"
            ))),
        }
    }

    /// Reconstructs one host-owned non-visual output after establishing its
    /// currentness barrier. Output values are derived cache entries, never
    /// authority included in snapshots or durable storage.
    pub fn output_value_current(&mut self, name: &str) -> Result<Value, Error> {
        let output = self
            .plan
            .output_root(name)
            .cloned()
            .ok_or_else(|| Error::Evaluation(format!("output root `{name}` does not exist")))?;
        let boon_plan::OutputContractKind::HostValue { data_type } = output.contract else {
            return Err(Error::Evaluation(format!(
                "output root `{name}` is retained visual content, not a host data value"
            )));
        };
        let boon_plan::OutputValueRef::RuntimeValue { value } = output.value else {
            return Err(Error::InvalidPlan(format!(
                "host output root `{name}` has no runtime value reference"
            )));
        };
        if let (ValueRef::List(list), boon_plan::DataTypePlan::List { item }) = (&value, &data_type)
        {
            return self.output_list_current(*list, item);
        }
        let mut work = self.fresh_work();
        let evaluated = self.eval_value_ref(&value, None, None, None, None, &mut work)?;
        let value = self.materialize_eval(evaluated)?;
        normalize_host_output_value(value)
    }

    fn output_list_current(
        &mut self,
        list: ListId,
        item_type: &boon_plan::DataTypePlan,
    ) -> Result<Value, Error> {
        let boon_plan::DataTypePlan::Record {
            fields,
            open: false,
        } = item_type
        else {
            return Err(Error::Evaluation(format!(
                "list output {} must expose a closed record item type",
                list.0
            )));
        };
        let rows = self.list_row_ids(list);
        let mut values = Vec::with_capacity(rows.len());
        let mut work = self.fresh_work();
        work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
        for row in rows {
            let mut record = BTreeMap::new();
            for output_field in fields {
                let field = self.metadata.list_storage_field(list, &output_field.name)?;
                let value = if self.metadata.row_computations.contains_key(&field) {
                    self.ensure_row_field(row, field, None, &mut work)?
                } else {
                    self.row_value(row, field)?
                };
                record.insert(
                    output_field.name.clone(),
                    normalize_host_output_value(value)?,
                );
            }
            values.push(Value::Record(record));
        }
        Ok(Value::List(values))
    }

    pub fn inspect_value_current(&mut self, name: &str, max_rows: usize) -> Result<Value, Error> {
        if let Some((list, field)) = self.metadata.exact_list_field(name)? {
            return self.inspect_list_field_current(list, field, max_rows);
        }
        if self.metadata.root_field_by_name.contains_key(name)
            || self
                .metadata
                .root_field_by_name
                .contains_key(local_name(name))
            || self.metadata.root_state_by_name.contains_key(name)
            || self
                .metadata
                .root_state_by_name
                .contains_key(local_name(name))
        {
            return self.root_value_current(name);
        }
        let (list, field) = self.metadata.any_list_field(name)?;
        self.inspect_list_field_current(list, field, max_rows)
    }

    fn inspect_list_field_current(
        &mut self,
        list: ListId,
        field: FieldId,
        max_rows: usize,
    ) -> Result<Value, Error> {
        let rows = self
            .lists
            .get(&list)
            .map(|state| {
                state
                    .order
                    .iter()
                    .copied()
                    .take(max_rows)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut work = self.fresh_work();
        work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
        let mut values = Vec::with_capacity(rows.len());
        for row in rows {
            let value = if self.metadata.row_computations.contains_key(&field) {
                self.ensure_row_field(row, field, None, &mut work)?
            } else {
                self.row_value(row, field)?
            };
            values.push(Value::Record(BTreeMap::from([
                ("key".to_owned(), Value::Number(row.key as i64)),
                (
                    "generation".to_owned(),
                    Value::Number(row.generation as i64),
                ),
                ("value".to_owned(), value),
            ])));
        }
        Ok(Value::List(values))
    }

    pub fn row_target_for_source(
        &self,
        source: SourceId,
        key: u64,
        generation: u64,
    ) -> Result<RowId, Error> {
        let route = self.metadata.routes.get(&source).ok_or_else(|| {
            Error::InvalidEvent(format!("source {} is not in the plan", source.0))
        })?;
        let scope = route
            .scope_id
            .ok_or_else(|| Error::InvalidEvent(format!("source {} is not row-scoped", source.0)))?;
        let list = *self.metadata.list_by_scope.get(&scope).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "source {} scope {} has no owning list",
                source.0, scope.0
            ))
        })?;
        let row = RowId {
            list,
            key,
            generation,
        };
        if !self
            .lists
            .get(&list)
            .is_some_and(|list| list.rows.contains_key(&row))
        {
            return Err(Error::InvalidEvent(format!(
                "source {} target row {}:{}:{} does not exist",
                source.0, list.0, key, generation
            )));
        }
        Ok(row)
    }

    pub fn row_target_for_source_path(
        &self,
        path: &str,
        key: u64,
        generation: u64,
    ) -> Result<RowId, Error> {
        let source = self
            .metadata
            .routes
            .values()
            .find(|route| route.path == path)
            .map(|route| route.source_id)
            .ok_or_else(|| {
                Error::InvalidEvent(format!("source path `{path}` is not in the plan"))
            })?;
        self.row_target_for_source(source, key, generation)
    }

    fn initialize_storage_defaults(&mut self, work: &mut Work) -> Result<(), Error> {
        for field in self.metadata.root_computations.keys() {
            self.root_fields.insert(*field, DerivedCell::default());
        }
        for slot in &self.plan.storage_layout.scalar_slots {
            if slot.indexed || slot.initial_value_kind == InitialValueKind::RootInitialField {
                continue;
            }
            let value = self.initial_slot_value(slot)?;
            self.root_states.insert(slot.state_id, value);
        }
        for slot in self.plan.storage_layout.list_slots.clone() {
            self.initialize_list(&slot, work)?;
        }
        let initial_rows = self
            .lists
            .values()
            .flat_map(|list| list.order.iter().copied())
            .collect::<Vec<_>>();
        for row in initial_rows {
            self.initialize_missing_indexed_states(row, work)?;
        }

        Ok(())
    }

    fn initialize_root_field_defaults(&mut self, work: &mut Work) -> Result<(), Error> {
        let root_initial_slots = self
            .plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| {
                !slot.indexed && slot.initial_value_kind == InitialValueKind::RootInitialField
            })
            .cloned()
            .collect::<Vec<_>>();
        for slot in root_initial_slots {
            let source = slot.initial_root_field_path.as_deref().ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "root initial state {} has no source field",
                    slot.state_id.0
                ))
            })?;
            let field = self.metadata.root_field(source)?;
            if !self.root_states.contains_key(&slot.state_id) {
                let value = self.ensure_root_field(field, None, work)?;
                self.root_states.insert(slot.state_id, value);
            }
        }
        Ok(())
    }

    fn install_authority(
        &mut self,
        authority: AuthoritySnapshot,
        work: &mut Work,
    ) -> Result<(), Error> {
        self.turn_sequence = authority.through_turn_sequence;
        self.touched_root_states.clear();
        for (state, scalar) in authority.states {
            if !self
                .plan
                .storage_layout
                .scalar_slots
                .iter()
                .any(|slot| !slot.indexed && slot.state_id == state)
            {
                return Err(Error::InvalidPlan(format!(
                    "restore image contains unknown root state {}",
                    state.0
                )));
            }
            if scalar.touched {
                self.root_states.insert(state, scalar.value);
                self.touched_root_states.insert(state);
            }
        }

        for (list_id, restored) in authority.lists {
            let slot = self
                .plan
                .storage_layout
                .list_slots
                .iter()
                .find(|slot| slot.list_id == list_id)
                .cloned()
                .ok_or_else(|| {
                    Error::InvalidPlan(format!("restore image contains unknown list {}", list_id.0))
                })?;
            let allowed_fields = self.authority_fields_for_list(list_id);
            if !restored.touched {
                if restored.next_key != 0 {
                    return Err(Error::InvalidPlan(format!(
                        "restore row overrides for list {} replace allocator state",
                        list_id.0
                    )));
                }
                let mut seen = BTreeSet::new();
                for restored_row in restored.rows {
                    if restored_row.id.list != list_id || !seen.insert(restored_row.id) {
                        return Err(Error::InvalidPlan(format!(
                            "restore image contains invalid or repeated row {}:{}:{}",
                            restored_row.id.list.0, restored_row.id.key, restored_row.id.generation
                        )));
                    }
                    if restored_row.touched_fields.is_empty()
                        || restored_row.fields.keys().any(|field| {
                            !allowed_fields.contains(field)
                                || !restored_row.touched_fields.contains(field)
                        })
                        || restored_row
                            .touched_fields
                            .iter()
                            .any(|field| !restored_row.fields.contains_key(field))
                    {
                        return Err(Error::InvalidPlan(format!(
                            "restore row override {}:{} contains non-authoritative or untouched data",
                            restored_row.id.key, restored_row.id.generation
                        )));
                    }
                    let row = self
                        .lists
                        .get_mut(&list_id)
                        .and_then(|list| list.rows.get_mut(&restored_row.id))
                        .ok_or_else(|| {
                            Error::InvalidPlan(format!(
                                "restore row override {}:{} does not exist in current defaults",
                                restored_row.id.key, restored_row.id.generation
                            ))
                        })?;
                    for (field, value) in restored_row.fields {
                        row.fields.insert(field, value);
                        self.touched_row_fields.insert((restored_row.id, field));
                    }
                }
                continue;
            }
            let mut rows = BTreeMap::new();
            let mut order = Vec::with_capacity(restored.rows.len());
            let mut seen = BTreeSet::new();
            for restored_row in restored.rows {
                work.consume(1)?;
                if restored_row.id.list != list_id {
                    return Err(Error::InvalidPlan(format!(
                        "restore row {}:{} belongs to list {}, expected {}",
                        restored_row.id.key,
                        restored_row.id.generation,
                        restored_row.id.list.0,
                        list_id.0
                    )));
                }
                if !seen.insert(restored_row.id) {
                    return Err(Error::InvalidPlan(format!(
                        "restore image repeats row {}:{}:{}",
                        list_id.0, restored_row.id.key, restored_row.id.generation
                    )));
                }
                if let Some(field) = restored_row
                    .fields
                    .keys()
                    .find(|field| !allowed_fields.contains(field))
                {
                    return Err(Error::InvalidPlan(format!(
                        "restore row {}:{} contains non-authoritative field {}",
                        list_id.0, restored_row.id.key, field.0
                    )));
                }
                if restored_row
                    .touched_fields
                    .iter()
                    .any(|field| !restored_row.fields.contains_key(field))
                {
                    return Err(Error::InvalidPlan(format!(
                        "restore row {}:{} touches a field without a value",
                        list_id.0, restored_row.id.key
                    )));
                }
                let mut row = Row {
                    fields: restored_row.fields,
                    ..Row::default()
                };
                for field in self.metadata.row_computations.keys() {
                    if self.metadata.row_field_owner.get(field) == Some(&list_id) {
                        row.derived.insert(*field, Currentness::Dirty);
                    }
                }
                self.touched_row_fields.extend(
                    restored_row
                        .touched_fields
                        .into_iter()
                        .map(|field| (restored_row.id, field)),
                );
                order.push(restored_row.id);
                rows.insert(restored_row.id, row);
            }
            let minimum_next = order
                .iter()
                .map(|row| row.key.saturating_add(1))
                .max()
                .unwrap_or(1);
            if restored.next_key < minimum_next {
                return Err(Error::InvalidPlan(format!(
                    "restore list {} next key {} is below required {}",
                    list_id.0, restored.next_key, minimum_next
                )));
            }
            self.lists.insert(
                list_id,
                ListState {
                    rows,
                    order,
                    next_key: restored.next_key,
                },
            );
            self.touched_lists.insert(list_id);

            for row in self.list_row_ids(list_id) {
                self.initialize_missing_indexed_states(row, work)?;
            }
            let _ = slot;
        }

        self.rebuild_runtime_state(work)?;
        Ok(())
    }

    fn initialize_missing_indexed_states(
        &mut self,
        row: RowId,
        work: &mut Work,
    ) -> Result<(), Error> {
        let existing = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .map(|row| row.fields.keys().copied().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        let slots = self
            .plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| self.metadata.indexed_state_owner.get(&slot.state_id) == Some(&row.list))
            .filter(|slot| {
                self.metadata
                    .indexed_state_field
                    .get(&slot.state_id)
                    .is_none_or(|field| !existing.contains(field))
            })
            .cloned()
            .collect::<Vec<_>>();
        for slot in slots {
            self.initialize_indexed_state(row, &slot, work)?;
        }
        Ok(())
    }

    fn rebuild_runtime_state(&mut self, work: &mut Work) -> Result<(), Error> {
        self.indexes.clear();
        self.dynamic_dependencies = DynamicDependencies::default();
        self.next_binding_id = 1;
        let rows = self
            .lists
            .iter()
            .flat_map(|(list, state)| state.order.iter().map(|row| (*list, *row)))
            .collect::<Vec<_>>();
        work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
        for (_, row) in &rows {
            if let Some(state) = self
                .lists
                .get_mut(&row.list)
                .and_then(|list| list.rows.get_mut(row))
            {
                state.bindings.clear();
                for currentness in state.derived.values_mut() {
                    *currentness = Currentness::Dirty;
                }
            }
        }
        for (_, row) in rows {
            self.index_row(row)?;
            let scope = self
                .plan
                .storage_layout
                .list_slots
                .iter()
                .find(|slot| slot.list_id == row.list)
                .and_then(|slot| slot.scope_id);
            self.bind_row_sources(row, scope)?;
        }
        for cell in self.root_fields.values_mut() {
            cell.currentness = Currentness::Dirty;
            cell.value = None;
        }
        Ok(())
    }

    fn initialize_list(&mut self, slot: &ListStorageSlot, work: &mut Work) -> Result<(), Error> {
        self.lists.entry(slot.list_id).or_default();
        for (index, initial) in slot.initial_rows.iter().enumerate() {
            let fields = initial
                .fields
                .iter()
                .map(|field| {
                    let id = field.field_id.ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "list {} initial field `{}` has no FieldId",
                            slot.list_id.0, field.name
                        ))
                    })?;
                    Ok((id, constant_value(&field.value)?))
                })
                .collect::<Result<BTreeMap<_, _>, Error>>()?;
            self.insert_initial_row(slot, index as u64 + 1, fields, work)?;
        }
        if slot.initializer_kind == ListInitializerKind::Range {
            let range = slot.range.ok_or_else(|| {
                Error::InvalidPlan(format!("list {} range has no bounds", slot.list_id.0))
            })?;
            if range.from <= range.to {
                let index_field = self.metadata.list_storage_field(slot.list_id, "index")?;
                let value_field = self.metadata.list_storage_field(slot.list_id, "value")?;
                for (offset, value) in (range.from..=range.to).enumerate() {
                    let text = Value::Text(value.to_string());
                    self.insert_initial_row(
                        slot,
                        offset as u64 + 1,
                        BTreeMap::from([(index_field, text.clone()), (value_field, text)]),
                        work,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn insert_initial_row(
        &mut self,
        slot: &ListStorageSlot,
        key: u64,
        fields: BTreeMap<FieldId, Value>,
        work: &mut Work,
    ) -> Result<RowId, Error> {
        work.consume(1)?;
        let row_id = RowId {
            list: slot.list_id,
            key,
            generation: 1,
        };
        let mut row = Row {
            fields,
            ..Row::default()
        };
        for field in self.metadata.row_computations.keys() {
            if self.metadata.row_field_owner.get(field) == Some(&slot.list_id) {
                row.derived.insert(*field, Currentness::Dirty);
            }
        }
        let list = self.lists.get_mut(&slot.list_id).ok_or_else(|| {
            Error::Evaluation(format!("list {} was not initialized", slot.list_id.0))
        })?;
        list.next_key = list.next_key.max(key.saturating_add(1));
        list.order.push(row_id);
        list.rows.insert(row_id, row);
        self.index_row(row_id)?;
        self.bind_row_sources(row_id, slot.scope_id)?;
        Ok(row_id)
    }

    fn initialize_indexed_states(&mut self, row: RowId, work: &mut Work) -> Result<(), Error> {
        let slots = self
            .plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| self.metadata.indexed_state_owner.get(&slot.state_id) == Some(&row.list))
            .cloned()
            .collect::<Vec<_>>();
        for slot in slots {
            self.initialize_indexed_state(row, &slot, work)?;
        }
        Ok(())
    }

    fn initialize_indexed_state(
        &mut self,
        row: RowId,
        slot: &ScalarStorageSlot,
        work: &mut Work,
    ) -> Result<(), Error> {
        let target = *self
            .metadata
            .indexed_state_field
            .get(&slot.state_id)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "indexed state {} has no row field",
                    slot.state_id.0
                ))
            })?;
        let value = match slot.initial_value_kind {
            InitialValueKind::RowInitialField => {
                if let Some(expression) = &slot.initial_row_expression {
                    let mut bindings = BTreeMap::new();
                    let evaluated = self.eval_row_expression(
                        expression,
                        Some(row),
                        None,
                        Some(target),
                        None,
                        &mut bindings,
                        work,
                    )?;
                    self.materialize_eval(evaluated)?
                } else {
                    let source = slot.initial_row_field_path.as_deref().ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "indexed state {} has no row initial source",
                            slot.state_id.0
                        ))
                    })?;
                    let source = self.metadata.list_field(row.list, source)?;
                    self.ensure_row_field(row, source, None, work)?
                }
            }
            _ => self.initial_slot_value(slot)?,
        };
        self.set_row_field(row, target, value, work)?;
        Ok(())
    }

    fn initial_slot_value(&self, slot: &ScalarStorageSlot) -> Result<Value, Error> {
        let constant = slot.initial_constant_id.ok_or_else(|| {
            Error::InvalidPlan(format!(
                "state {} {:?} initializer has no constant",
                slot.state_id.0, slot.initial_value_kind
            ))
        })?;
        self.metadata
            .constants
            .get(&constant)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("missing constant {}", constant.0)))
    }
}

impl Session {
    pub fn apply(&mut self, event: SourceEvent) -> Result<Turn, Error> {
        self.apply_with_demand(event, &[])
    }

    pub fn apply_with_demand(
        &mut self,
        event: SourceEvent,
        demanded_targets: &[ValueTarget],
    ) -> Result<Turn, Error> {
        let mut work = std::mem::take(&mut self.turn_work);
        if work.pending_settle {
            work.settle();
        }
        work.begin_turn(self.last_sequence, self.turn_sequence);
        let result = self.apply_with_work(event, demanded_targets, &mut work);
        let result = match result {
            Ok(turn) => {
                work.pending_settle = true;
                Ok(turn)
            }
            Err(error) => match self.rollback_turn(&mut work) {
                Ok(()) => {
                    self.last_sequence = work.previous_last_sequence;
                    self.turn_sequence = work.previous_turn_sequence;
                    work.pending_settle = false;
                    Err(error)
                }
                Err(rollback) => Err(Error::Evaluation(format!(
                    "turn failed with `{error}` and rollback failed with `{rollback}`"
                ))),
            },
        };
        self.turn_work = work;
        result
    }

    pub fn begin_effect_dispatch(
        &mut self,
        item: &boon_persistence::DurableOutboxItem,
    ) -> Result<Turn, Error> {
        let attempt = match item.state {
            boon_persistence::DurableOutboxState::Pending => 1,
            boon_persistence::DurableOutboxState::ReconciliationRequired { attempt } => attempt
                .checked_add(1)
                .ok_or_else(|| Error::Evaluation("effect attempt overflow".to_owned()))?,
            _ => {
                return Err(Error::Evaluation(format!(
                    "outbox item {} is not ready for dispatch",
                    item.item_id
                )));
            }
        };
        let sequence = self.next_internal_turn_sequence()?;
        self.finish_outbox_only_turn(boon_persistence::DurableOutboxChange::BeginDispatch {
            item_id: item.item_id,
            expected_revision: item.revision,
            next_revision: item
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::Evaluation("outbox item revision overflow".to_owned()))?,
            attempt,
            turn_sequence: sequence,
        })
    }

    pub fn require_effect_reconciliation(
        &mut self,
        item: &boon_persistence::DurableOutboxItem,
    ) -> Result<Turn, Error> {
        let boon_persistence::DurableOutboxState::Dispatching { attempt } = item.state else {
            return Err(Error::Evaluation(format!(
                "outbox item {} is not dispatching",
                item.item_id
            )));
        };
        let sequence = self.next_internal_turn_sequence()?;
        self.finish_outbox_only_turn(
            boon_persistence::DurableOutboxChange::RequireReconciliation {
                item_id: item.item_id,
                expected_revision: item.revision,
                next_revision: item
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| Error::Evaluation("outbox item revision overflow".to_owned()))?,
                attempt,
                turn_sequence: sequence,
            },
        )
    }

    pub fn complete_effect(
        &mut self,
        item: &boon_persistence::DurableOutboxItem,
        outcome: boon_persistence::StoredValue,
    ) -> Result<Turn, Error> {
        let attempt = match item.state {
            boon_persistence::DurableOutboxState::Dispatching { attempt }
            | boon_persistence::DurableOutboxState::ReconciliationRequired { attempt } => attempt,
            _ => {
                return Err(Error::Evaluation(format!(
                    "outbox item {} is not awaiting an outcome",
                    item.item_id
                )));
            }
        };
        let (op, effect) = self.effect_invocation(item.invocation_id)?;
        if effect.effect_id != item.effect_id {
            return Err(Error::InvalidPlan(format!(
                "outbox item {} effect does not match invocation {}",
                item.item_id, item.invocation_id
            )));
        }
        let mut completed = item.clone();
        completed.revision = item
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("outbox item revision overflow".to_owned()))?;
        completed.state = boon_persistence::DurableOutboxState::Completed {
            attempt,
            outcome: outcome.clone(),
        };
        let sequence = self.next_internal_turn_sequence()?;
        completed.updated_turn_sequence = sequence;
        let schema = self
            .plan
            .persistence
            .effect_outbox
            .iter()
            .find(|schema| schema.effect_id == item.effect_id)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "effect {} has no durable outbox schema",
                    item.effect_id
                ))
            })?;
        boon_persistence::validate_outbox_item_schema(&completed, schema)
            .map_err(|error| Error::InvalidPlan(error.to_string()))?;

        let mut work = self.take_internal_turn_work();
        let result = (|| {
            let row = self.runtime_row_for_effect(item.target_row)?;
            self.apply_effect_outcome(
                &op,
                &effect.result,
                row,
                outcome.clone(),
                sequence,
                &mut work,
            )?;
            let durable_changes = self.durable_changes(&work.authority_deltas)?;
            work.outbox_changes
                .push(boon_persistence::DurableOutboxChange::Complete {
                    item_id: item.item_id,
                    expected_revision: item.revision,
                    next_revision: completed.revision,
                    attempt,
                    outcome,
                    turn_sequence: sequence,
                });
            self.turn_sequence = sequence;
            work.finish_metrics();
            let turn = Turn {
                sequence,
                source_sequence: None,
                deltas: coalesce_deltas(std::mem::take(&mut work.deltas)),
                authority_deltas: std::mem::take(&mut work.authority_deltas),
                durable_changes,
                outbox_changes: std::mem::take(&mut work.outbox_changes),
                metrics: std::mem::take(&mut work.metrics),
            };
            work.pending_settle = true;
            Ok(turn)
        })();
        self.finish_internal_turn_work(work, result)
    }

    fn next_internal_turn_sequence(&self) -> Result<u64, Error> {
        self.turn_sequence
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("authority turn sequence overflow".to_owned()))
    }

    fn finish_outbox_only_turn(
        &mut self,
        change: boon_persistence::DurableOutboxChange,
    ) -> Result<Turn, Error> {
        let sequence = self.next_internal_turn_sequence()?;
        let mut work = self.take_internal_turn_work();
        work.outbox_changes.push(change);
        self.turn_sequence = sequence;
        work.finish_metrics();
        let turn = Turn {
            sequence,
            source_sequence: None,
            deltas: Vec::new(),
            authority_deltas: Vec::new(),
            durable_changes: Vec::new(),
            outbox_changes: std::mem::take(&mut work.outbox_changes),
            metrics: std::mem::take(&mut work.metrics),
        };
        work.pending_settle = true;
        self.turn_work = work;
        Ok(turn)
    }

    fn take_internal_turn_work(&mut self) -> Work {
        let mut work = std::mem::take(&mut self.turn_work);
        if work.pending_settle {
            work.settle();
        }
        work.begin_turn(self.last_sequence, self.turn_sequence);
        work
    }

    fn finish_internal_turn_work(
        &mut self,
        mut work: Work,
        result: Result<Turn, Error>,
    ) -> Result<Turn, Error> {
        let result = match result {
            Ok(turn) => Ok(turn),
            Err(error) => match self.rollback_turn(&mut work) {
                Ok(()) => {
                    self.last_sequence = work.previous_last_sequence;
                    self.turn_sequence = work.previous_turn_sequence;
                    work.pending_settle = false;
                    Err(error)
                }
                Err(rollback) => Err(Error::Evaluation(format!(
                    "effect outcome failed with `{error}` and rollback failed with `{rollback}`"
                ))),
            },
        };
        self.turn_work = work;
        result
    }

    fn effect_invocation(
        &self,
        invocation_id: EffectInvocationId,
    ) -> Result<(PlanOp, boon_plan::EffectInvocationPlan), Error> {
        self.plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .find_map(|op| {
                let PlanOpKind::UpdateBranch {
                    effect: Some(effect),
                    ..
                } = &op.kind
                else {
                    return None;
                };
                (effect.invocation_id == invocation_id).then(|| (op.clone(), effect.clone()))
            })
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "outbox invocation {invocation_id} is absent from the active plan"
                ))
            })
    }

    fn runtime_row_for_effect(
        &self,
        target: Option<boon_persistence::DurableEffectRow>,
    ) -> Result<Option<RowId>, Error> {
        let Some(target) = target else {
            return Ok(None);
        };
        let memory = self
            .plan
            .persistence
            .lists
            .iter()
            .find(|memory| memory.memory_id == target.list_memory_id)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "effect target list {} is absent from the active plan",
                    target.list_memory_id
                ))
            })?;
        let slot = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.id == memory.runtime_slot)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "effect target list {} has no runtime slot",
                    target.list_memory_id
                ))
            })?;
        let row = RowId {
            list: slot.list_id,
            key: target.row_key,
            generation: target.row_generation,
        };
        if !self
            .lists
            .get(&row.list)
            .is_some_and(|list| list.rows.contains_key(&row))
        {
            return Err(Error::Evaluation(format!(
                "effect target row {}:{}:{} no longer exists",
                row.list.0, row.key, row.generation
            )));
        }
        Ok(Some(row))
    }

    fn apply_effect_outcome(
        &mut self,
        op: &PlanOp,
        route: &boon_plan::EffectResultRoute,
        row: Option<RowId>,
        outcome: boon_persistence::StoredValue,
        sequence: u64,
        work: &mut Work,
    ) -> Result<(), Error> {
        match route {
            boon_plan::EffectResultRoute::Target { target, .. } => {
                let value = runtime_value(outcome)?;
                self.apply_effect_result(op, target, row, value, work)
            }
            boon_plan::EffectResultRoute::CorrelatedSources { variants } => {
                let boon_persistence::StoredValue::Variant { tag, fields } = outcome else {
                    return Err(Error::InvalidPlan(format!(
                        "correlated effect invocation {} completed with a non-variant outcome",
                        op.id.0
                    )));
                };
                let mut matching = variants.iter().filter(|route| route.tag == tag);
                let route = matching.next().ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "correlated effect invocation {} has no SOURCE route for `{tag}`",
                        op.id.0
                    ))
                })?;
                if matching.next().is_some() {
                    return Err(Error::InvalidPlan(format!(
                        "correlated effect invocation {} has multiple SOURCE routes for `{tag}`",
                        op.id.0
                    )));
                }
                let source = self
                    .metadata
                    .routes
                    .get(&route.source_id)
                    .cloned()
                    .ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "correlated effect result SOURCE {} is absent from the plan",
                            route.source_id.0
                        ))
                    })?;
                let expected_fields = source
                    .payload_schema
                    .typed_fields
                    .iter()
                    .map(|field| source_payload_schema_field_name(&field.field).to_owned())
                    .collect::<BTreeSet<_>>();
                if expected_fields != fields.keys().cloned().collect() {
                    return Err(Error::InvalidPlan(format!(
                        "correlated effect result `{tag}` fields differ from SOURCE {} payload schema",
                        route.source_id.0
                    )));
                }
                let mut payload = SourcePayload::default();
                for (name, value) in fields {
                    set_source_payload_value(
                        &mut payload,
                        &source_payload_field_from_effect_name(&name),
                        runtime_value(value)?,
                    )?;
                }
                let target = if source.scoped {
                    Some(row.ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "scoped correlated effect result SOURCE {} has no durable row",
                            route.source_id.0
                        ))
                    })?)
                } else {
                    None
                };
                let mut event = SourceEvent {
                    sequence,
                    source: route.source_id,
                    target,
                    payload,
                };
                self.validate_event_route(&event, true)?;
                self.route_event_with_work(&mut event, &[], work)
            }
        }
    }

    fn apply_effect_result(
        &mut self,
        op: &PlanOp,
        target: &ValueRef,
        row: Option<RowId>,
        value: Value,
        work: &mut Work,
    ) -> Result<(), Error> {
        let ValueRef::State(state) = target else {
            return Err(Error::InvalidPlan(format!(
                "effect invocation {} has a non-state result target",
                op.id.0
            )));
        };
        if op.indexed {
            let row = row.ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "indexed effect invocation {} has no durable row target",
                    op.id.0
                ))
            })?;
            let field = *self
                .metadata
                .indexed_state_field
                .get(state)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!("indexed state {} has no FieldId", state.0))
                })?;
            self.record_row_field_undo(row, field, work);
            self.touched_row_fields.insert((row, field));
            work.authority_deltas.push(AuthorityDelta::SetRowField {
                row,
                field,
                value: value.clone(),
            });
            self.set_row_field(row, field, value, work)?;
        } else {
            if row.is_some() {
                return Err(Error::InvalidPlan(format!(
                    "root effect invocation {} unexpectedly carries a row target",
                    op.id.0
                )));
            }
            self.record_root_undo(*state, work);
            self.touched_root_states.insert(*state);
            work.authority_deltas.push(AuthorityDelta::SetRoot {
                state: *state,
                value: value.clone(),
            });
            self.set_root_state(*state, value, work);
        }
        Ok(())
    }

    pub fn settle_turn(&mut self) {
        self.turn_work.settle();
    }

    pub fn rollback_unsettled_turn(&mut self) -> Result<(), Error> {
        if !self.turn_work.pending_settle {
            return Ok(());
        }
        let mut work = std::mem::take(&mut self.turn_work);
        let previous_last_sequence = work.previous_last_sequence;
        let previous_turn_sequence = work.previous_turn_sequence;
        let result = self.rollback_turn(&mut work);
        if result.is_ok() {
            self.last_sequence = previous_last_sequence;
            self.turn_sequence = previous_turn_sequence;
            work.pending_settle = false;
        }
        self.turn_work = work;
        result
    }

    fn record_root_undo(&self, state: StateId, work: &mut Work) {
        if work.undo_root_states.insert(state) {
            work.authority_undo.push(AuthorityUndo::RootState {
                state,
                value: self.root_states.get(&state).cloned(),
                touched: self.touched_root_states.contains(&state),
            });
        }
    }

    fn record_row_field_undo(&self, row: RowId, field: FieldId, work: &mut Work) {
        if work.undo_row_fields.insert((row, field)) {
            work.authority_undo.push(AuthorityUndo::RowField {
                row,
                field,
                value: self
                    .lists
                    .get(&row.list)
                    .and_then(|list| list.rows.get(&row))
                    .and_then(|row| row.fields.get(&field))
                    .cloned(),
                touched_field: self.touched_row_fields.contains(&(row, field)),
                touched_list: self.touched_lists.contains(&row.list),
            });
        }
    }

    fn rollback_turn(&mut self, work: &mut Work) -> Result<(), Error> {
        work.allow_rollback();
        for undo in work.authority_undo.drain(..).rev() {
            match undo {
                AuthorityUndo::RootState {
                    state,
                    value,
                    touched,
                } => {
                    match value {
                        Some(value) => {
                            self.root_states.insert(state, value);
                        }
                        None => {
                            self.root_states.remove(&state);
                        }
                    }
                    if touched {
                        self.touched_root_states.insert(state);
                    } else {
                        self.touched_root_states.remove(&state);
                    }
                }
                AuthorityUndo::RowField {
                    row,
                    field,
                    value,
                    touched_field,
                    touched_list,
                } => {
                    let fields = &mut self
                        .lists
                        .get_mut(&row.list)
                        .and_then(|list| list.rows.get_mut(&row))
                        .ok_or_else(|| {
                            Error::Evaluation(format!(
                                "rollback cannot find row {}:{}:{}",
                                row.list.0, row.key, row.generation
                            ))
                        })?
                        .fields;
                    match value {
                        Some(value) => {
                            fields.insert(field, value);
                        }
                        None => {
                            fields.remove(&field);
                        }
                    }
                    if touched_field {
                        self.touched_row_fields.insert((row, field));
                    } else {
                        self.touched_row_fields.remove(&(row, field));
                    }
                    if touched_list {
                        self.touched_lists.insert(row.list);
                    } else {
                        self.touched_lists.remove(&row.list);
                    }
                }
                AuthorityUndo::AppendRow {
                    row,
                    previous_next_key,
                    touched_list,
                } => {
                    let list = self.lists.get_mut(&row.list).ok_or_else(|| {
                        Error::Evaluation(format!("rollback list {} is missing", row.list.0))
                    })?;
                    list.order.retain(|candidate| *candidate != row);
                    list.rows.remove(&row);
                    list.next_key = previous_next_key;
                    self.touched_row_fields
                        .retain(|(candidate, _)| *candidate != row);
                    if touched_list {
                        self.touched_lists.insert(row.list);
                    } else {
                        self.touched_lists.remove(&row.list);
                    }
                }
                AuthorityUndo::RemoveRow {
                    row,
                    value,
                    order_index,
                    previous_next_key,
                    touched_list,
                    touched_fields,
                } => {
                    let list = self.lists.get_mut(&row.list).ok_or_else(|| {
                        Error::Evaluation(format!("rollback list {} is missing", row.list.0))
                    })?;
                    let index = order_index.min(list.order.len());
                    list.order.insert(index, row);
                    list.rows.insert(row, value);
                    list.next_key = previous_next_key;
                    self.touched_row_fields
                        .retain(|(candidate, _)| *candidate != row);
                    self.touched_row_fields
                        .extend(touched_fields.into_iter().map(|field| (row, field)));
                    if touched_list {
                        self.touched_lists.insert(row.list);
                    } else {
                        self.touched_lists.remove(&row.list);
                    }
                }
            }
        }
        work.undo_root_states.clear();
        work.undo_row_fields.clear();
        work.deltas.clear();
        work.authority_deltas.clear();
        work.outbox_changes.clear();
        work.emit = false;
        self.rebuild_runtime_state(work)?;
        self.ensure_published_current(None, work)?;
        Ok(())
    }

    fn apply_with_work(
        &mut self,
        mut event: SourceEvent,
        demanded_targets: &[ValueTarget],
        work: &mut Work,
    ) -> Result<Turn, Error> {
        self.validate_event(&event)?;
        self.route_event_with_work(&mut event, demanded_targets, work)?;
        let durable_changes = self.durable_changes(&work.authority_deltas)?;

        self.last_sequence = Some(event.sequence);
        self.turn_sequence = self
            .turn_sequence
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("authority turn sequence overflow".to_owned()))?;
        work.finish_metrics();
        Ok(Turn {
            sequence: self.turn_sequence,
            source_sequence: Some(event.sequence),
            deltas: coalesce_deltas(std::mem::take(&mut work.deltas)),
            authority_deltas: std::mem::take(&mut work.authority_deltas),
            durable_changes,
            outbox_changes: std::mem::take(&mut work.outbox_changes),
            metrics: std::mem::take(&mut work.metrics),
        })
    }

    fn route_event_with_work(
        &mut self,
        event: &mut SourceEvent,
        demanded_targets: &[ValueTarget],
        work: &mut Work,
    ) -> Result<(), Error> {
        self.complete_target_payload(event, work)?;
        let targets = self.event_targets(event, work)?;
        let metadata = Arc::clone(&self.metadata);

        if let Some(source_fields) = metadata.source_derived_by_source.get(&event.source) {
            for field in source_fields {
                self.mark_root_dirty(*field, work);
            }
            for field in source_fields {
                self.ensure_root_field(*field, Some(event), work)?;
            }
        }

        let scoped_update_row = metadata
            .routes
            .get(&event.source)
            .and_then(|route| route.scope_id)
            .and_then(|_| event.target.or_else(|| targets.first().copied()));
        if let Some(updates) = metadata.updates_by_source.get(&event.source) {
            for op in updates {
                if op.indexed {
                    let rows = self.indexed_update_targets(op, event, &targets)?;
                    self.execute_indexed_update_batch(op, &rows, event, work)?;
                } else {
                    self.execute_update(op, scoped_update_row, event, work)?;
                }
            }
        }

        for op in &metadata.mutations {
            self.execute_mutation(op, event, &targets, work)?;
        }

        self.ensure_demanded_current(demanded_targets, Some(event), work)?;
        Ok(())
    }

    fn complete_target_payload(
        &mut self,
        event: &mut SourceEvent,
        work: &mut Work,
    ) -> Result<(), Error> {
        let Some(row) = event.target else {
            return Ok(());
        };
        let route = self
            .metadata
            .routes
            .get(&event.source)
            .ok_or_else(|| Error::InvalidEvent(format!("unknown source {}", event.source.0)))?;
        let Some(field) = route.payload_schema.row_lookup_field_id() else {
            return Ok(());
        };
        if source_payload_value(&event.payload, &SourcePayloadField::Address).is_some() {
            return Ok(());
        }
        if self.metadata.row_field_owner.get(&field) != Some(&row.list) {
            return Err(Error::InvalidPlan(format!(
                "source {} row lookup field {} does not belong to target list {}",
                event.source.0, field.0, row.list.0
            )));
        }
        let value = if self.metadata.row_computations.contains_key(&field) {
            self.ensure_row_field(row, field, None, work)?
        } else {
            self.row_value(row, field)?
        };
        set_source_payload_value(&mut event.payload, &SourcePayloadField::Address, value)?;
        Ok(())
    }

    fn validate_event(&self, event: &SourceEvent) -> Result<(), Error> {
        if self.options.require_monotonic_sequences
            && self
                .last_sequence
                .is_some_and(|last| event.sequence <= last)
        {
            return Err(Error::InvalidEvent(format!(
                "sequence {} must be greater than {}",
                event.sequence,
                self.last_sequence.unwrap_or_default()
            )));
        }
        self.validate_event_route(event, false)
    }

    fn validate_event_route(
        &self,
        event: &SourceEvent,
        internal_effect_completion: bool,
    ) -> Result<(), Error> {
        if !self.metadata.routes.contains_key(&event.source) {
            return Err(Error::InvalidEvent(format!(
                "source {} is not in the plan",
                event.source.0
            )));
        }
        if !internal_effect_completion
            && self
                .metadata
                .internal_effect_result_sources
                .contains(&event.source)
        {
            return Err(Error::InvalidEvent(format!(
                "source {} is reserved for correlated host-effect completion",
                event.source.0
            )));
        }
        if let Some(row) = event.target {
            let exists = self
                .lists
                .get(&row.list)
                .is_some_and(|list| list.rows.contains_key(&row));
            if !exists {
                return Err(Error::InvalidEvent(format!(
                    "target row {}:{}:{} does not exist",
                    row.list.0, row.key, row.generation
                )));
            }
        }
        Ok(())
    }

    fn event_targets(&mut self, event: &SourceEvent, work: &mut Work) -> Result<Vec<RowId>, Error> {
        if let Some(row) = event.target {
            return Ok(vec![row]);
        }
        let route = self
            .metadata
            .routes
            .get(&event.source)
            .cloned()
            .ok_or_else(|| Error::InvalidEvent(format!("unknown source {}", event.source.0)))?;
        let Some(scope) = route.scope_id else {
            return Ok(Vec::new());
        };
        let list = *self.metadata.list_by_scope.get(&scope).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "source {} scope {} has no list",
                event.source.0, scope.0
            ))
        })?;
        let Some(field) = route.payload_schema.row_lookup_field_id() else {
            return Ok(Vec::new());
        };
        if self.metadata.row_field_owner.get(&field) != Some(&list) {
            return Err(Error::InvalidPlan(format!(
                "source {} row lookup field {} does not belong to scoped list {}",
                event.source.0, field.0, list.0
            )));
        }
        let Some(value) = source_payload_value(&event.payload, &SourcePayloadField::Address) else {
            return Ok(Vec::new());
        };
        self.lookup_index(list, field, &value, None, work)
    }

    fn indexed_update_targets(
        &self,
        op: &PlanOp,
        event: &SourceEvent,
        scoped_targets: &[RowId],
    ) -> Result<Vec<RowId>, Error> {
        let Some(ValueRef::State(state)) = op.output else {
            return Err(Error::InvalidPlan(format!(
                "indexed update op {} has no state output",
                op.id.0
            )));
        };
        let owner = *self
            .metadata
            .indexed_state_owner
            .get(&state)
            .ok_or_else(|| Error::InvalidPlan(format!("indexed state {} has no owner", state.0)))?;
        if let Some(target) = event.target {
            return Ok((target.list == owner)
                .then_some(target)
                .into_iter()
                .collect());
        }
        let route = self
            .metadata
            .routes
            .get(&event.source)
            .ok_or_else(|| Error::InvalidEvent(format!("unknown source {}", event.source.0)))?;
        if route.scope_id.is_some() {
            return Ok(scoped_targets
                .iter()
                .copied()
                .filter(|row| row.list == owner)
                .collect());
        }
        Ok(self.list_row_ids(owner))
    }

    fn execute_update(
        &mut self,
        op: &PlanOp,
        row: Option<RowId>,
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<(), Error> {
        let PlanOpKind::UpdateBranch { source_guard, .. } = &op.kind else {
            return Err(Error::InvalidPlan(format!(
                "update region op {} is not an update branch",
                op.id.0
            )));
        };
        if !self.source_guard_matches(source_guard.as_ref(), event)? {
            return Ok(());
        }
        if matches!(
            &op.kind,
            PlanOpKind::UpdateBranch {
                effect: Some(_),
                ..
            }
        ) {
            return self.stage_effect_invocation(op, row, event, work);
        }
        let Some(value) = self.evaluate_update(op, row, event, work)? else {
            return Ok(());
        };
        let Some(ValueRef::State(state)) = op.output else {
            return Err(Error::InvalidPlan(format!(
                "update op {} has no state output",
                op.id.0
            )));
        };
        if op.indexed {
            let row = row.ok_or_else(|| {
                Error::InvalidEvent(format!("indexed update op {} has no row target", op.id.0))
            })?;
            let field = *self
                .metadata
                .indexed_state_field
                .get(&state)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!("indexed state {} has no FieldId", state.0))
                })?;
            self.record_row_field_undo(row, field, work);
            self.touched_row_fields.insert((row, field));
            work.authority_deltas.push(AuthorityDelta::SetRowField {
                row,
                field,
                value: value.clone(),
            });
            self.set_row_field(row, field, value, work)?;
        } else {
            self.record_root_undo(state, work);
            self.touched_root_states.insert(state);
            work.authority_deltas.push(AuthorityDelta::SetRoot {
                state,
                value: value.clone(),
            });
            self.set_root_state(state, value, work);
        }
        Ok(())
    }

    fn execute_indexed_update_batch(
        &mut self,
        op: &PlanOp,
        rows: &[RowId],
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<(), Error> {
        let PlanOpKind::UpdateBranch { source_guard, .. } = &op.kind else {
            return Err(Error::InvalidPlan(format!(
                "update region op {} is not an update branch",
                op.id.0
            )));
        };
        if !self.source_guard_matches(source_guard.as_ref(), event)? {
            return Ok(());
        }
        if matches!(
            &op.kind,
            PlanOpKind::UpdateBranch {
                effect: Some(_),
                ..
            }
        ) {
            for row in rows {
                self.stage_effect_invocation(op, Some(*row), event, work)?;
            }
            return Ok(());
        }
        let Some(ValueRef::State(state)) = op.output else {
            return Err(Error::InvalidPlan(format!(
                "indexed update op {} has no state output",
                op.id.0
            )));
        };
        let field = *self
            .metadata
            .indexed_state_field
            .get(&state)
            .ok_or_else(|| {
                Error::InvalidPlan(format!("indexed state {} has no FieldId", state.0))
            })?;
        let mut pending = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(value) = self.evaluate_update(op, Some(*row), event, work)? {
                pending.push((*row, value));
            }
        }
        for (row, value) in pending {
            self.record_row_field_undo(row, field, work);
            self.touched_row_fields.insert((row, field));
            work.authority_deltas.push(AuthorityDelta::SetRowField {
                row,
                field,
                value: value.clone(),
            });
            self.set_row_field(row, field, value, work)?;
        }
        Ok(())
    }

    fn stage_effect_invocation(
        &mut self,
        op: &PlanOp,
        row: Option<RowId>,
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<(), Error> {
        let PlanOpKind::UpdateBranch {
            expression_kind,
            effect: Some(effect),
            ..
        } = &op.kind
        else {
            return Err(Error::InvalidPlan(format!(
                "update op {} has no effect invocation plan",
                op.id.0
            )));
        };
        if !matches!(
            expression_kind,
            PlanExpressionKind::FileWriteBytes | PlanExpressionKind::HostEffect
        ) {
            return Err(Error::Unsupported {
                op: op.id,
                detail: format!(
                    "effect intent lowering is not implemented for {expression_kind:?}"
                ),
            });
        }
        let intent = boon_persistence::StoredValue::Record(
            effect
                .intent_fields
                .iter()
                .map(|field| {
                    let value = self.eval_update_ref(&field.input, row, event, work)?;
                    Ok((
                        field.name.clone(),
                        stored_value_for_data_type(&value, &field.data_type)?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, Error>>()?,
        );
        let target_row = row
            .map(|row| {
                Ok(boon_persistence::DurableEffectRow {
                    list_memory_id: self.persistence_list(row.list)?.memory_id,
                    row_key: row.key,
                    row_generation: row.generation,
                })
            })
            .transpose()?;
        let sequence = self
            .turn_sequence
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("authority turn sequence overflow".to_owned()))?;
        let idempotency_key = match effect.idempotency_key {
            boon_plan::EffectIdempotencyKeyPlan::InvocationTurnIntentSha256 => {
                boon_persistence::canonical_intent_key(&boon_persistence::StoredValue::Record(
                    BTreeMap::from([
                        (
                            "authority_turn_sequence".to_owned(),
                            boon_persistence::StoredValue::Text(sequence.to_string()),
                        ),
                        ("canonical_intent".to_owned(), intent.clone()),
                        (
                            "invocation_id".to_owned(),
                            boon_persistence::StoredValue::Text(effect.invocation_id.to_string()),
                        ),
                        (
                            "source_sequence".to_owned(),
                            boon_persistence::StoredValue::Text(event.sequence.to_string()),
                        ),
                    ]),
                ))
            }
        };
        let item = boon_persistence::DurableOutboxItem::pending(
            effect.invocation_id,
            effect.effect_id,
            idempotency_key,
            intent,
            target_row,
            sequence,
        );
        let schema = self
            .plan
            .persistence
            .effect_outbox
            .iter()
            .find(|schema| schema.effect_id == effect.effect_id)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "effect {} has no durable outbox schema",
                    effect.effect_id
                ))
            })?;
        boon_persistence::validate_outbox_item_schema(&item, schema)
            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
        work.outbox_changes
            .push(boon_persistence::DurableOutboxChange::Enqueue { item });
        Ok(())
    }

    fn source_guard_matches(
        &self,
        guard: Option<&PlanSourceGuard>,
        event: &SourceEvent,
    ) -> Result<bool, Error> {
        match guard {
            None => Ok(true),
            Some(PlanSourceGuard::SourcePayloadOneOf {
                source_id,
                field,
                values,
            }) => {
                if *source_id != event.source {
                    return Ok(false);
                }
                let Some(value) = source_payload_value(&event.payload, field) else {
                    return Ok(false);
                };
                let text = value_to_text(&value)?;
                Ok(values.contains(&text))
            }
        }
    }

    fn ensure_root_field(
        &mut self,
        field: FieldId,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let currentness = self
            .root_fields
            .get(&field)
            .map(|cell| cell.currentness)
            .ok_or_else(|| {
                Error::InvalidPlan(format!("field {} has no root computation", field.0))
            })?;
        match currentness {
            Currentness::Current => {
                return self
                    .root_fields
                    .get(&field)
                    .and_then(|cell| cell.value.clone())
                    .ok_or_else(|| {
                        Error::Evaluation(format!("current root field {} has no value", field.0))
                    });
            }
            Currentness::Evaluating => return Err(Error::Cycle { field, row: None }),
            Currentness::Dirty => {}
        }
        work.consume(1)?;
        self.root_fields
            .get_mut(&field)
            .expect("root cell checked above")
            .currentness = Currentness::Evaluating;
        let consumer = Consumer::Root(field);
        self.dynamic_dependencies.clear(consumer);
        let op = self
            .metadata
            .root_computations
            .get(&field)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("root field {} has no plan op", field.0)))?;
        let evaluated = self.evaluate_root_computation(field, &op, event, work);
        let value = match evaluated {
            Ok(value) => value,
            Err(error) => {
                self.root_fields
                    .get_mut(&field)
                    .expect("root cell checked above")
                    .currentness = Currentness::Dirty;
                return Err(error);
            }
        };
        let old = self
            .root_fields
            .get(&field)
            .and_then(|cell| cell.value.clone());
        {
            let cell = self.root_fields.get_mut(&field).expect("root cell exists");
            cell.value = Some(value.clone());
            cell.currentness = Currentness::Current;
        }
        work.metrics.recomputed_field_count += 1;
        work.recomputed_targets.insert(ValueTarget::Field(field));
        if old.as_ref() != Some(&value) {
            self.invalidate_root_field(field, work);
            if work.emit {
                work.deltas.push(Delta::SetValue {
                    target: ValueTarget::Field(field),
                    value: value.clone(),
                });
            }
        }
        Ok(value)
    }

    fn ensure_published_current(
        &mut self,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<(), Error> {
        let fields = self.metadata.published.iter().copied().collect::<Vec<_>>();
        for _ in 0..=fields.len() {
            let dirty = fields
                .iter()
                .copied()
                .filter(|field| {
                    self.root_fields
                        .get(field)
                        .is_some_and(|cell| cell.currentness != Currentness::Current)
                })
                .collect::<Vec<_>>();
            if dirty.is_empty() {
                return Ok(());
            }
            for field in dirty {
                self.ensure_root_field(field, event, work)?;
            }
        }
        Err(Error::Evaluation(
            "published fields did not converge at the currentness barrier".to_owned(),
        ))
    }

    fn ensure_demanded_current(
        &mut self,
        targets: &[ValueTarget],
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<(), Error> {
        let max_passes = self
            .metadata
            .published
            .len()
            .saturating_add(targets.len())
            .saturating_add(1);
        for _ in 0..max_passes {
            self.ensure_published_current(event, work)?;
            for target in targets {
                match *target {
                    ValueTarget::State(_) => {}
                    ValueTarget::Field(field) => {
                        self.ensure_root_field(field, event, work)?;
                    }
                    ValueTarget::RowField { row, field }
                        if self.metadata.row_computations.contains_key(&field) =>
                    {
                        if self.row_exists(row) {
                            self.ensure_row_field(row, field, event, work)?;
                        }
                    }
                    ValueTarget::RowField { row, field } => {
                        if self.row_exists(row) {
                            self.row_value(row, field)?;
                        }
                    }
                }
            }
            self.ensure_published_current(event, work)?;
            let all_current = targets.iter().all(|target| match *target {
                ValueTarget::State(_) => true,
                ValueTarget::Field(field) => self
                    .root_fields
                    .get(&field)
                    .is_some_and(|cell| cell.currentness == Currentness::Current),
                ValueTarget::RowField { row, field } => {
                    !self.row_exists(row)
                        || self
                            .lists
                            .get(&row.list)
                            .and_then(|list| list.rows.get(&row))
                            .is_some_and(|row| {
                                row.derived
                                    .get(&field)
                                    .is_none_or(|currentness| *currentness == Currentness::Current)
                            })
                }
            });
            if all_current {
                return Ok(());
            }
        }
        Err(Error::Evaluation(
            "document demands did not converge at the currentness barrier".to_owned(),
        ))
    }

    fn ensure_row_field(
        &mut self,
        row: RowId,
        field: FieldId,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let currentness = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .and_then(|row| row.derived.get(&field))
            .copied();
        let Some(currentness) = currentness else {
            return self.row_value(row, field);
        };
        match currentness {
            Currentness::Current => return self.row_value(row, field),
            Currentness::Evaluating => {
                if self.row_identity_has_raw_value(row, field) {
                    return self.row_value(row, field);
                }
                return Err(Error::Cycle {
                    field,
                    row: Some(row),
                });
            }
            Currentness::Dirty => {}
        }
        work.consume(1)?;
        self.set_row_currentness(row, field, Currentness::Evaluating)?;
        let consumer = Consumer::Row(row, field);
        self.dynamic_dependencies.clear(consumer);
        let op = self
            .metadata
            .row_computations
            .get(&field)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("row field {} has no plan op", field.0)))?;
        let evaluated = self.evaluate_derived_op(&op, Some(row), event, work);
        let value = match evaluated.and_then(|value| self.materialize_eval(value)) {
            Ok(value) => value,
            Err(error) => {
                self.set_row_currentness(row, field, Currentness::Dirty)?;
                return Err(error);
            }
        };
        self.set_row_field(row, field, value.clone(), work)?;
        self.set_row_currentness(row, field, Currentness::Current)?;
        work.metrics.recomputed_field_count += 1;
        work.recomputed_targets
            .insert(ValueTarget::RowField { row, field });
        Ok(value)
    }

    fn row_identity_has_raw_value(&self, row: RowId, field: FieldId) -> bool {
        let Some(op) = self.metadata.row_computations.get(&field) else {
            return false;
        };
        let identity = matches!(
            &op.kind,
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::RowExpression {
                    expression: PlanRowExpression::Field {
                        input: ValueRef::Field(input),
                    },
                }),
                ..
            } if *input == field
        );
        identity
            && self
                .lists
                .get(&row.list)
                .and_then(|list| list.rows.get(&row))
                .is_some_and(|row| row.fields.contains_key(&field))
    }

    fn evaluate_root_computation(
        &mut self,
        field: FieldId,
        op: &PlanOp,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let value = match &op.kind {
            PlanOpKind::DerivedValue { .. } => {
                let value = self.evaluate_derived_op(op, None, event, work)?;
                self.materialize_eval(value)?
            }
            PlanOpKind::ListProjection { projection } => {
                self.evaluate_projection(field, op.id, projection, event, work)?
            }
            PlanOpKind::ListOperation {
                operation_kind: PlanListOperationKind::Retain,
                retain: Some(retain),
                ..
            } => self.evaluate_list_retain(op, retain, event, work)?,
            PlanOpKind::ListOperation {
                operation_kind: PlanListOperationKind::Count,
                count: Some(count),
                ..
            } => self.evaluate_list_count(op, count, event, work)?,
            _ => {
                return Err(Error::Unsupported {
                    op: op.id,
                    detail: "operation cannot produce a root field".to_owned(),
                });
            }
        };
        Ok(value)
    }

    fn evaluate_derived_op(
        &mut self,
        op: &PlanOp,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let PlanOpKind::DerivedValue {
            derived_kind,
            expression,
            ..
        } = &op.kind
        else {
            return Err(Error::InvalidPlan(format!(
                "op {} is not a derived value",
                op.id.0
            )));
        };
        let Some(expression) = expression else {
            return Err(Error::Unsupported {
                op: op.id,
                detail: format!("{derived_kind:?} derived value has no typed expression"),
            });
        };
        self.eval_derived_expression(
            expression,
            row,
            event,
            op.output.as_ref().and_then(|output| match output {
                ValueRef::Field(field) => Some(*field),
                _ => None,
            }),
            &mut BTreeMap::new(),
            work,
        )
    }
}

impl Session {
    fn row_value(&self, row: RowId, field: FieldId) -> Result<Value, Error> {
        let value = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .and_then(|row| row.fields.get(&field))
            .cloned();
        value.ok_or_else(|| {
            let available = self
                .lists
                .get(&row.list)
                .and_then(|list| list.rows.get(&row))
                .map(|row| row.fields.keys().map(|field| field.0).collect::<Vec<_>>())
                .unwrap_or_default();
            Error::Evaluation(format!(
                "row {}:{}:{} has no field {}; available fields are {:?}",
                row.list.0, row.key, row.generation, field.0, available
            ))
        })
    }

    fn set_row_currentness(
        &mut self,
        row: RowId,
        field: FieldId,
        currentness: Currentness,
    ) -> Result<(), Error> {
        let derived = self
            .lists
            .get_mut(&row.list)
            .and_then(|list| list.rows.get_mut(&row))
            .and_then(|row| row.derived.get_mut(&field))
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "row {}:{}:{} has no derived field {}",
                    row.list.0, row.key, row.generation, field.0
                ))
            })?;
        *derived = currentness;
        Ok(())
    }

    fn set_root_state(&mut self, state: StateId, value: Value, work: &mut Work) -> bool {
        if self.root_states.get(&state) == Some(&value) {
            return false;
        }
        self.root_states.insert(state, value.clone());
        work.dirty_states.insert(state);
        if work.emit {
            work.deltas.push(Delta::SetValue {
                target: ValueTarget::State(state),
                value,
            });
        }
        let dependents = self
            .metadata
            .dependencies
            .root_by_state
            .get(&state)
            .cloned()
            .unwrap_or_default();
        for field in dependents {
            self.mark_root_dirty(field, work);
        }
        let row_dependents = self
            .metadata
            .dependencies
            .row_by_root_state
            .get(&state)
            .cloned()
            .unwrap_or_default();
        for (list, field) in row_dependents {
            for row in self.list_row_ids(list) {
                self.mark_row_dirty(row, field, work);
            }
        }
        true
    }

    fn set_row_field(
        &mut self,
        row: RowId,
        field: FieldId,
        value: Value,
        work: &mut Work,
    ) -> Result<bool, Error> {
        let old = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .and_then(|row| row.fields.get(&field))
            .cloned();
        if old.as_ref() == Some(&value) {
            return Ok(false);
        }
        if let Some(old) = &old {
            self.remove_index_value(row, field, old);
        }
        self.lists
            .get_mut(&row.list)
            .and_then(|list| list.rows.get_mut(&row))
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "cannot set field {} on missing row {}:{}:{}",
                    field.0, row.list.0, row.key, row.generation
                ))
            })?
            .fields
            .insert(field, value.clone());
        self.insert_index_value(row, field, &value);
        work.changed_rows.insert(row);
        if work.emit && !work.suppress_row_deltas.contains(&row) {
            work.deltas.push(Delta::SetValue {
                target: ValueTarget::RowField { row, field },
                value: value.clone(),
            });
        }
        self.invalidate_row_field(row, field, old.as_ref(), Some(&value), work);
        Ok(true)
    }

    fn index_row(&mut self, row: RowId) -> Result<(), Error> {
        let fields = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .map(|row| row.fields.clone())
            .ok_or_else(|| Error::Evaluation("cannot index a missing row".to_owned()))?;
        for (field, value) in fields {
            self.insert_index_value(row, field, &value);
        }
        Ok(())
    }

    fn insert_index_value(&mut self, row: RowId, field: FieldId, value: &Value) {
        let Some(value) = ScalarKey::from_value(value) else {
            return;
        };
        self.indexes
            .entry(IndexKey {
                list: row.list,
                field,
                value,
            })
            .or_default()
            .insert(row);
    }

    fn remove_index_value(&mut self, row: RowId, field: FieldId, value: &Value) {
        let Some(value) = ScalarKey::from_value(value) else {
            return;
        };
        let key = IndexKey {
            list: row.list,
            field,
            value,
        };
        let remove = self.indexes.get_mut(&key).is_some_and(|rows| {
            rows.remove(&row);
            rows.is_empty()
        });
        if remove {
            self.indexes.remove(&key);
        }
    }

    fn lookup_index(
        &mut self,
        list: ListId,
        field: FieldId,
        value: &Value,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<Vec<RowId>, Error> {
        let scalar = ScalarKey::from_value(value).ok_or_else(|| {
            Error::Evaluation(format!(
                "list {} field {} lookup value is not scalar",
                list.0, field.0
            ))
        })?;
        let key = IndexKey {
            list,
            field,
            value: scalar,
        };
        if let Some(consumer) = consumer {
            self.dynamic_dependencies
                .insert(consumer, DynamicDependency::Query(key.clone()));
        }
        work.metrics.index_lookup_count += 1;
        let rows = self
            .indexes
            .get(&key)
            .map(|rows| rows.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        work.metrics.index_candidate_count += rows.len();
        Ok(rows)
    }

    fn list_row_ids(&self, list: ListId) -> Vec<RowId> {
        self.lists
            .get(&list)
            .map(|list| list.order.clone())
            .unwrap_or_default()
    }

    fn row_exists(&self, row: RowId) -> bool {
        self.lists
            .get(&row.list)
            .is_some_and(|list| list.rows.contains_key(&row))
    }

    fn mark_root_dirty(&mut self, field: FieldId, work: &mut Work) {
        if self
            .root_fields
            .get(&field)
            .is_some_and(|cell| cell.currentness == Currentness::Evaluating)
        {
            return;
        }
        let became_dirty = self.root_fields.get_mut(&field).map(|cell| {
            let became_dirty = cell.currentness == Currentness::Current;
            if became_dirty {
                cell.currentness = Currentness::Dirty;
            }
            became_dirty
        });
        let consumer = Consumer::Root(field);
        let first_in_turn = work.dirty_consumers.insert(consumer);
        if became_dirty.is_none() || (!became_dirty.unwrap_or_default() && !first_in_turn) {
            return;
        }
        self.dynamic_dependencies.clear(consumer);
        self.invalidate_root_field(field, work);
    }

    fn invalidate_root_field(&mut self, field: FieldId, work: &mut Work) {
        let root_dependents = self
            .metadata
            .dependencies
            .root_by_field
            .get(&field)
            .cloned()
            .unwrap_or_default();
        for dependent in root_dependents {
            if dependent != field {
                self.mark_root_dirty(dependent, work);
            }
        }
        let row_dependents = self
            .metadata
            .dependencies
            .row_by_root_field
            .get(&field)
            .cloned()
            .unwrap_or_default();
        for (list, dependent) in row_dependents {
            for row in self.list_row_ids(list) {
                self.mark_row_dirty(row, dependent, work);
            }
        }
    }

    fn mark_row_dirty(&mut self, row: RowId, field: FieldId, work: &mut Work) {
        if self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .and_then(|row| row.derived.get(&field))
            .is_some_and(|currentness| *currentness == Currentness::Evaluating)
        {
            return;
        }
        let became_dirty = self
            .lists
            .get_mut(&row.list)
            .and_then(|list| list.rows.get_mut(&row))
            .and_then(|row| row.derived.get_mut(&field))
            .map(|currentness| {
                let became_dirty = *currentness == Currentness::Current;
                if became_dirty {
                    *currentness = Currentness::Dirty;
                }
                became_dirty
            });
        let consumer = Consumer::Row(row, field);
        let first_in_turn = work.dirty_consumers.insert(consumer);
        if became_dirty.is_none() || (!became_dirty.unwrap_or_default() && !first_in_turn) {
            return;
        }
        let dynamic_dependents = self
            .dynamic_dependencies
            .by_row_field
            .get(&(row, field))
            .cloned()
            .unwrap_or_default();
        self.dynamic_dependencies.clear(consumer);
        let dependents = self
            .metadata
            .dependencies
            .row_by_field
            .get(&(row.list, field))
            .cloned()
            .unwrap_or_default();
        for dependent in dependents {
            if dependent != field {
                self.mark_row_dirty(row, dependent, work);
            }
        }
        for dependent in dynamic_dependents {
            if dependent != consumer {
                self.mark_consumer_dirty(dependent, work);
            }
        }
    }

    fn mark_consumer_dirty(&mut self, consumer: Consumer, work: &mut Work) {
        work.metrics.dependency_fanout_count += 1;
        match consumer {
            Consumer::Root(field) => self.mark_root_dirty(field, work),
            Consumer::Row(row, field) => self.mark_row_dirty(row, field, work),
        }
    }

    fn invalidate_row_field(
        &mut self,
        row: RowId,
        field: FieldId,
        old: Option<&Value>,
        new: Option<&Value>,
        work: &mut Work,
    ) {
        let mut consumers = self
            .dynamic_dependencies
            .by_row_field
            .get(&(row, field))
            .cloned()
            .unwrap_or_default();
        for value in [old, new].into_iter().flatten() {
            if let Some(value) = ScalarKey::from_value(value) {
                let key = IndexKey {
                    list: row.list,
                    field,
                    value,
                };
                if let Some(query_consumers) = self.dynamic_dependencies.by_query.get(&key) {
                    consumers.extend(query_consumers);
                }
            }
        }
        let static_dependents = self
            .metadata
            .dependencies
            .row_by_field
            .get(&(row.list, field))
            .cloned()
            .unwrap_or_default();
        for dependent in static_dependents {
            consumers.insert(Consumer::Row(row, dependent));
        }
        for consumer in consumers {
            self.mark_consumer_dirty(consumer, work);
        }
    }

    fn invalidate_list_structure(&mut self, list: ListId, work: &mut Work) {
        let mut consumers = self
            .dynamic_dependencies
            .by_list
            .get(&list)
            .cloned()
            .unwrap_or_default();
        for (key, query_consumers) in &self.dynamic_dependencies.by_query {
            if key.list == list {
                consumers.extend(query_consumers);
            }
        }
        for field in self
            .metadata
            .dependencies
            .root_by_list
            .get(&list)
            .cloned()
            .unwrap_or_default()
        {
            consumers.insert(Consumer::Root(field));
        }
        for (owner, field) in self
            .metadata
            .dependencies
            .row_by_list
            .get(&list)
            .cloned()
            .unwrap_or_default()
        {
            for row in self.list_row_ids(owner) {
                consumers.insert(Consumer::Row(row, field));
            }
        }
        for consumer in consumers {
            self.mark_consumer_dirty(consumer, work);
        }
    }

    fn register_row_dependency(
        &mut self,
        consumer: Option<Consumer>,
        row: RowId,
        field: FieldId,
    ) -> bool {
        if let Some(consumer) = consumer {
            let dependency = Consumer::Row(row, field);
            let creates_cycle =
                dependency == consumer || self.dynamic_dependency_reaches(dependency, consumer);
            self.dynamic_dependencies
                .insert(consumer, DynamicDependency::RowField(row, field));
            creates_cycle
        } else {
            false
        }
    }

    fn dynamic_dependency_reaches(&self, start: Consumer, target: Consumer) -> bool {
        let mut pending = vec![start];
        let mut visited = BTreeSet::new();
        while let Some(consumer) = pending.pop() {
            if consumer == target {
                return true;
            }
            if !visited.insert(consumer) {
                continue;
            }
            pending.extend(
                self.dynamic_dependencies
                    .by_consumer
                    .get(&consumer)
                    .into_iter()
                    .flatten()
                    .filter_map(|dependency| match dependency {
                        DynamicDependency::RowField(row, field) => {
                            Some(Consumer::Row(*row, *field))
                        }
                        DynamicDependency::Query(_) | DynamicDependency::List(_) => None,
                    }),
            );
        }
        false
    }

    fn register_list_dependency(&mut self, consumer: Option<Consumer>, list: ListId) {
        if let Some(consumer) = consumer {
            self.dynamic_dependencies
                .insert(consumer, DynamicDependency::List(list));
        }
    }

    fn materialize_eval(&mut self, value: EvalValue) -> Result<Value, Error> {
        match value {
            EvalValue::Value(value) => Ok(value),
            EvalValue::Row(row) => Ok(row_identity_value(row)),
            EvalValue::List(values) => values
                .into_iter()
                .map(|value| self.materialize_eval(value))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List),
            EvalValue::Record(values) => values
                .into_iter()
                .map(|(name, value)| Ok((name, self.materialize_eval(value)?)))
                .collect::<Result<BTreeMap<_, _>, Error>>()
                .map(Value::Record),
            EvalValue::MappedRow { id, fields } => fields
                .into_iter()
                .map(|(name, value)| Ok((name, self.materialize_eval(value)?)))
                .collect::<Result<BTreeMap<_, _>, Error>>()
                .map(|fields| Value::MappedRow { id, fields }),
        }
    }
}

impl Session {
    #[allow(clippy::too_many_arguments)]
    fn eval_derived_expression(
        &mut self,
        expression: &PlanDerivedExpression,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        bindings: &mut BTreeMap<String, EvalValue>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        work.consume(1)?;
        let consumer = output.map(|field| match row {
            Some(row) => Consumer::Row(row, field),
            None => Consumer::Root(field),
        });
        match expression {
            PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                source_id,
                key_field,
                required_key,
                state,
                skip_empty,
            } => {
                let Some(event) = event else {
                    return Ok(EvalValue::Value(Value::Null));
                };
                if event.source != *source_id {
                    return Ok(EvalValue::Value(Value::Null));
                }
                let key = source_payload_value(&event.payload, key_field)
                    .map(|value| value_to_text(&value))
                    .transpose()?
                    .unwrap_or_default();
                if key != *required_key {
                    return Ok(EvalValue::Value(Value::Null));
                }
                let value = self.eval_value_ref(state, row, Some(event), output, consumer, work)?;
                let text = eval_to_text(&value)?.trim().to_owned();
                if *skip_empty && text.is_empty() {
                    Ok(EvalValue::Value(Value::Null))
                } else {
                    Ok(EvalValue::Value(Value::Text(text)))
                }
            }
            PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
                if let Some(event) = event
                    && let Some(arm) = arms.iter().find(|arm| arm.source_id == event.source)
                {
                    return self.eval_row_expression(
                        &arm.value,
                        row.or(event.target),
                        Some(event),
                        output,
                        consumer,
                        bindings,
                        work,
                    );
                }
                self.eval_row_expression(default, row, event, output, consumer, bindings, work)
            }
            PlanDerivedExpression::BoolNot { input } => {
                let value = self.eval_value_ref(input, row, event, output, consumer, work)?;
                Ok(EvalValue::Value(Value::Bool(!eval_to_bool(&value)?)))
            }
            PlanDerivedExpression::NumberCompareConst { left, op, right } => {
                let left = self.eval_value_ref(left, row, event, output, consumer, work)?;
                let left = eval_to_number(&left)?;
                Ok(EvalValue::Value(Value::Bool(compare_numbers(
                    left, op, *right,
                )?)))
            }
            PlanDerivedExpression::ValueCompare { left, op, right } => {
                let left = self.eval_value_ref(left, row, event, output, consumer, work)?;
                let right = self.eval_value_ref(right, row, event, output, consumer, work)?;
                let EvalValue::Value(left) = left else {
                    return Err(Error::Evaluation(
                        "left comparison operand is not a scalar value".to_owned(),
                    ));
                };
                let EvalValue::Value(right) = right else {
                    return Err(Error::Evaluation(
                        "right comparison operand is not a scalar value".to_owned(),
                    ));
                };
                Ok(EvalValue::Value(Value::Bool(compare_update_values(
                    &left, op, &right,
                )?)))
            }
            PlanDerivedExpression::BoolAnd { left, right } => {
                let left =
                    self.eval_derived_expression(left, row, event, output, bindings, work)?;
                if !eval_to_bool(&left)? {
                    return Ok(EvalValue::Value(Value::Bool(false)));
                }
                let right =
                    self.eval_derived_expression(right, row, event, output, bindings, work)?;
                Ok(EvalValue::Value(Value::Bool(eval_to_bool(&right)?)))
            }
            PlanDerivedExpression::BoolNotExpression { input } => {
                let value =
                    self.eval_derived_expression(input, row, event, output, bindings, work)?;
                Ok(EvalValue::Value(Value::Bool(!eval_to_bool(&value)?)))
            }
            PlanDerivedExpression::RowExpression { expression } => self.eval_row_expression(
                expression,
                expression_row(row),
                event,
                output,
                consumer,
                bindings,
                work,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_value_ref(
        &mut self,
        value_ref: &ValueRef,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        work.consume(1)?;
        match value_ref {
            ValueRef::Source(source) => Ok(EvalValue::Value(Value::Bool(
                event.is_some_and(|event| event.source == *source),
            ))),
            ValueRef::SourcePayload { source_id, field } => {
                let Some(event) = event.filter(|event| event.source == *source_id) else {
                    return Ok(EvalValue::Value(Value::Null));
                };
                Ok(EvalValue::Value(
                    source_payload_value(&event.payload, field).unwrap_or(Value::Null),
                ))
            }
            ValueRef::Constant(constant) => self
                .metadata
                .constants
                .get(constant)
                .cloned()
                .map(EvalValue::Value)
                .ok_or_else(|| Error::InvalidPlan(format!("missing constant {}", constant.0))),
            ValueRef::List(list) => {
                self.register_list_dependency(consumer, *list);
                let rows = self.list_row_ids(*list);
                work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
                Ok(EvalValue::List(
                    rows.into_iter().map(EvalValue::Row).collect(),
                ))
            }
            ValueRef::State(state) => {
                if let Some(owner) = self.metadata.indexed_state_owner.get(state).copied() {
                    let row = row.ok_or_else(|| {
                        Error::Evaluation(format!(
                            "indexed state {} requires a row context",
                            state.0
                        ))
                    })?;
                    if row.list != owner {
                        return Err(Error::Evaluation(format!(
                            "indexed state {} belongs to list {}, not {}",
                            state.0, owner.0, row.list.0
                        )));
                    }
                    let field = *self
                        .metadata
                        .indexed_state_field
                        .get(state)
                        .ok_or_else(|| {
                            Error::InvalidPlan(format!("indexed state {} has no field", state.0))
                        })?;
                    self.register_row_dependency(consumer, row, field);
                    return self.row_value(row, field).map(EvalValue::Value);
                }
                self.root_states
                    .get(state)
                    .cloned()
                    .map(EvalValue::Value)
                    .ok_or_else(|| {
                        Error::Evaluation(format!("root state {} has no value", state.0))
                    })
            }
            ValueRef::Field(field) => {
                if let Some(owner) = self.metadata.row_field_owner.get(field).copied() {
                    let row = row.ok_or_else(|| {
                        Error::Evaluation(format!("row field {} requires a row context", field.0))
                    })?;
                    if row.list != owner {
                        return Err(Error::Evaluation(format!(
                            "field {} belongs to list {}, not {}",
                            field.0, owner.0, row.list.0
                        )));
                    }
                    if output == Some(*field)
                        && self
                            .metadata
                            .row_computations
                            .get(field)
                            .is_some_and(|op| source_event_transform_op(op))
                    {
                        return self.row_value(row, *field).map(EvalValue::Value);
                    }
                    self.register_row_dependency(consumer, row, *field);
                    let has_value = self
                        .lists
                        .get(&row.list)
                        .and_then(|list| list.rows.get(&row))
                        .is_some_and(|row| row.fields.contains_key(field));
                    if !has_value && !self.metadata.row_computations.contains_key(field) {
                        return Ok(EvalValue::Value(Value::Null));
                    }
                    if output == Some(*field) && self.row_identity_has_raw_value(row, *field) {
                        return self.row_value(row, *field).map(EvalValue::Value);
                    }
                    return self
                        .ensure_row_field(row, *field, event, work)
                        .map(EvalValue::Value);
                }
                if output == Some(*field)
                    && self
                        .metadata
                        .root_computations
                        .get(field)
                        .is_some_and(|op| source_event_transform_op(op))
                    && let Some(value) = self
                        .root_fields
                        .get(field)
                        .and_then(|cell| cell.value.clone())
                {
                    return Ok(EvalValue::Value(value));
                }
                self.ensure_root_field(*field, event, work)
                    .map(EvalValue::Value)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_row_expression(
        &mut self,
        expression: &PlanRowExpression,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut BTreeMap<String, EvalValue>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        work.consume(1)?;
        match expression {
            PlanRowExpression::Field { input } => {
                self.eval_value_ref(input, row, event, output, consumer, work)
            }
            PlanRowExpression::Constant { constant_id } => self
                .metadata
                .constants
                .get(constant_id)
                .cloned()
                .map(EvalValue::Value)
                .ok_or_else(|| Error::InvalidPlan(format!("missing constant {}", constant_id.0))),
            PlanRowExpression::TextTrim { input } => {
                let value =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Text(
                    eval_to_text(&value)?.trim().to_owned(),
                )))
            }
            PlanRowExpression::TextIsEmpty { input } => {
                let value =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Bool(
                    eval_to_text(&value)?.is_empty(),
                )))
            }
            PlanRowExpression::TextStartsWith { input, prefix } => {
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let prefix =
                    self.eval_row_expression(prefix, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Bool(
                    eval_to_text(&input)?.starts_with(&eval_to_text(&prefix)?),
                )))
            }
            PlanRowExpression::TextLength { input } => {
                let value =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Number(
                    eval_to_text(&value)?.chars().count() as i64,
                )))
            }
            PlanRowExpression::TextToNumber { input } => {
                let value =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let text = eval_to_text(&value)?;
                Ok(EvalValue::Value(match text.trim().parse::<i64>() {
                    Ok(value) => Value::Number(value),
                    Err(_) => Value::Text("NaN".to_owned()),
                }))
            }
            PlanRowExpression::TextSubstring {
                input,
                start,
                length,
            } => {
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let start =
                    self.eval_row_expression(start, row, event, output, consumer, bindings, work)?;
                let length =
                    self.eval_row_expression(length, row, event, output, consumer, bindings, work)?;
                let start = nonnegative_usize(eval_to_number(&start)?, "text substring start")?;
                let length = nonnegative_usize(eval_to_number(&length)?, "text substring length")?;
                let text = eval_to_text(&input)?;
                let value = text.chars().skip(start).take(length).collect::<String>();
                Ok(EvalValue::Value(Value::Text(value)))
            }
            PlanRowExpression::TextToBytes { input, encoding } => {
                validate_encoding(
                    self.eval_optional_text(
                        encoding.as_deref(),
                        row,
                        event,
                        output,
                        consumer,
                        bindings,
                        work,
                    )?
                    .as_deref(),
                )?;
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Bytes(
                    eval_to_text(&input)?.into_bytes(),
                )))
            }
            PlanRowExpression::BytesToText { input, encoding } => {
                validate_encoding(
                    self.eval_optional_text(
                        encoding.as_deref(),
                        row,
                        event,
                        output,
                        consumer,
                        bindings,
                        work,
                    )?
                    .as_deref(),
                )?;
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let bytes = eval_to_bytes(&input)?;
                let text = String::from_utf8(bytes)
                    .map_err(|error| Error::Evaluation(format!("invalid UTF-8: {error}")))?;
                Ok(EvalValue::Value(Value::Text(text)))
            }
            PlanRowExpression::BytesToHex { input } => {
                let bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Text(encode_hex(&bytes))))
            }
            PlanRowExpression::BytesToBase64 { input } => {
                let bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Text(encode_base64(&bytes))))
            }
            PlanRowExpression::BytesFromHex { input } => {
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Bytes(decode_hex(&eval_to_text(
                    &input,
                )?)?)))
            }
            PlanRowExpression::BytesFromBase64 { input } => {
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Bytes(decode_base64(
                    &eval_to_text(&input)?,
                )?)))
            }
            PlanRowExpression::BytesIsEmpty { input } => {
                let bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Bool(bytes.is_empty())))
            }
            PlanRowExpression::BytesLength { input } => {
                let bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Number(bytes.len() as i64)))
            }
            PlanRowExpression::BytesGet { input, index } => {
                let bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                let index = self
                    .eval_expression_number(index, row, event, output, consumer, bindings, work)?;
                let index = nonnegative_usize(index, "byte index")?;
                let value = bytes.get(index).copied().ok_or_else(|| {
                    Error::Evaluation(format!("byte index {index} is out of range"))
                })?;
                Ok(EvalValue::Value(Value::Number(i64::from(value))))
            }
            PlanRowExpression::BytesSlice {
                input,
                offset,
                byte_count,
            } => {
                let bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                let offset = nonnegative_usize(
                    self.eval_expression_number(
                        offset, row, event, output, consumer, bindings, work,
                    )?,
                    "byte offset",
                )?;
                let count = nonnegative_usize(
                    self.eval_expression_number(
                        byte_count, row, event, output, consumer, bindings, work,
                    )?,
                    "byte count",
                )?;
                Ok(EvalValue::Value(Value::Bytes(checked_slice(
                    &bytes, offset, count,
                )?)))
            }
            PlanRowExpression::BytesTake { input, byte_count } => {
                let bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                let count = nonnegative_usize(
                    self.eval_expression_number(
                        byte_count, row, event, output, consumer, bindings, work,
                    )?,
                    "byte count",
                )?;
                Ok(EvalValue::Value(Value::Bytes(
                    bytes.into_iter().take(count).collect(),
                )))
            }
            PlanRowExpression::BytesDrop { input, byte_count } => {
                let bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                let count = nonnegative_usize(
                    self.eval_expression_number(
                        byte_count, row, event, output, consumer, bindings, work,
                    )?,
                    "byte count",
                )?;
                Ok(EvalValue::Value(Value::Bytes(
                    bytes.into_iter().skip(count).collect(),
                )))
            }
            PlanRowExpression::BytesZeros { byte_count } => {
                let count = nonnegative_usize(
                    self.eval_expression_number(
                        byte_count, row, event, output, consumer, bindings, work,
                    )?,
                    "byte count",
                )?;
                Ok(EvalValue::Value(Value::Bytes(vec![0; count])))
            }
            PlanRowExpression::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            }
            | PlanRowExpression::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                let bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                let offset = nonnegative_usize(
                    self.eval_expression_number(
                        offset, row, event, output, consumer, bindings, work,
                    )?,
                    "byte offset",
                )?;
                let count = nonnegative_usize(
                    self.eval_expression_number(
                        byte_count, row, event, output, consumer, bindings, work,
                    )?,
                    "byte count",
                )?;
                let endian =
                    self.eval_row_expression(endian, row, event, output, consumer, bindings, work)?;
                let signed = matches!(expression, PlanRowExpression::BytesReadSigned { .. });
                Ok(EvalValue::Value(Value::Number(read_integer(
                    &bytes,
                    offset,
                    count,
                    &eval_to_text(&endian)?,
                    signed,
                )?)))
            }
            PlanRowExpression::BytesSet {
                input,
                index,
                value,
            } => {
                let mut bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                let index = nonnegative_usize(
                    self.eval_expression_number(
                        index, row, event, output, consumer, bindings, work,
                    )?,
                    "byte index",
                )?;
                let value = self
                    .eval_expression_number(value, row, event, output, consumer, bindings, work)?;
                let value = u8::try_from(value).map_err(|_| {
                    Error::Evaluation(format!("byte value {value} is outside 0..=255"))
                })?;
                let target = bytes.get_mut(index).ok_or_else(|| {
                    Error::Evaluation(format!("byte index {index} is out of range"))
                })?;
                *target = value;
                Ok(EvalValue::Value(Value::Bytes(bytes)))
            }
            PlanRowExpression::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            }
            | PlanRowExpression::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                let mut bytes = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                let offset = nonnegative_usize(
                    self.eval_expression_number(
                        offset, row, event, output, consumer, bindings, work,
                    )?,
                    "byte offset",
                )?;
                let count = nonnegative_usize(
                    self.eval_expression_number(
                        byte_count, row, event, output, consumer, bindings, work,
                    )?,
                    "byte count",
                )?;
                let endian =
                    self.eval_row_expression(endian, row, event, output, consumer, bindings, work)?;
                let value = self
                    .eval_expression_number(value, row, event, output, consumer, bindings, work)?;
                write_integer(&mut bytes, offset, count, &eval_to_text(&endian)?, value)?;
                Ok(EvalValue::Value(Value::Bytes(bytes)))
            }
            PlanRowExpression::BytesFind { input, needle } => {
                let input = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                let needle = self
                    .eval_expression_bytes(needle, row, event, output, consumer, bindings, work)?;
                let value = find_bytes(&input, &needle)
                    .map(|index| Value::Number(index as i64))
                    .unwrap_or_else(|| Value::Text("NaN".to_owned()));
                Ok(EvalValue::Value(value))
            }
            PlanRowExpression::BytesStartsWith { input, prefix } => {
                let input = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                let prefix = self
                    .eval_expression_bytes(prefix, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Bool(input.starts_with(&prefix))))
            }
            PlanRowExpression::BytesEndsWith { input, suffix } => {
                let input = self
                    .eval_expression_bytes(input, row, event, output, consumer, bindings, work)?;
                let suffix = self
                    .eval_expression_bytes(suffix, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Bool(input.ends_with(&suffix))))
            }
            PlanRowExpression::BytesConcat { left, right } => {
                let mut left =
                    self.eval_expression_bytes(left, row, event, output, consumer, bindings, work)?;
                left.extend(
                    self.eval_expression_bytes(
                        right, row, event, output, consumer, bindings, work,
                    )?,
                );
                Ok(EvalValue::Value(Value::Bytes(left)))
            }
            PlanRowExpression::BytesEqual { left, right } => {
                let left =
                    self.eval_expression_bytes(left, row, event, output, consumer, bindings, work)?;
                let right = self
                    .eval_expression_bytes(right, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Bool(left == right)))
            }
            PlanRowExpression::NumberInfix { op, left, right } => {
                let left =
                    self.eval_row_expression(left, row, event, output, consumer, bindings, work)?;
                let right =
                    self.eval_row_expression(right, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(eval_number_infix(op, &left, &right)?))
            }
            PlanRowExpression::TextConcat { parts } => {
                let mut text = String::new();
                for part in parts {
                    let value = self
                        .eval_row_expression(part, row, event, output, consumer, bindings, work)?;
                    text.push_str(&eval_to_text(&value)?);
                }
                Ok(EvalValue::Value(Value::Text(text)))
            }
            PlanRowExpression::ListGetField {
                list_id,
                index,
                field,
            } => {
                let index = self
                    .eval_expression_number(index, row, event, output, consumer, bindings, work)?;
                let index = nonnegative_usize(index, "list index")?;
                let target = self
                    .lists
                    .get(list_id)
                    .and_then(|list| list.order.get(index))
                    .copied()
                    .ok_or_else(|| {
                        Error::Evaluation(format!(
                            "list {} index {index} is out of range",
                            list_id.0
                        ))
                    })?;
                self.register_row_dependency(consumer, target, *field);
                self.ensure_row_field(target, *field, event, work)
                    .map(EvalValue::Value)
            }
            PlanRowExpression::ListRef { list_id } => {
                self.register_list_dependency(consumer, *list_id);
                let rows = self.list_row_ids(*list_id);
                work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
                Ok(EvalValue::List(
                    rows.into_iter().map(EvalValue::Row).collect(),
                ))
            }
            PlanRowExpression::ListFindValue {
                list_id,
                field,
                value,
                target,
                fallback,
            } => {
                let key =
                    self.eval_row_expression(value, row, event, output, consumer, bindings, work)?;
                let key = self.materialize_eval(key)?;
                let candidates = self.lookup_index(*list_id, *field, &key, consumer, work)?;
                if let Some(found) = candidates.first().copied() {
                    if self.register_row_dependency(consumer, found, *target) {
                        return Ok(EvalValue::Value(Value::Error {
                            code: "cycle_error".to_owned(),
                        }));
                    }
                    return self
                        .ensure_row_field(found, *target, event, work)
                        .map(EvalValue::Value);
                }
                if let Some(fallback) = fallback {
                    self.eval_row_expression(fallback, row, event, output, consumer, bindings, work)
                } else {
                    Ok(EvalValue::Value(Value::Text("NaN".to_owned())))
                }
            }
            PlanRowExpression::ListRange { from, to } => {
                let from = self
                    .eval_expression_number(from, row, event, output, consumer, bindings, work)?;
                let to =
                    self.eval_expression_number(to, row, event, output, consumer, bindings, work)?;
                let values = if from <= to {
                    let length = to
                        .checked_sub(from)
                        .and_then(|span| span.checked_add(1))
                        .and_then(|length| u64::try_from(length).ok())
                        .unwrap_or(u64::MAX);
                    work.consume(length)?;
                    (from..=to)
                        .map(|value| EvalValue::Value(Value::Number(value)))
                        .collect()
                } else {
                    Vec::new()
                };
                Ok(EvalValue::List(values))
            }
            PlanRowExpression::ListLiteral { items } => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(
                        self.eval_row_expression(
                            item, row, event, output, consumer, bindings, work,
                        )?,
                    );
                }
                Ok(EvalValue::List(values))
            }
            PlanRowExpression::ListMap {
                input,
                binding,
                value,
            } => {
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let items = eval_to_list(input)?;
                work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                let previous = bindings.get(binding).cloned();
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    let origin = eval_row_id(&item);
                    bindings.insert(binding.clone(), item);
                    let value = self
                        .eval_row_expression(value, row, event, output, consumer, bindings, work)?;
                    values.push(match (origin, value) {
                        (Some(id), EvalValue::Record(fields)) => {
                            EvalValue::MappedRow { id, fields }
                        }
                        (_, value) => value,
                    });
                }
                match previous {
                    Some(previous) => {
                        bindings.insert(binding.clone(), previous);
                    }
                    None => {
                        bindings.remove(binding);
                    }
                }
                Ok(EvalValue::List(values))
            }
            PlanRowExpression::ListMapItem { binding } => {
                bindings.get(binding).cloned().ok_or_else(|| {
                    Error::Evaluation(format!("List/map binding `{binding}` is missing"))
                })
            }
            PlanRowExpression::ListSum { input } => {
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let items = eval_to_list(input)?;
                work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                let mut total = 0i64;
                for item in items {
                    if let Ok(value) = eval_to_number(&item) {
                        total = total
                            .checked_add(value)
                            .ok_or_else(|| Error::Evaluation("List/sum overflow".to_owned()))?;
                    }
                }
                Ok(EvalValue::Value(Value::Number(total)))
            }
            PlanRowExpression::Object { fields } => {
                let mut record = BTreeMap::new();
                for field in fields {
                    record.insert(
                        field.name.clone(),
                        self.eval_row_expression(
                            &field.value,
                            row,
                            event,
                            output,
                            consumer,
                            bindings,
                            work,
                        )?,
                    );
                }
                Ok(EvalValue::Record(record))
            }
            PlanRowExpression::ObjectField { object, field } => {
                let object =
                    self.eval_row_expression(object, row, event, output, consumer, bindings, work)?;
                self.eval_object_field(object, field, event, consumer, work)
            }
            PlanRowExpression::ListRowField {
                row: row_expression,
                list_id,
                field,
            } => {
                let value = self.eval_row_expression(
                    row_expression,
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                self.eval_list_row_field(value, *list_id, *field, event, consumer, work)
            }
            PlanRowExpression::BuiltinCall {
                function,
                input,
                args,
            } => self.eval_builtin(
                function,
                input.as_deref(),
                args,
                row,
                event,
                output,
                consumer,
                bindings,
                work,
            ),
            PlanRowExpression::Select { input, arms } => {
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let input_value = self.materialize_eval(input)?;
                for arm in arms {
                    if select_pattern_matches(&arm.pattern, &input_value) {
                        return self.eval_row_expression(
                            &arm.value, row, event, output, consumer, bindings, work,
                        );
                    }
                }
                Err(Error::Evaluation(format!(
                    "select has no matching arm for {input_value:?}"
                )))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_expression_number(
        &mut self,
        expression: &PlanRowExpression,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut BTreeMap<String, EvalValue>,
        work: &mut Work,
    ) -> Result<i64, Error> {
        let value =
            self.eval_row_expression(expression, row, event, output, consumer, bindings, work)?;
        eval_to_number(&value)
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_expression_bytes(
        &mut self,
        expression: &PlanRowExpression,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut BTreeMap<String, EvalValue>,
        work: &mut Work,
    ) -> Result<Vec<u8>, Error> {
        let value =
            self.eval_row_expression(expression, row, event, output, consumer, bindings, work)?;
        eval_to_bytes(&value)
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_optional_text(
        &mut self,
        expression: Option<&PlanRowExpression>,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut BTreeMap<String, EvalValue>,
        work: &mut Work,
    ) -> Result<Option<String>, Error> {
        expression
            .map(|expression| {
                let value = self.eval_row_expression(
                    expression, row, event, output, consumer, bindings, work,
                )?;
                eval_to_text(&value)
            })
            .transpose()
    }

    fn eval_object_field(
        &mut self,
        object: EvalValue,
        field: &str,
        event: Option<&SourceEvent>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        match object {
            EvalValue::Record(mut record) => record
                .remove(field)
                .ok_or_else(|| Error::Evaluation(format!("record has no field `{field}`"))),
            EvalValue::MappedRow { mut fields, .. } => fields
                .remove(field)
                .ok_or_else(|| Error::Evaluation(format!("record has no field `{field}`"))),
            EvalValue::Value(Value::Record(mut record)) => record
                .remove(field)
                .map(EvalValue::Value)
                .ok_or_else(|| Error::Evaluation(format!("record has no field `{field}`"))),
            EvalValue::Value(Value::MappedRow { mut fields, .. }) => fields
                .remove(field)
                .map(EvalValue::Value)
                .ok_or_else(|| Error::Evaluation(format!("record has no field `{field}`"))),
            EvalValue::Row(row) | EvalValue::Value(Value::Row { id: row, .. }) => {
                let field = self.metadata.list_field(row.list, field)?;
                self.register_row_dependency(consumer, row, field);
                self.ensure_row_field(row, field, event, work)
                    .map(EvalValue::Value)
            }
            other => Err(Error::Evaluation(format!(
                "value {other:?} is not an object"
            ))),
        }
    }

    fn eval_list_row_field(
        &mut self,
        value: EvalValue,
        list_id: ListId,
        field: FieldId,
        event: Option<&SourceEvent>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let row = match value {
            EvalValue::Row(row) | EvalValue::Value(Value::Row { id: row, .. }) => row,
            other => {
                return Err(Error::Evaluation(format!(
                    "value {other:?} is not a typed list row"
                )));
            }
        };
        if row.list != list_id {
            return Err(Error::InvalidPlan(format!(
                "typed row field {} belongs to list {}, but expression produced list {}",
                field.0, list_id.0, row.list.0
            )));
        }
        if self.metadata.row_field_owner.get(&field) != Some(&list_id) {
            return Err(Error::InvalidPlan(format!(
                "typed row field {} does not belong to list {}",
                field.0, list_id.0
            )));
        }
        self.register_row_dependency(consumer, row, field);
        self.ensure_row_field(row, field, event, work)
            .map(EvalValue::Value)
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_builtin(
        &mut self,
        function: &str,
        input: Option<&PlanRowExpression>,
        args: &[PlanRowCallArg],
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut BTreeMap<String, EvalValue>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        match function {
            "Text/empty" => Ok(EvalValue::Value(Value::Text(String::new()))),
            "Error/new" => {
                let code = self
                    .eval_named_arg(args, "code", row, event, output, consumer, bindings, work)?
                    .map(|value| eval_to_text(&value))
                    .transpose()?
                    .unwrap_or_else(|| "error".to_owned());
                Ok(EvalValue::Value(Value::Error { code }))
            }
            "Error/text" => {
                let value = if let Some(input) = input {
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?
                } else if let Some(value) = self
                    .eval_named_arg(args, "value", row, event, output, consumer, bindings, work)?
                {
                    value
                } else if let Some(arg) = args.first() {
                    self.eval_row_expression(
                        &arg.value, row, event, output, consumer, bindings, work,
                    )?
                } else {
                    return Err(Error::Evaluation("Error/text requires a value".to_owned()));
                };
                let code = match value {
                    EvalValue::Value(Value::Error { code }) => code,
                    _ => String::new(),
                };
                Ok(EvalValue::Value(Value::Text(code)))
            }
            "Number/min" | "Number/max" => {
                let left = if let Some(input) = input {
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?
                } else {
                    self.eval_named_arg(args, "left", row, event, output, consumer, bindings, work)?
                        .ok_or_else(|| Error::Evaluation(format!("{function} requires `left`")))?
                };
                let right = self
                    .eval_named_arg(args, "right", row, event, output, consumer, bindings, work)?
                    .ok_or_else(|| Error::Evaluation(format!("{function} requires `right`")))?;
                let left = eval_to_number(&left)?;
                let right = eval_to_number(&right)?;
                Ok(EvalValue::Value(Value::Number(
                    if function == "Number/min" {
                        left.min(right)
                    } else {
                        left.max(right)
                    },
                )))
            }
            "Number/interpolate" => {
                let start = self.required_number_arg(
                    args, "start", row, event, output, consumer, bindings, work,
                )?;
                let end = self.required_number_arg(
                    args, "end", row, event, output, consumer, bindings, work,
                )?;
                let numerator = self.required_number_arg(
                    args,
                    "numerator",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let denominator = self.required_number_arg(
                    args,
                    "denominator",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let fallback = self.required_number_arg(
                    args, "fallback", row, event, output, consumer, bindings, work,
                )?;
                let value = if denominator == 0 {
                    fallback
                } else {
                    end.checked_sub(start)
                        .and_then(|span| span.checked_mul(numerator))
                        .and_then(|offset| offset.checked_div(denominator))
                        .and_then(|offset| start.checked_add(offset))
                        .ok_or_else(|| {
                            Error::Evaluation("Number/interpolate arithmetic overflow".to_owned())
                        })?
                };
                Ok(EvalValue::Value(Value::Number(value)))
            }
            "Number/project_offset" => {
                let time = self.required_number_arg(
                    args, "time", row, event, output, consumer, bindings, work,
                )?;
                let start = self.required_number_arg(
                    args,
                    "viewport_start",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let end = self.required_number_arg(
                    args,
                    "viewport_end",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let width = self.required_number_arg(
                    args,
                    "canvas_width",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let fallback = self.required_number_arg(
                    args, "fallback", row, event, output, consumer, bindings, work,
                )?;
                let _zoom = self
                    .eval_named_arg(args, "zoom", row, event, output, consumer, bindings, work)?;
                let span = end.checked_sub(start).ok_or_else(|| {
                    Error::Evaluation("Number/project_offset span overflow".to_owned())
                })?;
                let value = if span <= 0 || width <= 0 {
                    fallback
                } else {
                    time.checked_sub(start)
                        .and_then(|offset| offset.checked_mul(width))
                        .and_then(|offset| offset.checked_div(span))
                        .ok_or_else(|| {
                            Error::Evaluation(
                                "Number/project_offset arithmetic overflow".to_owned(),
                            )
                        })?
                        .clamp(0, width)
                };
                Ok(EvalValue::Value(Value::Number(value)))
            }
            "Number/project_time" => {
                let x = self.required_number_arg(
                    args,
                    "pointer_x",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let width = self.required_number_arg(
                    args,
                    "pointer_width",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let start = self.required_number_arg(
                    args,
                    "viewport_start",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let end = self.required_number_arg(
                    args,
                    "viewport_end",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let fallback = self.required_number_arg(
                    args, "fallback", row, event, output, consumer, bindings, work,
                )?;
                let value = if width <= 0 {
                    fallback
                } else {
                    end.checked_sub(start)
                        .and_then(|span| x.checked_mul(span))
                        .and_then(|offset| offset.checked_div(width))
                        .and_then(|offset| offset.checked_add(start))
                        .ok_or_else(|| {
                            Error::Evaluation("Number/project_time arithmetic overflow".to_owned())
                        })?
                        .clamp(start.min(end), start.max(end))
                };
                Ok(EvalValue::Value(Value::Number(value)))
            }
            "Number/project_width" => {
                let segment_start = self.required_number_arg(
                    args,
                    "start_time",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let segment_end = self.required_number_arg(
                    args, "end_time", row, event, output, consumer, bindings, work,
                )?;
                let viewport_start = self.required_number_arg(
                    args,
                    "viewport_start",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let viewport_end = self.required_number_arg(
                    args,
                    "viewport_end",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let canvas_width = self.required_number_arg(
                    args,
                    "canvas_width",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let fallback = self.required_number_arg(
                    args, "fallback", row, event, output, consumer, bindings, work,
                )?;
                let _zoom = self
                    .eval_named_arg(args, "zoom", row, event, output, consumer, bindings, work)?;
                let viewport_span = viewport_end.checked_sub(viewport_start).ok_or_else(|| {
                    Error::Evaluation("Number/project_width viewport overflow".to_owned())
                })?;
                let segment_span = segment_end.checked_sub(segment_start).ok_or_else(|| {
                    Error::Evaluation("Number/project_width segment overflow".to_owned())
                })?;
                let value = if viewport_span <= 0 || segment_span <= 0 || canvas_width <= 0 {
                    fallback
                } else {
                    segment_span
                        .checked_mul(canvas_width)
                        .and_then(|width| width.checked_div(viewport_span))
                        .ok_or_else(|| {
                            Error::Evaluation("Number/project_width arithmetic overflow".to_owned())
                        })?
                        .clamp(0, canvas_width)
                };
                Ok(EvalValue::Value(Value::Number(value)))
            }
            "List/count" | "List/length" => {
                let input = input.ok_or_else(|| {
                    Error::Evaluation(format!("{function} requires an input list"))
                })?;
                let value =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                Ok(EvalValue::Value(Value::Number(
                    eval_to_list(value)?.len() as i64
                )))
            }
            "List/any" => {
                let input = input.ok_or_else(|| {
                    Error::Evaluation("List/any requires an input list".to_owned())
                })?;
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let binding = self
                    .eval_named_arg(
                        args, "binding", row, event, output, consumer, bindings, work,
                    )?
                    .map(|value| eval_to_text(&value))
                    .transpose()?
                    .ok_or_else(|| Error::Evaluation("List/any requires `binding`".to_owned()))?;
                let predicate = args
                    .iter()
                    .find(|arg| arg.name.as_deref() == Some("if"))
                    .ok_or_else(|| Error::Evaluation("List/any requires `if`".to_owned()))?;
                let previous = bindings.get(&binding).cloned();
                let mut matched = false;
                let items = eval_to_list(input)?;
                work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                for item in items {
                    bindings.insert(binding.clone(), item);
                    let include = self.eval_row_expression(
                        &predicate.value,
                        row,
                        event,
                        output,
                        consumer,
                        bindings,
                        work,
                    )?;
                    if eval_to_bool(&include)? {
                        matched = true;
                        break;
                    }
                }
                match previous {
                    Some(previous) => {
                        bindings.insert(binding, previous);
                    }
                    None => {
                        bindings.remove(&binding);
                    }
                }
                Ok(EvalValue::Value(Value::Bool(matched)))
            }
            "List/retain" => {
                let input = input.ok_or_else(|| {
                    Error::Evaluation("List/retain requires an input list".to_owned())
                })?;
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let binding = self
                    .eval_named_arg(
                        args, "binding", row, event, output, consumer, bindings, work,
                    )?
                    .map(|value| eval_to_text(&value))
                    .transpose()?
                    .ok_or_else(|| {
                        Error::Evaluation("List/retain requires `binding`".to_owned())
                    })?;
                let predicate = args
                    .iter()
                    .find(|arg| arg.name.as_deref() == Some("if"))
                    .ok_or_else(|| Error::Evaluation("List/retain requires `if`".to_owned()))?;
                let previous = bindings.get(&binding).cloned();
                let mut retained = Vec::new();
                let items = eval_to_list(input)?;
                work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                for item in items {
                    bindings.insert(binding.clone(), item.clone());
                    let include = self.eval_row_expression(
                        &predicate.value,
                        row,
                        event,
                        output,
                        consumer,
                        bindings,
                        work,
                    )?;
                    if eval_to_bool(&include)? {
                        retained.push(item);
                    }
                }
                match previous {
                    Some(previous) => {
                        bindings.insert(binding, previous);
                    }
                    None => {
                        bindings.remove(&binding);
                    }
                }
                Ok(EvalValue::List(retained))
            }
            "List/filter_field_equal" | "List/filter_field_not_equal" => {
                let input = input.ok_or_else(|| {
                    Error::Evaluation(format!("{function} requires an input list"))
                })?;
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let field = self
                    .eval_named_arg(args, "field", row, event, output, consumer, bindings, work)?
                    .map(|value| eval_to_text(&value))
                    .transpose()?
                    .ok_or_else(|| Error::Evaluation(format!("{function} requires `field`")))?;
                let expected = self
                    .eval_named_arg(args, "value", row, event, output, consumer, bindings, work)?
                    .ok_or_else(|| Error::Evaluation(format!("{function} requires `value`")))?;
                let expected = self.materialize_eval(expected)?;
                let retain_equal = function == "List/filter_field_equal";
                let mut filtered = Vec::new();
                let items = eval_to_list(input)?;
                work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                for item in items {
                    let actual =
                        self.eval_object_field(item.clone(), &field, event, consumer, work)?;
                    let actual = self.materialize_eval(actual)?;
                    if (actual == expected) == retain_equal {
                        filtered.push(item);
                    }
                }
                Ok(EvalValue::List(filtered))
            }
            "List/filter_text_contains" => {
                let input = input.ok_or_else(|| {
                    Error::Evaluation("List/filter_text_contains requires a list".to_owned())
                })?;
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let field = self
                    .named_text_arg(args, "field", row, event, output, consumer, bindings, work)?
                    .ok_or_else(|| {
                        Error::Evaluation("List/filter_text_contains requires `field`".to_owned())
                    })?;
                let needle = self
                    .named_text_arg(args, "needle", row, event, output, consumer, bindings, work)?
                    .unwrap_or_default()
                    .to_lowercase();
                let prefer = self.named_text_arg(
                    args,
                    "prefer_field",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let empty_field = self.named_text_arg(
                    args,
                    "empty_field",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let empty_value = self.named_text_arg(
                    args,
                    "empty_value",
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                let mut filtered = Vec::new();
                let items = eval_to_list(input)?;
                work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                for item in items {
                    if needle.is_empty()
                        && let (Some(empty_field), Some(empty_value)) =
                            (empty_field.as_deref(), empty_value.as_deref())
                    {
                        let actual = self.eval_object_field(
                            item.clone(),
                            empty_field,
                            event,
                            consumer,
                            work,
                        )?;
                        if eval_to_text(&actual)? == empty_value {
                            filtered.push(item);
                        }
                        continue;
                    }
                    let actual =
                        self.eval_object_field(item.clone(), &field, event, consumer, work)?;
                    let primary_matches = eval_to_text(&actual)?.to_lowercase().contains(&needle);
                    let preferred_matches = match prefer.as_deref() {
                        Some(prefer) => self
                            .eval_object_field(item.clone(), prefer, event, consumer, work)
                            .ok()
                            .and_then(|value| eval_to_text(&value).ok())
                            .is_some_and(|value| value.to_lowercase().contains(&needle)),
                        None => false,
                    };
                    if primary_matches || preferred_matches {
                        filtered.push(item);
                    }
                }
                Ok(EvalValue::List(filtered))
            }
            "List/join_field" => {
                let input = input.ok_or_else(|| {
                    Error::Evaluation("List/join_field requires a list".to_owned())
                })?;
                let input =
                    self.eval_row_expression(input, row, event, output, consumer, bindings, work)?;
                let field = self
                    .named_text_arg(args, "field", row, event, output, consumer, bindings, work)?
                    .ok_or_else(|| {
                        Error::Evaluation("List/join_field requires `field`".to_owned())
                    })?;
                let separator = self
                    .named_text_arg(
                        args,
                        "separator",
                        row,
                        event,
                        output,
                        consumer,
                        bindings,
                        work,
                    )?
                    .unwrap_or_default();
                let empty = self
                    .named_text_arg(args, "empty", row, event, output, consumer, bindings, work)?
                    .unwrap_or_default();
                let items = eval_to_list(input)?;
                work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                if items.is_empty() {
                    return Ok(EvalValue::Value(Value::Text(empty)));
                }
                let mut values = Vec::new();
                for item in items {
                    let value = self.eval_object_field(item, &field, event, consumer, work)?;
                    values.push(eval_to_text(&value)?);
                }
                Ok(EvalValue::Value(Value::Text(values.join(&separator))))
            }
            _ => Err(Error::Evaluation(format!(
                "unsupported typed builtin `{function}`"
            ))),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_named_arg(
        &mut self,
        args: &[PlanRowCallArg],
        name: &str,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut BTreeMap<String, EvalValue>,
        work: &mut Work,
    ) -> Result<Option<EvalValue>, Error> {
        args.iter()
            .find(|arg| arg.name.as_deref() == Some(name))
            .map(|arg| {
                self.eval_row_expression(&arg.value, row, event, output, consumer, bindings, work)
            })
            .transpose()
    }

    #[allow(clippy::too_many_arguments)]
    fn named_text_arg(
        &mut self,
        args: &[PlanRowCallArg],
        name: &str,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut BTreeMap<String, EvalValue>,
        work: &mut Work,
    ) -> Result<Option<String>, Error> {
        self.eval_named_arg(args, name, row, event, output, consumer, bindings, work)?
            .map(|value| eval_to_text(&value))
            .transpose()
    }

    #[allow(clippy::too_many_arguments)]
    fn required_number_arg(
        &mut self,
        args: &[PlanRowCallArg],
        name: &str,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut BTreeMap<String, EvalValue>,
        work: &mut Work,
    ) -> Result<i64, Error> {
        self.eval_named_arg(args, name, row, event, output, consumer, bindings, work)?
            .ok_or_else(|| Error::Evaluation(format!("numeric builtin requires `{name}`")))
            .and_then(|value| eval_to_number(&value))
    }
}

impl Session {
    fn evaluate_update(
        &mut self,
        op: &PlanOp,
        row: Option<RowId>,
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<Option<Value>, Error> {
        let PlanOpKind::UpdateBranch {
            expression_kind,
            ordered_inputs,
            source_payload_field,
            update_constant_id,
            ..
        } = &op.kind
        else {
            return Err(Error::InvalidPlan(format!(
                "op {} is not an update branch",
                op.id.0
            )));
        };
        let value = match expression_kind {
            PlanExpressionKind::SourcePayload => {
                let field = source_payload_field.as_ref().ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "source-payload update {} has no payload field",
                        op.id.0
                    ))
                })?;
                let Some(value) = source_payload_value(&event.payload, field) else {
                    return Ok(None);
                };
                value
            }
            PlanExpressionKind::Const => {
                let constant = update_constant_id.ok_or_else(|| {
                    Error::InvalidPlan(format!("const update {} has no constant", op.id.0))
                })?;
                self.constant(constant)?
            }
            PlanExpressionKind::PreviousValue => {
                let input = self.single_update_input(op)?;
                self.eval_update_ref(&input, row, event, work)?
            }
            PlanExpressionKind::ReadPath => {
                let input = self.single_update_input(op)?;
                self.eval_update_ref(&input, row, event, work)?
            }
            PlanExpressionKind::BoolNot => {
                let input = self.single_update_input(op)?;
                Value::Bool(!value_to_bool(
                    &self.eval_update_ref(&input, row, event, work)?,
                )?)
            }
            PlanExpressionKind::TextToNumber => {
                let [input] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "TextToNumber update {} requires one text input",
                        op.id.0
                    )));
                };
                Value::Number(value_to_number(
                    &self.eval_update_ref(input, row, event, work)?,
                )?)
            }
            PlanExpressionKind::TextTrimOrPrevious => {
                let [input, previous] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "TextTrimOrPrevious update {} requires input and previous value",
                        op.id.0
                    )));
                };
                let input = self.eval_update_ref(input, row, event, work)?;
                let text = value_to_text(&input)?.trim().to_owned();
                if text.is_empty() {
                    self.eval_update_ref(previous, row, event, work)?
                } else {
                    Value::Text(text)
                }
            }
            PlanExpressionKind::PrefixPayloadConcat | PlanExpressionKind::PrefixRootConcat => {
                let [prefix, input, separator] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "concat update {} requires three ordered inputs",
                        op.id.0
                    )));
                };
                let prefix = value_to_text(&self.eval_update_ref(prefix, row, event, work)?)?;
                let input = value_to_text(&self.eval_update_ref(input, row, event, work)?)?;
                let separator = value_to_text(&self.eval_update_ref(separator, row, event, work)?)?;
                Value::Text(format!("{prefix}{separator}{input}"))
            }
            PlanExpressionKind::NumberInfix => {
                let [left, operator, right] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "number update {} requires left, operator, right",
                        op.id.0
                    )));
                };
                let left = EvalValue::Value(self.eval_update_ref(left, row, event, work)?);
                let operator = value_to_text(&self.eval_update_ref(operator, row, event, work)?)?;
                let right = EvalValue::Value(self.eval_update_ref(right, row, event, work)?);
                eval_number_infix(&operator, &left, &right)?
            }
            PlanExpressionKind::ProjectTime => {
                let [
                    pointer_x,
                    pointer_width,
                    viewport_start,
                    viewport_end,
                    fallback,
                ] = ordered_inputs.as_slice()
                else {
                    return Err(Error::InvalidPlan(format!(
                        "ProjectTime update {} requires five inputs",
                        op.id.0
                    )));
                };
                let x = value_to_number(&self.eval_update_ref(pointer_x, row, event, work)?)?;
                let width =
                    value_to_number(&self.eval_update_ref(pointer_width, row, event, work)?)?;
                let start =
                    value_to_number(&self.eval_update_ref(viewport_start, row, event, work)?)?;
                let end =
                    value_to_number(&self.eval_update_ref(viewport_end, row, event, work)?)?;
                let fallback = value_to_number(&self.eval_update_ref(fallback, row, event, work)?)?;
                if width <= 0 {
                    Value::Number(fallback)
                } else {
                    let span = end
                        .checked_sub(start)
                        .ok_or_else(|| Error::Evaluation("ProjectTime span overflow".to_owned()))?;
                    let projected = x
                        .checked_mul(span)
                        .and_then(|value| value.checked_div(width))
                        .and_then(|value| value.checked_add(start))
                        .ok_or_else(|| {
                            Error::Evaluation("ProjectTime arithmetic overflow".to_owned())
                        })?;
                    Value::Number(projected.clamp(start.min(end), start.max(end)))
                }
            }
            PlanExpressionKind::BytesLength => {
                let input = self.single_update_input(op)?;
                Value::Number(
                    value_to_bytes(&self.eval_update_ref(&input, row, event, work)?)?.len() as i64,
                )
            }
            PlanExpressionKind::BytesIsEmpty => {
                let input = self.single_update_input(op)?;
                Value::Bool(
                    value_to_bytes(&self.eval_update_ref(&input, row, event, work)?)?.is_empty(),
                )
            }
            PlanExpressionKind::BytesGet => {
                let input = self.single_update_input(op)?;
                let bytes = value_to_bytes(&self.eval_update_ref(&input, row, event, work)?)?;
                let index = update_constant_id
                    .map(|constant| self.constant(constant))
                    .transpose()?
                    .map(|value| value_to_number(&value))
                    .transpose()?
                    .ok_or_else(|| {
                        Error::InvalidPlan(format!("BytesGet update {} has no index", op.id.0))
                    })?;
                let index = nonnegative_usize(index, "byte index")?;
                Value::Number(i64::from(*bytes.get(index).ok_or_else(|| {
                    Error::Evaluation(format!("byte index {index} is out of range"))
                })?))
            }
            PlanExpressionKind::BytesSet => {
                let [input, index, value] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "BytesSet update {} requires three inputs",
                        op.id.0
                    )));
                };
                let mut bytes = value_to_bytes(&self.eval_update_ref(input, row, event, work)?)?;
                let index = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(index, row, event, work)?)?,
                    "byte index",
                )?;
                let value = value_to_number(&self.eval_update_ref(value, row, event, work)?)?;
                let value = u8::try_from(value).map_err(|_| {
                    Error::Evaluation(format!("byte value {value} is outside 0..=255"))
                })?;
                *bytes.get_mut(index).ok_or_else(|| {
                    Error::Evaluation(format!("byte index {index} is out of range"))
                })? = value;
                Value::Bytes(bytes)
            }
            PlanExpressionKind::BytesSlice => {
                let [input, offset, count] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "BytesSlice update {} requires three inputs",
                        op.id.0
                    )));
                };
                let bytes = value_to_bytes(&self.eval_update_ref(input, row, event, work)?)?;
                let offset = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(offset, row, event, work)?)?,
                    "byte offset",
                )?;
                let count = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(count, row, event, work)?)?,
                    "byte count",
                )?;
                Value::Bytes(checked_slice(&bytes, offset, count)?)
            }
            PlanExpressionKind::BytesTake | PlanExpressionKind::BytesDrop => {
                let [input, count] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "byte take/drop update {} requires two inputs",
                        op.id.0
                    )));
                };
                let bytes = value_to_bytes(&self.eval_update_ref(input, row, event, work)?)?;
                let count = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(count, row, event, work)?)?,
                    "byte count",
                )?;
                Value::Bytes(if *expression_kind == PlanExpressionKind::BytesTake {
                    bytes.into_iter().take(count).collect()
                } else {
                    bytes.into_iter().skip(count).collect()
                })
            }
            PlanExpressionKind::BytesZeros => {
                let [count] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "BytesZeros update {} requires a count",
                        op.id.0
                    )));
                };
                let count = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(count, row, event, work)?)?,
                    "byte count",
                )?;
                Value::Bytes(vec![0; count])
            }
            PlanExpressionKind::BytesToHex | PlanExpressionKind::BytesToBase64 => {
                let input = ordered_inputs
                    .first()
                    .cloned()
                    .unwrap_or(self.single_update_input(op)?);
                let bytes = value_to_bytes(&self.eval_update_ref(&input, row, event, work)?)?;
                Value::Text(if *expression_kind == PlanExpressionKind::BytesToHex {
                    encode_hex(&bytes)
                } else {
                    encode_base64(&bytes)
                })
            }
            PlanExpressionKind::BytesFromHex | PlanExpressionKind::BytesFromBase64 => {
                let input = ordered_inputs
                    .first()
                    .cloned()
                    .unwrap_or(self.single_update_input(op)?);
                let text = value_to_text(&self.eval_update_ref(&input, row, event, work)?)?;
                Value::Bytes(if *expression_kind == PlanExpressionKind::BytesFromHex {
                    decode_hex(&text)?
                } else {
                    decode_base64(&text)?
                })
            }
            PlanExpressionKind::TextToBytes | PlanExpressionKind::BytesToText => {
                let [input, encoding] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "text/bytes update {} requires input and encoding",
                        op.id.0
                    )));
                };
                let encoding = value_to_text(&self.eval_update_ref(encoding, row, event, work)?)?;
                validate_encoding(Some(&encoding))?;
                if *expression_kind == PlanExpressionKind::TextToBytes {
                    Value::Bytes(
                        value_to_text(&self.eval_update_ref(input, row, event, work)?)?
                            .into_bytes(),
                    )
                } else {
                    let bytes = value_to_bytes(&self.eval_update_ref(input, row, event, work)?)?;
                    Value::Text(
                        String::from_utf8(bytes).map_err(|error| {
                            Error::Evaluation(format!("invalid UTF-8: {error}"))
                        })?,
                    )
                }
            }
            PlanExpressionKind::BytesConcat | PlanExpressionKind::BytesEqual => {
                let [left, right] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "binary BYTES update {} requires two inputs",
                        op.id.0
                    )));
                };
                let mut left = value_to_bytes(&self.eval_update_ref(left, row, event, work)?)?;
                let right = value_to_bytes(&self.eval_update_ref(right, row, event, work)?)?;
                if *expression_kind == PlanExpressionKind::BytesEqual {
                    Value::Bool(left == right)
                } else {
                    left.extend(right);
                    Value::Bytes(left)
                }
            }
            PlanExpressionKind::BytesFind
            | PlanExpressionKind::BytesStartsWith
            | PlanExpressionKind::BytesEndsWith => {
                let [left, right] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "BYTES search update {} requires two inputs",
                        op.id.0
                    )));
                };
                let left = value_to_bytes(&self.eval_update_ref(left, row, event, work)?)?;
                let right = value_to_bytes(&self.eval_update_ref(right, row, event, work)?)?;
                match expression_kind {
                    PlanExpressionKind::BytesFind => find_bytes(&left, &right)
                        .map(|index| Value::Number(index as i64))
                        .unwrap_or_else(|| Value::Text("NaN".to_owned())),
                    PlanExpressionKind::BytesStartsWith => Value::Bool(left.starts_with(&right)),
                    PlanExpressionKind::BytesEndsWith => Value::Bool(left.ends_with(&right)),
                    _ => unreachable!(),
                }
            }
            PlanExpressionKind::BytesReadUnsigned | PlanExpressionKind::BytesReadSigned => {
                let [input, offset, count, endian] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "BYTES read update {} requires four inputs",
                        op.id.0
                    )));
                };
                let bytes = value_to_bytes(&self.eval_update_ref(input, row, event, work)?)?;
                let offset = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(offset, row, event, work)?)?,
                    "byte offset",
                )?;
                let count = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(count, row, event, work)?)?,
                    "byte count",
                )?;
                let endian = value_to_text(&self.eval_update_ref(endian, row, event, work)?)?;
                Value::Number(read_integer(
                    &bytes,
                    offset,
                    count,
                    &endian,
                    *expression_kind == PlanExpressionKind::BytesReadSigned,
                )?)
            }
            PlanExpressionKind::BytesWriteUnsigned | PlanExpressionKind::BytesWriteSigned => {
                let [input, offset, count, endian, value] = ordered_inputs.as_slice() else {
                    return Err(Error::InvalidPlan(format!(
                        "BYTES write update {} requires five inputs",
                        op.id.0
                    )));
                };
                let mut bytes = value_to_bytes(&self.eval_update_ref(input, row, event, work)?)?;
                let offset = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(offset, row, event, work)?)?,
                    "byte offset",
                )?;
                let count = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(count, row, event, work)?)?,
                    "byte count",
                )?;
                let endian = value_to_text(&self.eval_update_ref(endian, row, event, work)?)?;
                let value = value_to_number(&self.eval_update_ref(value, row, event, work)?)?;
                write_integer(&mut bytes, offset, count, &endian, value)?;
                Value::Bytes(bytes)
            }
            PlanExpressionKind::MatchConst => {
                self.evaluate_match_update(op, ordered_inputs, row, event, work)?
            }
            PlanExpressionKind::MatchValueConst | PlanExpressionKind::MatchTextIsEmptyConst => self
                .evaluate_value_match_update(
                    op,
                    *expression_kind,
                    ordered_inputs,
                    row,
                    event,
                    work,
                )?,
            PlanExpressionKind::MatchInfixConst => {
                self.evaluate_infix_match_update(op, ordered_inputs, row, event, work)?
            }
            PlanExpressionKind::ListFindValue => {
                self.evaluate_list_find_update(op, ordered_inputs, row, event, work)?
            }
            PlanExpressionKind::FileReadBytes
            | PlanExpressionKind::FileWriteBytes
            | PlanExpressionKind::HostEffect => {
                return Err(Error::Unsupported {
                    op: op.id,
                    detail: format!(
                        "{expression_kind:?} is excluded from the in-memory Session engine"
                    ),
                });
            }
            PlanExpressionKind::Unknown => {
                return Err(Error::Unsupported {
                    op: op.id,
                    detail: "unknown update expression".to_owned(),
                });
            }
        };
        if value == Value::Text("SKIP".to_owned()) {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }

    fn constant(&self, constant: PlanConstantId) -> Result<Value, Error> {
        self.metadata
            .constants
            .get(&constant)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("missing constant {}", constant.0)))
    }

    fn eval_update_ref(
        &mut self,
        value_ref: &ValueRef,
        row: Option<RowId>,
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let value = self.eval_value_ref(value_ref, row, Some(event), None, None, work)?;
        self.materialize_eval(value)
    }

    fn single_update_input(&self, op: &PlanOp) -> Result<ValueRef, Error> {
        let output = op.output.as_ref();
        let inputs = op
            .inputs
            .iter()
            .filter(|input| {
                !matches!(input, ValueRef::Source(_) | ValueRef::SourcePayload { .. })
                    && Some(*input) != output
            })
            .cloned()
            .collect::<Vec<_>>();
        match inputs.as_slice() {
            [input] => Ok(input.clone()),
            [] => op
                .output
                .as_ref()
                .filter(|output| op.inputs.iter().any(|input| input == *output))
                .cloned()
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "update op {} expected one value input, found 0",
                        op.id.0
                    ))
                }),
            _ => Err(Error::InvalidPlan(format!(
                "update op {} expected one value input, found {}",
                op.id.0,
                inputs.len()
            ))),
        }
    }

    fn evaluate_match_update(
        &mut self,
        op: &PlanOp,
        inputs: &[ValueRef],
        row: Option<RowId>,
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let (input, arms) = inputs
            .split_first()
            .ok_or_else(|| Error::InvalidPlan(format!("match update {} has no input", op.id.0)))?;
        let current = value_to_text(&self.eval_update_ref(input, row, event, work)?)?;
        let mut wildcard = None;
        for pair in arms.chunks_exact(2) {
            let pattern = value_to_text(&self.eval_update_ref(&pair[0], row, event, work)?)?;
            let value = self.eval_update_ref(&pair[1], row, event, work)?;
            if pattern == current {
                return Ok(value);
            }
            if pattern == "__" {
                wildcard = Some(value);
            }
        }
        wildcard.ok_or_else(|| {
            Error::Evaluation(format!(
                "match update {} has no arm for `{current}`",
                op.id.0
            ))
        })
    }

    fn evaluate_value_match_update(
        &mut self,
        op: &PlanOp,
        kind: PlanExpressionKind,
        inputs: &[ValueRef],
        row: Option<RowId>,
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let (input, arms) = inputs
            .split_first()
            .ok_or_else(|| Error::InvalidPlan(format!("match update {} has no input", op.id.0)))?;
        let input = value_to_text(&self.eval_update_ref(input, row, event, work)?)?;
        let current = if kind == PlanExpressionKind::MatchTextIsEmptyConst {
            if input.is_empty() { "True" } else { "False" }
        } else {
            input.as_str()
        };
        let mut cursor = 0usize;
        let mut selected = None;
        let mut wildcard = None;
        while cursor < arms.len() {
            let pattern = value_to_text(&self.eval_update_ref(
                arms.get(cursor).ok_or_else(|| {
                    Error::InvalidPlan(format!("match update {} has no arm pattern", op.id.0))
                })?,
                row,
                event,
                work,
            )?)?;
            cursor += 1;
            let value = self.eval_encoded_update(arms, &mut cursor, row, event, work)?;
            if pattern == current && selected.is_none() {
                selected = Some(value.clone());
            }
            if pattern == "__" {
                wildcard = Some(value);
            }
        }
        selected.or(wildcard).ok_or_else(|| {
            Error::Evaluation(format!(
                "match update {} has no arm for `{current}`",
                op.id.0
            ))
        })
    }

    fn evaluate_infix_match_update(
        &mut self,
        op: &PlanOp,
        inputs: &[ValueRef],
        row: Option<RowId>,
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let [left, operator, right, arms @ ..] = inputs else {
            return Err(Error::InvalidPlan(format!(
                "infix match update {} has malformed inputs",
                op.id.0
            )));
        };
        let left = self.eval_update_ref(left, row, event, work)?;
        let operator = value_to_text(&self.eval_update_ref(operator, row, event, work)?)?;
        let right = self.eval_update_ref(right, row, event, work)?;
        let current = compare_update_values(&left, &operator, &right)?;
        let current = if current { "True" } else { "False" };
        let mut cursor = 0usize;
        let mut wildcard = None;
        while cursor < arms.len() {
            let pattern = value_to_text(&self.eval_update_ref(&arms[cursor], row, event, work)?)?;
            cursor += 1;
            let value = self.eval_encoded_update(arms, &mut cursor, row, event, work)?;
            if pattern == current {
                return Ok(value);
            }
            if pattern == "__" {
                wildcard = Some(value);
            }
        }
        wildcard.ok_or_else(|| {
            Error::Evaluation(format!(
                "infix match update {} has no arm for `{current}`",
                op.id.0
            ))
        })
    }

    fn eval_encoded_update(
        &mut self,
        inputs: &[ValueRef],
        cursor: &mut usize,
        row: Option<RowId>,
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let tag_ref = inputs
            .get(*cursor)
            .ok_or_else(|| Error::InvalidPlan("encoded update expression has no tag".to_owned()))?;
        *cursor += 1;
        let tag = value_to_text(&self.eval_update_ref(tag_ref, row, event, work)?)?;
        match tag.as_str() {
            "ref" => {
                let value = inputs
                    .get(*cursor)
                    .ok_or_else(|| Error::InvalidPlan("encoded ref has no value".to_owned()))?;
                *cursor += 1;
                self.eval_update_ref(value, row, event, work)
            }
            "number_infix" => {
                let left = inputs.get(*cursor).ok_or_else(|| {
                    Error::InvalidPlan("encoded infix has no left input".to_owned())
                })?;
                let operator = inputs.get(*cursor + 1).ok_or_else(|| {
                    Error::InvalidPlan("encoded infix has no operator".to_owned())
                })?;
                let right = inputs.get(*cursor + 2).ok_or_else(|| {
                    Error::InvalidPlan("encoded infix has no right input".to_owned())
                })?;
                *cursor += 3;
                let left = EvalValue::Value(self.eval_update_ref(left, row, event, work)?);
                let operator = value_to_text(&self.eval_update_ref(operator, row, event, work)?)?;
                let right = EvalValue::Value(self.eval_update_ref(right, row, event, work)?);
                eval_number_infix(&operator, &left, &right)
            }
            "match_const" | "match_text_is_empty_const" => {
                let input = inputs
                    .get(*cursor)
                    .ok_or_else(|| Error::InvalidPlan("encoded match has no input".to_owned()))?;
                let arm_count = inputs.get(*cursor + 1).ok_or_else(|| {
                    Error::InvalidPlan("encoded match has no arm count".to_owned())
                })?;
                *cursor += 2;
                let input = value_to_text(&self.eval_update_ref(input, row, event, work)?)?;
                let input = if tag == "match_text_is_empty_const" {
                    if input.is_empty() { "True" } else { "False" }
                } else {
                    input.as_str()
                };
                let arm_count = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(arm_count, row, event, work)?)?,
                    "encoded match arm count",
                )?;
                let mut selected = None;
                let mut wildcard = None;
                for _ in 0..arm_count {
                    let pattern = inputs.get(*cursor).ok_or_else(|| {
                        Error::InvalidPlan("encoded match has no arm pattern".to_owned())
                    })?;
                    *cursor += 1;
                    let pattern = value_to_text(&self.eval_update_ref(pattern, row, event, work)?)?;
                    let value = self.eval_encoded_update(inputs, cursor, row, event, work)?;
                    if pattern == input && selected.is_none() {
                        selected = Some(value.clone());
                    }
                    if pattern == "__" {
                        wildcard = Some(value);
                    }
                }
                selected.or(wildcard).ok_or_else(|| {
                    Error::Evaluation(format!("encoded match has no arm for `{input}`"))
                })
            }
            "match_infix_const" => {
                let left = inputs.get(*cursor).ok_or_else(|| {
                    Error::InvalidPlan("encoded infix match has no left input".to_owned())
                })?;
                let operator = inputs.get(*cursor + 1).ok_or_else(|| {
                    Error::InvalidPlan("encoded infix match has no operator".to_owned())
                })?;
                let right = inputs.get(*cursor + 2).ok_or_else(|| {
                    Error::InvalidPlan("encoded infix match has no right input".to_owned())
                })?;
                let arm_count = inputs.get(*cursor + 3).ok_or_else(|| {
                    Error::InvalidPlan("encoded infix match has no arm count".to_owned())
                })?;
                *cursor += 4;
                let left = self.eval_update_ref(left, row, event, work)?;
                let operator = value_to_text(&self.eval_update_ref(operator, row, event, work)?)?;
                let right = self.eval_update_ref(right, row, event, work)?;
                let current = if compare_update_values(&left, &operator, &right)? {
                    "True"
                } else {
                    "False"
                };
                let arm_count = nonnegative_usize(
                    value_to_number(&self.eval_update_ref(arm_count, row, event, work)?)?,
                    "encoded infix match arm count",
                )?;
                let mut selected = None;
                let mut wildcard = None;
                for _ in 0..arm_count {
                    let pattern = inputs.get(*cursor).ok_or_else(|| {
                        Error::InvalidPlan("encoded infix match has no arm pattern".to_owned())
                    })?;
                    *cursor += 1;
                    let pattern = value_to_text(&self.eval_update_ref(pattern, row, event, work)?)?;
                    let value = self.eval_encoded_update(inputs, cursor, row, event, work)?;
                    if pattern == current && selected.is_none() {
                        selected = Some(value.clone());
                    }
                    if pattern == "__" {
                        wildcard = Some(value);
                    }
                }
                selected.or(wildcard).ok_or_else(|| {
                    Error::Evaluation(format!("encoded infix match has no arm for `{current}`"))
                })
            }
            other => Err(Error::Evaluation(format!(
                "unsupported encoded update expression `{other}`"
            ))),
        }
    }

    fn evaluate_list_find_update(
        &mut self,
        op: &PlanOp,
        inputs: &[ValueRef],
        row: Option<RowId>,
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let [
            ValueRef::List(list),
            ValueRef::Field(field),
            expected,
            ValueRef::Field(target),
            rest @ ..,
        ] = inputs
        else {
            return Err(Error::InvalidPlan(format!(
                "ListFindValue update {} has malformed inputs",
                op.id.0
            )));
        };
        let expected = self.eval_update_ref(expected, row, event, work)?;
        let candidates = self.lookup_index(*list, *field, &expected, None, work)?;
        if let Some(found) = candidates.first().copied() {
            return self.ensure_row_field(found, *target, Some(event), work);
        }
        if let Some(fallback) = rest.first() {
            self.eval_update_ref(fallback, row, event, work)
        } else {
            Ok(Value::Null)
        }
    }
}

impl Session {
    fn execute_mutation(
        &mut self,
        op: &PlanOp,
        event: &SourceEvent,
        event_targets: &[RowId],
        work: &mut Work,
    ) -> Result<(), Error> {
        work.consume(1)?;
        let PlanOpKind::ListOperation {
            operation_kind,
            append,
            remove,
            ..
        } = &op.kind
        else {
            return Err(Error::InvalidPlan(format!(
                "mutation op {} is not a list operation",
                op.id.0
            )));
        };
        match operation_kind {
            PlanListOperationKind::Append => {
                let append = append.as_ref().ok_or_else(|| {
                    Error::InvalidPlan(format!("append op {} has no descriptor", op.id.0))
                })?;
                if !self.mutation_trigger_accepts_source(&append.trigger, event.source) {
                    return Ok(());
                }
                let trigger = self.eval_update_ref(&append.trigger, None, event, work)?;
                if !trigger_is_active(&trigger, event.source, &append.trigger) {
                    return Ok(());
                }
                let Some(ValueRef::List(list)) = op.output else {
                    return Err(Error::InvalidPlan(format!(
                        "append op {} has no list output",
                        op.id.0
                    )));
                };
                let mut fields = BTreeMap::new();
                for field in &append.fields {
                    let field_id = field.field_id.ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "append op {} field `{}` has no FieldId",
                            op.id.0, field.name
                        ))
                    })?;
                    let value = match (&field.value_ref, field.constant_id) {
                        (Some(value_ref), None) => {
                            self.eval_update_ref(value_ref, None, event, work)?
                        }
                        (None, Some(constant)) => self.constant(constant)?,
                        _ => {
                            return Err(Error::InvalidPlan(format!(
                                "append op {} field {} has invalid value source",
                                op.id.0, field_id.0
                            )));
                        }
                    };
                    fields.insert(field_id, value);
                }
                self.append_row(list, fields, work)?;
            }
            PlanListOperationKind::Remove => {
                let remove = remove.as_ref().ok_or_else(|| {
                    Error::InvalidPlan(format!("remove op {} has no descriptor", op.id.0))
                })?;
                if !matches!(&remove.source, ValueRef::Source(source) if *source == event.source) {
                    return Ok(());
                }
                let Some(ValueRef::List(list)) = op.output else {
                    return Err(Error::InvalidPlan(format!(
                        "remove op {} has no list output",
                        op.id.0
                    )));
                };
                let candidates = if event_targets.is_empty() {
                    self.list_row_ids(list)
                } else {
                    event_targets
                        .iter()
                        .copied()
                        .filter(|row| row.list == list)
                        .collect()
                };
                work.consume(candidates.len().try_into().unwrap_or(u64::MAX))?;
                let mut removed = Vec::new();
                for row in candidates {
                    if self.evaluate_list_predicate(
                        &remove.predicate,
                        row,
                        Some(event),
                        None,
                        work,
                    )? {
                        removed.push(row);
                    }
                }
                for row in removed {
                    self.remove_row(row, work)?;
                }
            }
            PlanListOperationKind::Retain | PlanListOperationKind::Count => {}
        }
        Ok(())
    }

    fn mutation_trigger_accepts_source(&self, trigger: &ValueRef, source: SourceId) -> bool {
        match trigger {
            ValueRef::Source(trigger) => *trigger == source,
            ValueRef::SourcePayload {
                source_id: trigger,
                ..
            } => *trigger == source,
            ValueRef::Field(field) => self
                .metadata
                .root_computations
                .get(field)
                .filter(|op| source_event_transform_op(op))
                .is_none_or(|op| {
                    op.inputs
                        .iter()
                        .any(|input| matches!(input, ValueRef::Source(candidate) if *candidate == source))
                }),
            ValueRef::Constant(_) | ValueRef::State(_) | ValueRef::List(_) => true,
        }
    }

    fn append_row(
        &mut self,
        list_id: ListId,
        fields: BTreeMap<FieldId, Value>,
        work: &mut Work,
    ) -> Result<RowId, Error> {
        let slot = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list_id)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("missing list slot {}", list_id.0)))?;
        if slot
            .capacity
            .is_some_and(|capacity| self.list_row_ids(list_id).len() >= capacity)
        {
            return Err(Error::Evaluation(format!(
                "list {} capacity {} is exhausted",
                list_id.0,
                slot.capacity.unwrap_or_default()
            )));
        }
        let key = self
            .lists
            .get(&list_id)
            .map(|list| list.next_key.max(1))
            .ok_or_else(|| Error::Evaluation(format!("list {} is missing", list_id.0)))?;
        let row_id = RowId {
            list: list_id,
            key,
            generation: 1,
        };
        let mut row = Row {
            fields,
            ..Row::default()
        };
        for field in self.metadata.row_computations.keys() {
            if self.metadata.row_field_owner.get(field) == Some(&list_id) {
                row.derived.insert(*field, Currentness::Dirty);
            }
        }
        let previous_next_key = self
            .lists
            .get(&list_id)
            .map(|list| list.next_key)
            .ok_or_else(|| Error::Evaluation(format!("list {} is missing", list_id.0)))?;
        let was_structurally_touched = self.touched_lists.contains(&list_id);
        work.authority_undo.push(AuthorityUndo::AppendRow {
            row: row_id,
            previous_next_key,
            touched_list: was_structurally_touched,
        });
        let list = self
            .lists
            .get_mut(&list_id)
            .ok_or_else(|| Error::Evaluation(format!("list {} is missing", list_id.0)))?;
        list.next_key = key.saturating_add(1);
        list.order.push(row_id);
        list.rows.insert(row_id, row);
        self.touched_lists.insert(list_id);
        self.index_row(row_id)?;
        work.suppress_row_deltas.insert(row_id);
        self.initialize_indexed_states(row_id, work)?;
        work.suppress_row_deltas.remove(&row_id);

        let authority_fields = self.authority_fields_for_list(list_id);
        let row_authority = self
            .lists
            .get(&list_id)
            .and_then(|list| list.rows.get(&row_id))
            .ok_or_else(|| Error::Evaluation("appended row disappeared".to_owned()))?;
        let fields = authority_fields
            .iter()
            .filter_map(|field| {
                row_authority
                    .fields
                    .get(field)
                    .cloned()
                    .map(|value| (*field, value))
            })
            .collect();
        let next_key = self
            .lists
            .get(&list_id)
            .map(|list| list.next_key)
            .unwrap_or(row_id.key.saturating_add(1));
        if was_structurally_touched {
            work.authority_deltas.push(AuthorityDelta::InsertRow {
                row: RowAuthority {
                    id: row_id,
                    fields,
                    touched_fields: BTreeSet::new(),
                },
                index: self
                    .lists
                    .get(&list_id)
                    .map(|list| list.order.len().saturating_sub(1))
                    .unwrap_or(0)
                    .try_into()
                    .map_err(|_| {
                        Error::Evaluation("list insertion index exceeds u64".to_owned())
                    })?,
                next_key,
            });
        } else {
            work.authority_deltas.push(AuthorityDelta::ReplaceList {
                list_id,
                authority: self.list_authority(list_id)?,
            });
        }

        self.bind_row_sources(row_id, slot.scope_id)?;
        work.changed_rows.insert(row_id);
        if work.emit {
            let row = self
                .lists
                .get(&list_id)
                .and_then(|list| list.rows.get(&row_id))
                .expect("appended row exists");
            work.deltas.push(Delta::InsertRow {
                row: RowSnapshot {
                    id: row_id,
                    fields: row.fields.clone(),
                },
            });
            for (source, binding_id) in &row.bindings {
                work.deltas.push(Delta::BindSource {
                    row: row_id,
                    source: *source,
                    binding_id: *binding_id,
                });
            }
        }
        self.invalidate_list_structure(list_id, work);
        Ok(row_id)
    }

    fn bind_row_sources(&mut self, row: RowId, scope: Option<ScopeId>) -> Result<(), Error> {
        let sources = self
            .metadata
            .routes
            .values()
            .filter(|route| route.scope_id == scope && route.scoped)
            .map(|route| route.source_id)
            .collect::<Vec<_>>();
        let row_state = self
            .lists
            .get_mut(&row.list)
            .and_then(|list| list.rows.get_mut(&row))
            .ok_or_else(|| {
                Error::Evaluation(format!("cannot bind sources to missing row {row:?}"))
            })?;
        for source in sources {
            let binding_id = self.next_binding_id;
            self.next_binding_id = self.next_binding_id.saturating_add(1);
            row_state.bindings.insert(source, binding_id);
        }
        Ok(())
    }

    fn remove_row(&mut self, row: RowId, work: &mut Work) -> Result<(), Error> {
        let (removed_value, order_index, previous_next_key) = self
            .lists
            .get(&row.list)
            .and_then(|list| {
                Some((
                    list.rows.get(&row)?.clone(),
                    list.order.iter().position(|candidate| *candidate == row)?,
                    list.next_key,
                ))
            })
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "cannot remove missing row {}:{}:{}",
                    row.list.0, row.key, row.generation
                ))
            })?;
        let touched_fields = self
            .touched_row_fields
            .iter()
            .filter_map(|(candidate, field)| (*candidate == row).then_some(*field))
            .collect();
        let was_structurally_touched = self.touched_lists.contains(&row.list);
        work.authority_undo.push(AuthorityUndo::RemoveRow {
            row,
            value: removed_value,
            order_index,
            previous_next_key,
            touched_list: was_structurally_touched,
            touched_fields,
        });
        let removed = self
            .lists
            .get_mut(&row.list)
            .and_then(|list| {
                list.order.retain(|candidate| *candidate != row);
                list.rows.remove(&row)
            })
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "cannot remove missing row {}:{}:{}",
                    row.list.0, row.key, row.generation
                ))
            })?;
        self.touched_lists.insert(row.list);
        self.touched_row_fields
            .retain(|(candidate, _)| *candidate != row);
        if was_structurally_touched {
            work.authority_deltas.push(AuthorityDelta::RemoveRow {
                row,
                next_key: previous_next_key,
            });
        } else {
            work.authority_deltas.push(AuthorityDelta::ReplaceList {
                list_id: row.list,
                authority: self.list_authority(row.list)?,
            });
        }
        for (field, value) in &removed.fields {
            self.remove_index_value(row, *field, value);
            self.invalidate_row_field(row, *field, Some(value), None, work);
        }
        for field in removed.derived.keys() {
            self.dynamic_dependencies.clear(Consumer::Row(row, *field));
        }
        work.changed_rows.insert(row);
        if work.emit {
            for (source, binding_id) in removed.bindings {
                work.deltas.push(Delta::UnbindSource {
                    row,
                    source,
                    binding_id,
                });
            }
            work.deltas.push(Delta::RemoveRow { row });
        }
        self.invalidate_list_structure(row.list, work);
        Ok(())
    }

    fn evaluate_list_predicate(
        &mut self,
        predicate: &PlanListRemovePredicate,
        row: RowId,
        event: Option<&SourceEvent>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<bool, Error> {
        match predicate {
            PlanListRemovePredicate::AlwaysTrue => Ok(true),
            PlanListRemovePredicate::RowFieldBool { input } => {
                let value = self.eval_value_ref(input, Some(row), event, None, consumer, work)?;
                eval_to_bool(&value)
            }
            PlanListRemovePredicate::RowFieldBoolNot { input } => {
                let value = self.eval_value_ref(input, Some(row), event, None, consumer, work)?;
                Ok(!eval_to_bool(&value)?)
            }
            PlanListRemovePredicate::SelectedFilterVisibility {
                selector,
                row_field,
            } => {
                let selector =
                    self.eval_value_ref(selector, Some(row), event, None, consumer, work)?;
                let selector = eval_to_text(&selector)?;
                let row_value =
                    self.eval_value_ref(row_field, Some(row), event, None, consumer, work)?;
                let row_value = eval_to_bool(&row_value)?;
                match selector.as_str() {
                    "All" | "all" => Ok(true),
                    "Active" | "active" => Ok(!row_value),
                    "Completed" | "completed" => Ok(row_value),
                    _ => Ok(true),
                }
            }
            PlanListRemovePredicate::Unknown { summary } => Err(Error::Evaluation(format!(
                "unsupported list predicate: {summary}"
            ))),
        }
    }

    fn evaluate_list_retain(
        &mut self,
        op: &PlanOp,
        retain: &boon_plan::PlanListRetain,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let Some(ValueRef::List(list)) = op.output else {
            return Err(Error::InvalidPlan(format!(
                "retain op {} has no source list output",
                op.id.0
            )));
        };
        let ValueRef::Field(output) = retain.target else {
            return Err(Error::InvalidPlan(format!(
                "retain op {} target is not a field",
                op.id.0
            )));
        };
        let consumer = Some(Consumer::Root(output));
        self.register_list_dependency(consumer, list);
        let rows = self.list_row_ids(list);
        work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
        let mut values = Vec::new();
        for row in rows {
            if self.evaluate_list_predicate(&retain.predicate, row, event, consumer, work)? {
                values.push(row_identity_value(row));
            }
        }
        Ok(Value::List(values))
    }

    fn evaluate_list_count(
        &mut self,
        op: &PlanOp,
        count: &boon_plan::PlanListCount,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let Some(ValueRef::List(list)) = op.output else {
            return Err(Error::InvalidPlan(format!(
                "count op {} has no source list output",
                op.id.0
            )));
        };
        let ValueRef::Field(output) = count.target else {
            return Err(Error::InvalidPlan(format!(
                "count op {} target is not a field",
                op.id.0
            )));
        };
        let consumer = Some(Consumer::Root(output));
        self.register_list_dependency(consumer, list);
        let rows = self.list_row_ids(list);
        work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
        let mut total = 0i64;
        for row in rows {
            if self.evaluate_list_predicate(&count.predicate, row, event, consumer, work)? {
                total += 1;
            }
        }
        Ok(Value::Number(total))
    }

    fn evaluate_projection(
        &mut self,
        output: FieldId,
        op: PlanOpId,
        projection: &PlanListProjection,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let consumer = Some(Consumer::Root(output));
        match projection {
            PlanListProjection::Find {
                source_list,
                field,
                value,
            } => {
                let field = self.metadata.list_field(*source_list, field)?;
                let selector =
                    self.eval_value_ref(value, None, event, Some(output), consumer, work)?;
                let selector = self.materialize_eval(selector)?;
                let candidates =
                    self.lookup_index(*source_list, field, &selector, consumer, work)?;
                match candidates.first().copied() {
                    Some(row) => {
                        self.register_row_dependency(consumer, row, field);
                        Ok(row_identity_value(row))
                    }
                    None => Ok(Value::Null),
                }
            }
            PlanListProjection::Chunk {
                source_list,
                size,
                item_field,
                label_field,
            } => {
                if *size == 0 {
                    return Err(Error::InvalidPlan(format!(
                        "chunk projection {} has size zero",
                        op.0
                    )));
                }
                self.register_list_dependency(consumer, *source_list);
                let rows = self.list_row_ids(*source_list);
                work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
                let mut chunks = Vec::new();
                for (index, chunk) in rows.chunks(*size).enumerate() {
                    let items = chunk
                        .iter()
                        .map(|row| row_identity_value(*row))
                        .collect::<Vec<_>>();
                    chunks.push(Value::Record(BTreeMap::from([
                        (label_field.clone(), Value::Text(index.to_string())),
                        (item_field.clone(), Value::List(items)),
                    ])));
                }
                Ok(Value::List(chunks))
            }
            PlanListProjection::ChunkValue {
                source,
                size,
                item_field,
                label_field,
            } => {
                if *size == 0 {
                    return Err(Error::InvalidPlan(format!(
                        "chunk-value projection {} has size zero",
                        op.0
                    )));
                }
                let source =
                    self.eval_value_ref(source, None, event, Some(output), consumer, work)?;
                let source = self.materialize_eval(source)?;
                let Value::List(rows) = source else {
                    return Err(Error::Evaluation(format!(
                        "chunk-value projection {} source is not a list",
                        op.0
                    )));
                };
                work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
                let chunks = rows
                    .chunks(*size)
                    .enumerate()
                    .map(|(index, chunk)| {
                        Value::Record(BTreeMap::from([
                            (label_field.clone(), Value::Text(index.to_string())),
                            (item_field.clone(), Value::List(chunk.to_vec())),
                        ]))
                    })
                    .collect();
                Ok(Value::List(chunks))
            }
            PlanListProjection::Unknown { summary } => Err(Error::Unsupported {
                op,
                detail: format!("unknown list projection: {summary}"),
            }),
        }
    }
}

fn trigger_is_active(value: &Value, source: SourceId, trigger: &ValueRef) -> bool {
    match trigger {
        ValueRef::Source(expected) => *expected == source,
        _ => match value {
            Value::Null | Value::Bool(false) | Value::Error { .. } => false,
            Value::Text(value) if value == "SKIP" => false,
            _ => true,
        },
    }
}

fn expression_row(row: Option<RowId>) -> Option<RowId> {
    row
}

fn row_identity_value(row: RowId) -> Value {
    Value::Row {
        id: row,
        fields: BTreeMap::new(),
    }
}

fn normalize_host_output_value(value: Value) -> Result<Value, Error> {
    match value {
        Value::List(values) => values
            .into_iter()
            .map(normalize_host_output_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        Value::Record(fields) => fields
            .into_iter()
            .map(|(name, value)| Ok((name, normalize_host_output_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()
            .map(Value::Record),
        Value::MappedRow { fields, .. } => fields
            .into_iter()
            .map(|(name, value)| Ok((name, normalize_host_output_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()
            .map(Value::Record),
        Value::Row { .. } => Err(Error::Evaluation(
            "host list output exposes an unprojected runtime row; map it to explicit data fields"
                .to_owned(),
        )),
        Value::Null
        | Value::Bool(_)
        | Value::Number(_)
        | Value::Text(_)
        | Value::Bytes(_)
        | Value::Error { .. } => Ok(value),
    }
}

fn eval_row_id(value: &EvalValue) -> Option<RowId> {
    match value {
        EvalValue::Row(id) | EvalValue::MappedRow { id, .. } => Some(*id),
        EvalValue::Value(Value::Row { id, .. }) | EvalValue::Value(Value::MappedRow { id, .. }) => {
            Some(*id)
        }
        EvalValue::Value(_) | EvalValue::List(_) | EvalValue::Record(_) => None,
    }
}

fn eval_to_list(value: EvalValue) -> Result<Vec<EvalValue>, Error> {
    match value {
        EvalValue::List(values) => Ok(values),
        EvalValue::Value(Value::List(values)) => {
            Ok(values.into_iter().map(EvalValue::Value).collect())
        }
        other => Err(Error::Evaluation(format!("value {other:?} is not a list"))),
    }
}

fn eval_to_text(value: &EvalValue) -> Result<String, Error> {
    match value {
        EvalValue::Value(value) => value_to_text(value),
        other => Err(Error::Evaluation(format!(
            "value {other:?} is not text-like"
        ))),
    }
}

fn value_to_text(value: &Value) -> Result<String, Error> {
    match value {
        Value::Null => Ok(String::new()),
        Value::Bool(value) => Ok(if *value { "True" } else { "False" }.to_owned()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Text(value) => Ok(value.clone()),
        Value::Bytes(bytes) => String::from_utf8(bytes.clone())
            .map_err(|error| Error::Evaluation(format!("invalid UTF-8: {error}"))),
        Value::Error { code } => Ok(code.clone()),
        Value::List(_) | Value::Record(_) | Value::MappedRow { .. } | Value::Row { .. } => Err(
            Error::Evaluation("list or record cannot be converted to text".to_owned()),
        ),
    }
}

fn source_event_transform_op(op: &PlanOp) -> bool {
    matches!(
        &op.kind,
        PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::SourceEventTransform,
            ..
        }
    )
}

fn value_to_number(value: &Value) -> Result<i64, Error> {
    match value {
        Value::Number(value) => Ok(*value),
        Value::Text(value) => value
            .parse::<i64>()
            .map_err(|_| Error::Evaluation(format!("text `{value}` is not an integer"))),
        other => Err(Error::Evaluation(format!("value {other:?} is not numeric"))),
    }
}

fn value_to_bool(value: &Value) -> Result<bool, Error> {
    match value {
        Value::Bool(value) => Ok(*value),
        Value::Text(value) if value == "True" => Ok(true),
        Value::Text(value) if value == "False" => Ok(false),
        other => Err(Error::Evaluation(format!("value {other:?} is not boolean"))),
    }
}

fn value_to_bytes(value: &Value) -> Result<Vec<u8>, Error> {
    match value {
        Value::Bytes(value) => Ok(value.clone()),
        other => Err(Error::Evaluation(format!("value {other:?} is not BYTES"))),
    }
}

fn eval_to_number(value: &EvalValue) -> Result<i64, Error> {
    match value {
        EvalValue::Value(Value::Number(value)) => Ok(*value),
        EvalValue::Value(Value::Text(value)) => value
            .parse::<i64>()
            .map_err(|_| Error::Evaluation(format!("text `{value}` is not an integer"))),
        other => Err(Error::Evaluation(format!("value {other:?} is not numeric"))),
    }
}

fn eval_to_bool(value: &EvalValue) -> Result<bool, Error> {
    match value {
        EvalValue::Value(Value::Bool(value)) => Ok(*value),
        EvalValue::Value(Value::Text(value)) if value == "True" => Ok(true),
        EvalValue::Value(Value::Text(value)) if value == "False" => Ok(false),
        other => Err(Error::Evaluation(format!("value {other:?} is not boolean"))),
    }
}

fn eval_to_bytes(value: &EvalValue) -> Result<Vec<u8>, Error> {
    match value {
        EvalValue::Value(Value::Bytes(value)) => Ok(value.clone()),
        other => Err(Error::Evaluation(format!("value {other:?} is not BYTES"))),
    }
}

fn eval_number_infix(op: &str, left: &EvalValue, right: &EvalValue) -> Result<Value, Error> {
    for value in [left, right] {
        if let EvalValue::Value(Value::Error { code }) = value {
            return Ok(Value::Error { code: code.clone() });
        }
    }
    if matches!(left, EvalValue::Value(Value::Text(value)) if value == "NaN")
        || matches!(right, EvalValue::Value(Value::Text(value)) if value == "NaN")
    {
        return Ok(Value::Text("NaN".to_owned()));
    }
    if matches!(op, "==" | "!=") {
        let equal = eval_to_text(left)? == eval_to_text(right)?;
        return Ok(Value::Bool(if op == "==" { equal } else { !equal }));
    }
    let left_number = eval_to_number(left);
    let right_number = eval_to_number(right);
    if op == "+" && (left_number.is_err() || right_number.is_err()) {
        return Ok(Value::Text(format!(
            "{}{}",
            eval_to_text(left)?,
            eval_to_text(right)?
        )));
    }
    let left = left_number?;
    let right = right_number?;
    match op {
        "+" => left
            .checked_add(right)
            .map(Value::Number)
            .ok_or_else(|| Error::Evaluation("integer addition overflow".to_owned())),
        "-" => left
            .checked_sub(right)
            .map(Value::Number)
            .ok_or_else(|| Error::Evaluation("integer subtraction overflow".to_owned())),
        "*" => left
            .checked_mul(right)
            .map(Value::Number)
            .ok_or_else(|| Error::Evaluation("integer multiplication overflow".to_owned())),
        "/" if right == 0 => Ok(Value::Error {
            code: "div_by_zero".to_owned(),
        }),
        "%" if right == 0 => Ok(Value::Error {
            code: "mod_by_zero".to_owned(),
        }),
        "/" => Ok(Value::Number(left / right)),
        "%" => Ok(Value::Number(left % right)),
        ">" => Ok(Value::Bool(left > right)),
        ">=" => Ok(Value::Bool(left >= right)),
        "<" => Ok(Value::Bool(left < right)),
        "<=" => Ok(Value::Bool(left <= right)),
        _ => Err(Error::Evaluation(format!(
            "unsupported numeric operator `{op}`"
        ))),
    }
}

fn compare_numbers(left: i64, op: &str, right: i64) -> Result<bool, Error> {
    match op {
        "==" => Ok(left == right),
        "!=" => Ok(left != right),
        ">" => Ok(left > right),
        ">=" => Ok(left >= right),
        "<" => Ok(left < right),
        "<=" => Ok(left <= right),
        _ => Err(Error::Evaluation(format!("unsupported comparison `{op}`"))),
    }
}

fn compare_update_values(left: &Value, op: &str, right: &Value) -> Result<bool, Error> {
    match op {
        "==" => Ok(left == right),
        "!=" => Ok(left != right),
        ">" | ">=" | "<" | "<=" => {
            compare_numbers(value_to_number(left)?, op, value_to_number(right)?)
        }
        _ => Err(Error::Evaluation(format!("unsupported comparison `{op}`"))),
    }
}

fn select_pattern_matches(pattern: &PlanRowSelectPattern, value: &Value) -> bool {
    match pattern {
        PlanRowSelectPattern::Bool { value: expected } => value == &Value::Bool(*expected),
        PlanRowSelectPattern::Text { value: expected } => value == &Value::Text(expected.clone()),
        PlanRowSelectPattern::Number { value: expected } => value == &Value::Number(*expected),
        PlanRowSelectPattern::NaN => value == &Value::Text("NaN".to_owned()),
        PlanRowSelectPattern::Wildcard => true,
    }
}

fn nonnegative_usize(value: i64, context: &str) -> Result<usize, Error> {
    usize::try_from(value)
        .map_err(|_| Error::Evaluation(format!("{context} {value} is negative or too large")))
}

fn validate_encoding(encoding: Option<&str>) -> Result<(), Error> {
    if encoding.is_none_or(|encoding| {
        matches!(
            encoding.to_ascii_lowercase().as_str(),
            "utf8" | "utf-8" | "text" | "ascii"
        )
    }) {
        Ok(())
    } else {
        Err(Error::Evaluation(format!(
            "unsupported text encoding `{}`",
            encoding.unwrap_or_default()
        )))
    }
}

fn checked_slice(bytes: &[u8], offset: usize, count: usize) -> Result<Vec<u8>, Error> {
    let end = offset
        .checked_add(count)
        .ok_or_else(|| Error::Evaluation("byte range overflow".to_owned()))?;
    bytes.get(offset..end).map(<[u8]>::to_vec).ok_or_else(|| {
        Error::Evaluation(format!(
            "byte range {offset}..{end} exceeds length {}",
            bytes.len()
        ))
    })
}

fn read_integer(
    bytes: &[u8],
    offset: usize,
    count: usize,
    endian: &str,
    signed: bool,
) -> Result<i64, Error> {
    if count == 0 || count > 8 {
        return Err(Error::Evaluation(format!(
            "integer byte count {count} is outside 1..=8"
        )));
    }
    let slice = checked_slice(bytes, offset, count)?;
    let little = parse_endian(endian)?;
    let mut value = 0u64;
    if little {
        for (shift, byte) in slice.iter().enumerate() {
            value |= u64::from(*byte) << (shift * 8);
        }
    } else {
        for byte in slice {
            value = (value << 8) | u64::from(byte);
        }
    }
    if signed && count < 8 && value & (1u64 << (count * 8 - 1)) != 0 {
        value |= u64::MAX << (count * 8);
    }
    Ok(value as i64)
}

fn write_integer(
    bytes: &mut [u8],
    offset: usize,
    count: usize,
    endian: &str,
    value: i64,
) -> Result<(), Error> {
    if count == 0 || count > 8 {
        return Err(Error::Evaluation(format!(
            "integer byte count {count} is outside 1..=8"
        )));
    }
    let end = offset
        .checked_add(count)
        .ok_or_else(|| Error::Evaluation("byte range overflow".to_owned()))?;
    let target_len = bytes.len();
    let target = bytes.get_mut(offset..end).ok_or_else(|| {
        Error::Evaluation(format!(
            "byte range {offset}..{end} exceeds length {target_len}"
        ))
    })?;
    let little = parse_endian(endian)?;
    for (index, byte) in target.iter_mut().enumerate() {
        let shift = if little { index } else { count - index - 1 } * 8;
        *byte = ((value as u64 >> shift) & 0xff) as u8;
    }
    Ok(())
}

fn parse_endian(endian: &str) -> Result<bool, Error> {
    match endian.to_ascii_lowercase().as_str() {
        "little" | "little_endian" | "le" => Ok(true),
        "big" | "big_endian" | "be" => Ok(false),
        _ => Err(Error::Evaluation(format!(
            "unsupported byte order `{endian}`"
        ))),
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(text: &str) -> Result<Vec<u8>, Error> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        return Err(Error::Evaluation(
            "hex input has an odd number of digits".to_owned(),
        ));
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_digit(pair[0])?;
            let low = hex_digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_digit(digit: u8) -> Result<u8, Error> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => Err(Error::Evaluation(format!(
            "invalid hex digit `{}`",
            digit as char
        ))),
    }
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        output.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(value & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn decode_base64(text: &str) -> Result<Vec<u8>, Error> {
    let bytes = text.trim().as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(Error::Evaluation(
            "base64 input length is not divisible by four".to_owned(),
        ));
    }
    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks_exact(4) {
        let a = base64_digit(chunk[0])?;
        let b = base64_digit(chunk[1])?;
        let c = if chunk[2] == b'=' {
            0
        } else {
            base64_digit(chunk[2])?
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            base64_digit(chunk[3])?
        };
        let value =
            (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        output.push((value >> 16) as u8);
        if chunk[2] != b'=' {
            output.push((value >> 8) as u8);
        }
        if chunk[3] != b'=' {
            output.push(value as u8);
        }
    }
    Ok(output)
}

fn base64_digit(digit: u8) -> Result<u8, Error> {
    match digit {
        b'A'..=b'Z' => Ok(digit - b'A'),
        b'a'..=b'z' => Ok(digit - b'a' + 26),
        b'0'..=b'9' => Ok(digit - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(Error::Evaluation(format!(
            "invalid base64 digit `{}`",
            digit as char
        ))),
    }
}

fn coalesce_deltas(deltas: Vec<Delta>) -> Vec<Delta> {
    let mut output = Vec::with_capacity(deltas.len());
    let mut positions = BTreeMap::<ValueTarget, usize>::new();
    for delta in deltas {
        match delta {
            Delta::SetValue { target, value } => {
                if let Some(position) = positions.get(&target).copied() {
                    output[position] = Delta::SetValue { target, value };
                } else {
                    positions.insert(target, output.len());
                    output.push(Delta::SetValue { target, value });
                }
            }
            other => output.push(other),
        }
    }
    output
}

fn source_payload_value(payload: &SourcePayload, field: &SourcePayloadField) -> Option<Value> {
    match field {
        SourcePayloadField::Address => payload.address.clone().map(Value::Text),
        SourcePayloadField::Key => payload.key.clone().map(Value::Text),
        SourcePayloadField::Text => payload.text.clone().map(Value::Text),
        SourcePayloadField::Named(name) => payload.fields.get(name).cloned(),
        SourcePayloadField::Bytes => payload
            .fields
            .get("bytes")
            .or_else(|| payload.fields.get("Bytes"))
            .cloned(),
    }
}

fn source_payload_schema_field_name(field: &SourcePayloadField) -> &str {
    match field {
        SourcePayloadField::Address => "address",
        SourcePayloadField::Bytes => "bytes",
        SourcePayloadField::Key => "key",
        SourcePayloadField::Named(name) => name,
        SourcePayloadField::Text => "text",
    }
}

fn source_payload_field_from_effect_name(name: &str) -> SourcePayloadField {
    match name {
        "address" => SourcePayloadField::Address,
        "bytes" => SourcePayloadField::Bytes,
        "key" => SourcePayloadField::Key,
        "text" => SourcePayloadField::Text,
        _ => SourcePayloadField::Named(name.to_owned()),
    }
}

fn set_source_payload_value(
    payload: &mut SourcePayload,
    field: &SourcePayloadField,
    value: Value,
) -> Result<(), Error> {
    match (field, value) {
        (SourcePayloadField::Address, Value::Text(value)) => payload.address = Some(value),
        (SourcePayloadField::Key, Value::Text(value)) => payload.key = Some(value),
        (SourcePayloadField::Text, Value::Text(value)) => payload.text = Some(value),
        (SourcePayloadField::Bytes, value @ Value::Bytes(_)) => {
            payload.fields.insert("bytes".to_owned(), value);
        }
        (SourcePayloadField::Named(name), value) => {
            payload.fields.insert(name.clone(), value);
        }
        (field, value) => {
            return Err(Error::Evaluation(format!(
                "source payload field {field:?} cannot contain {value:?}"
            )));
        }
    }
    Ok(())
}
