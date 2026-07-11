use boon_plan::{
    FieldId, InitialValueKind, ListId, ListInitializerKind, ListStorageSlot, MachinePlan,
    PlanConstantId, PlanConstantValue, PlanDerivedExpression, PlanDerivedKind, PlanExpressionKind,
    PlanListOperationKind, PlanListProjection, PlanListRemovePredicate, PlanOp, PlanOpId,
    PlanOpKind, PlanRowCallArg, PlanRowExpression, PlanRowSelectPattern, PlanSourceGuard,
    RootOutputDemand, ScalarStorageSlot, ScopeId, SourceId, SourcePayloadField, SourceRoute,
    StateId, ValueRef,
};
use std::collections::{BTreeMap, BTreeSet};
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
    pub recomputed_targets: Vec<ValueTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Turn {
    pub sequence: u64,
    pub deltas: Vec<Delta>,
    pub metrics: TurnMetrics,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub states: BTreeMap<StateId, Value>,
    pub fields: BTreeMap<FieldId, Value>,
    pub lists: BTreeMap<ListId, Vec<RowSnapshot>>,
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
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            require_monotonic_sequences: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidPlan(String),
    InvalidEvent(String),
    Unsupported { op: PlanOpId, detail: String },
    Cycle { field: FieldId, row: Option<RowId> },
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
            Value::List(_) | Value::Record(_) | Value::Row { .. } | Value::Error { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct IndexKey {
    list: ListId,
    field: FieldId,
    value: ScalarKey,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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
    root_computations: BTreeMap<FieldId, PlanOp>,
    row_computations: BTreeMap<FieldId, PlanOp>,
    row_field_owner: BTreeMap<FieldId, ListId>,
    indexed_state_field: BTreeMap<StateId, FieldId>,
    indexed_state_owner: BTreeMap<StateId, ListId>,
    list_by_scope: BTreeMap<ScopeId, ListId>,
    list_labels: BTreeMap<ListId, String>,
    list_fields_by_name: BTreeMap<(ListId, String), Vec<FieldId>>,
    root_field_by_name: BTreeMap<String, Vec<FieldId>>,
    root_state_by_name: BTreeMap<String, Vec<StateId>>,
    routes: BTreeMap<SourceId, SourceRoute>,
    updates_by_source: BTreeMap<SourceId, Vec<PlanOp>>,
    mutations: Vec<PlanOp>,
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
        for slot in &plan.storage_layout.list_slots {
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
            }
        }
        for fields in list_fields_by_name.values_mut() {
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
        let mut root_state_by_name = BTreeMap::<String, Vec<StateId>>::new();
        for slot in plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| !slot.indexed)
        {
            if let Some(label) = state_labels.get(&slot.state_id) {
                for name in debug_name_variants(label) {
                    root_state_by_name
                        .entry(name)
                        .or_default()
                        .push(slot.state_id);
                }
            }
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
        let mut updates_by_source = BTreeMap::<SourceId, Vec<PlanOp>>::new();
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
                        row_computations.insert(field, op.clone());
                    } else {
                        root_computations.insert(field, op.clone());
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
                    root_computations.insert(field, op.clone());
                }
                PlanOpKind::UpdateBranch { .. } => {
                    for source in op.inputs.iter().filter_map(|input| match input {
                        ValueRef::Source(source) => Some(*source),
                        _ => None,
                    }) {
                        updates_by_source
                            .entry(source)
                            .or_default()
                            .push(op.clone());
                    }
                }
                PlanOpKind::ListOperation {
                    operation_kind,
                    retain,
                    count,
                    ..
                } => match operation_kind {
                    PlanListOperationKind::Append | PlanListOperationKind::Remove => {
                        mutations.push(op.clone());
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
                        root_computations.insert(*field, op.clone());
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
                        root_computations.insert(*field, op.clone());
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

        let mut root_field_by_name = BTreeMap::<String, Vec<FieldId>>::new();
        for field in root_computations.keys() {
            if let Some(label) = field_labels.get(field) {
                for name in debug_name_variants(label) {
                    root_field_by_name.entry(name).or_default().push(*field);
                }
            }
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
            root_field_by_name,
            root_state_by_name,
            routes,
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
        if !name.starts_with("store.")
            && let Some([field]) = self
                .root_field_by_name
                .get(&format!("store.{name}"))
                .map(Vec::as_slice)
        {
            return Ok(*field);
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

    fn root_state(&self, name: &str) -> Result<StateId, Error> {
        if !name.starts_with("store.")
            && let Some([state]) = self
                .root_state_by_name
                .get(&format!("store.{name}"))
                .map(Vec::as_slice)
        {
            return Ok(*state);
        }
        let states = self
            .root_state_by_name
            .get(name)
            .or_else(|| self.root_state_by_name.get(local_name(name)))
            .ok_or_else(|| Error::InvalidPlan(format!("no root state `{name}`")))?;
        match states.as_slice() {
            [state] => Ok(*state),
            _ => Err(Error::InvalidPlan(format!(
                "root state name `{name}` is ambiguous across StateIds {:?}",
                states
            ))),
        }
    }

    fn list_storage_field(&self, list: ListId, name: &str) -> Result<FieldId, Error> {
        let list_label = self
            .list_labels
            .get(&list)
            .ok_or_else(|| Error::InvalidPlan(format!("list {} has no debug label", list.0)))?;
        self.list_field(list, &format!("{list_label}.{name}"))
    }
}

fn debug_name_variants(label: &str) -> Vec<String> {
    let parts = label.split('.').collect::<Vec<_>>();
    (0..parts.len())
        .map(|start| parts[start..].join("."))
        .collect()
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

#[derive(Default)]
struct Work {
    emit: bool,
    deltas: Vec<Delta>,
    metrics: TurnMetrics,
    dirty_states: BTreeSet<StateId>,
    dirty_consumers: BTreeSet<Consumer>,
    changed_rows: BTreeSet<RowId>,
    suppress_row_deltas: BTreeSet<RowId>,
    recomputed_targets: BTreeSet<ValueTarget>,
}

impl Work {
    fn turn() -> Self {
        Self {
            emit: true,
            ..Self::default()
        }
    }

    fn finish_metrics(&mut self) {
        self.metrics.dirty_state_count = self.dirty_states.len();
        self.metrics.dirty_field_count = self.dirty_consumers.len();
        self.metrics.changed_row_count = self.changed_rows.len();
        self.metrics.recomputed_targets = self.recomputed_targets.iter().copied().collect();
    }
}

#[derive(Clone, Debug)]
enum EvalValue {
    Value(Value),
    Row(RowId),
    List(Vec<EvalValue>),
    Record(BTreeMap<String, EvalValue>),
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
    next_binding_id: u64,
}

impl Session {
    pub fn new(plan: MachinePlan, options: SessionOptions) -> Result<Self, Error> {
        if plan.version.major != boon_plan::PLAN_MAJOR_VERSION {
            return Err(Error::InvalidPlan(format!(
                "plan major version {} is not supported",
                plan.version.major
            )));
        }
        let metadata = Arc::new(Metadata::new(&plan)?);
        let mut session = Self {
            plan: Arc::new(plan),
            options,
            metadata,
            root_states: BTreeMap::new(),
            root_fields: BTreeMap::new(),
            lists: BTreeMap::new(),
            indexes: BTreeMap::new(),
            dynamic_dependencies: DynamicDependencies::default(),
            last_sequence: None,
            next_binding_id: 1,
        };
        let mut work = Work::default();
        session.initialize(&mut work)?;
        Ok(session)
    }

    pub fn plan(&self) -> &MachinePlan {
        &self.plan
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
        self.ensure_published_current(None, &mut Work::default())
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

    pub fn project_current(
        &mut self,
        targets: &[ValueTarget],
    ) -> Result<BTreeMap<ValueTarget, Value>, Error> {
        let mut values = BTreeMap::new();
        let mut work = Work::default();
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

    pub fn root_value_current(&mut self, name: &str) -> Result<Value, Error> {
        if self.metadata.root_field_by_name.contains_key(name)
            || self
                .metadata
                .root_field_by_name
                .contains_key(local_name(name))
        {
            let field = self.metadata.root_field(name)?;
            return self.ensure_root_field(field, None, &mut Work::default());
        }
        let state = self.metadata.root_state(name)?;
        self.root_states
            .get(&state)
            .cloned()
            .ok_or_else(|| Error::Evaluation(format!("root state `{name}` has no value")))
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

    fn initialize(&mut self, work: &mut Work) -> Result<(), Error> {
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
            self.initialize_indexed_states(row, work)?;
        }

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
            let value = self.ensure_root_field(field, None, work)?;
            self.root_states.insert(slot.state_id, value);
        }

        let demanded = self.metadata.published.iter().copied().collect::<Vec<_>>();
        for field in demanded {
            self.ensure_root_field(field, None, work)?;
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
        _work: &mut Work,
    ) -> Result<RowId, Error> {
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
                        self.materialize_eval(evaluated, None, work)?
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
                _ => self.initial_slot_value(&slot)?,
            };
            self.set_row_field(row, target, value, work)?;
        }
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
        mut event: SourceEvent,
        demanded_targets: &[ValueTarget],
    ) -> Result<Turn, Error> {
        self.validate_event(&event)?;
        let mut work = Work::turn();
        self.complete_target_payload(&mut event, &mut work)?;
        let targets = self.event_targets(&event, &mut work)?;

        let source_fields = self
            .metadata
            .source_derived_by_source
            .get(&event.source)
            .into_iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        for field in &source_fields {
            self.mark_root_dirty(*field, &mut work);
        }
        for field in source_fields {
            self.ensure_root_field(field, Some(&event), &mut work)?;
        }

        let updates = self
            .metadata
            .updates_by_source
            .get(&event.source)
            .cloned()
            .unwrap_or_default();
        let scoped_update_row = self
            .metadata
            .routes
            .get(&event.source)
            .and_then(|route| route.scope_id)
            .and_then(|_| event.target.or_else(|| targets.first().copied()));
        for op in updates {
            if op.indexed {
                let rows = self.indexed_update_targets(&op, &event, &targets)?;
                self.execute_indexed_update_batch(&op, &rows, &event, &mut work)?;
            } else {
                self.execute_update(&op, scoped_update_row, &event, &mut work)?;
            }
        }

        let mutations = self.metadata.mutations.clone();
        for op in mutations {
            self.execute_mutation(&op, &event, &targets, &mut work)?;
        }

        self.ensure_demanded_current(demanded_targets, Some(&event), &mut work)?;

        self.last_sequence = Some(event.sequence);
        work.finish_metrics();
        Ok(Turn {
            sequence: event.sequence,
            deltas: coalesce_deltas(work.deltas),
            metrics: work.metrics,
        })
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
        let Some(name) = route
            .payload_schema
            .row_lookup_field_name()
            .map(str::to_owned)
        else {
            return Ok(());
        };
        if source_payload_value(&event.payload, &SourcePayloadField::Address).is_some() {
            return Ok(());
        }
        let field = self.metadata.list_field(row.list, &name)?;
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
        if !self.metadata.routes.contains_key(&event.source) {
            return Err(Error::InvalidEvent(format!(
                "source {} is not in the plan",
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
        let Some(lookup_name) = route.payload_schema.row_lookup_field_name() else {
            return Ok(Vec::new());
        };
        let field = self.metadata.list_field(list, lookup_name)?;
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
            self.set_row_field(row, field, value, work)?;
        } else {
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
            self.set_row_field(row, field, value, work)?;
        }
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
        let value = match evaluated.and_then(|value| self.materialize_eval(value, event, work)) {
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
                self.materialize_eval(value, event, work)?
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
        self.lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .and_then(|row| row.fields.get(&field))
            .cloned()
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "row {}:{}:{} has no field {}",
                    row.list.0, row.key, row.generation, field.0
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

    fn materialize_eval(
        &mut self,
        value: EvalValue,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        match value {
            EvalValue::Value(value) => Ok(value),
            EvalValue::Row(row) => Ok(row_identity_value(row)),
            EvalValue::List(values) => values
                .into_iter()
                .map(|value| self.materialize_eval(value, event, work))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List),
            EvalValue::Record(values) => values
                .into_iter()
                .map(|(name, value)| Ok((name, self.materialize_eval(value, event, work)?)))
                .collect::<Result<BTreeMap<_, _>, Error>>()
                .map(Value::Record),
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
                Ok(EvalValue::List(
                    self.list_row_ids(*list)
                        .into_iter()
                        .map(EvalValue::Row)
                        .collect(),
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
                            .is_some_and(source_event_transform_op)
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
                        .is_some_and(source_event_transform_op)
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
                Ok(EvalValue::List(
                    self.list_row_ids(*list_id)
                        .into_iter()
                        .map(EvalValue::Row)
                        .collect(),
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
                let key = self.materialize_eval(key, event, work)?;
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
                let previous = bindings.get(binding).cloned();
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    bindings.insert(binding.clone(), item);
                    values.push(self.eval_row_expression(
                        value, row, event, output, consumer, bindings, work,
                    )?);
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
                let mut total = 0i64;
                for item in eval_to_list(input)? {
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
                let input_value = self.materialize_eval(input, event, work)?;
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
            EvalValue::Value(Value::Record(mut record)) => record
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
                for item in eval_to_list(input)? {
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
                let expected = self.materialize_eval(expected, event, work)?;
                let retain_equal = function == "List/filter_field_equal";
                let mut filtered = Vec::new();
                for item in eval_to_list(input)? {
                    let actual =
                        self.eval_object_field(item.clone(), &field, event, consumer, work)?;
                    let actual = self.materialize_eval(actual, event, work)?;
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
                for item in eval_to_list(input)? {
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
            PlanExpressionKind::FileReadBytes | PlanExpressionKind::FileWriteBytes => {
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
        self.materialize_eval(value, Some(event), work)
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
            "match_const" => {
                let input = inputs
                    .get(*cursor)
                    .ok_or_else(|| Error::InvalidPlan("encoded match has no input".to_owned()))?;
                let arm_count = inputs.get(*cursor + 1).ok_or_else(|| {
                    Error::InvalidPlan("encoded match has no arm count".to_owned())
                })?;
                *cursor += 2;
                let input = value_to_text(&self.eval_update_ref(input, row, event, work)?)?;
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
        let list = self
            .lists
            .get_mut(&list_id)
            .ok_or_else(|| Error::Evaluation(format!("list {} is missing", list_id.0)))?;
        list.next_key = key.saturating_add(1);
        list.order.push(row_id);
        list.rows.insert(row_id, row);
        self.index_row(row_id)?;
        work.suppress_row_deltas.insert(row_id);
        self.initialize_indexed_states(row_id, work)?;
        work.suppress_row_deltas.remove(&row_id);

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
        let mut values = Vec::new();
        for row in self.list_row_ids(list) {
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
        let mut total = 0i64;
        for row in self.list_row_ids(list) {
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
                let selector = self.materialize_eval(selector, event, work)?;
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
                let source = self.materialize_eval(source, event, work)?;
                let Value::List(rows) = source else {
                    return Err(Error::Evaluation(format!(
                        "chunk-value projection {} source is not a list",
                        op.0
                    )));
                };
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
        _ => !matches!(
            value,
            Value::Null | Value::Bool(false) | Value::Error { .. }
        ),
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
        Value::List(_) | Value::Record(_) | Value::Row { .. } => Err(Error::Evaluation(
            "list or record cannot be converted to text".to_owned(),
        )),
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
    if text.len() % 2 != 0 {
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
    if bytes.len() % 4 != 0 {
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
