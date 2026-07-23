use boon_data::{
    Bytes, NumberTextFormat, format_number_ascii_text, format_number_text, number_bit_width,
};
use boon_document_model::{
    Axis, DocumentFrame, DocumentNode, DocumentNodeId as FrameNodeId, DocumentNodeKind,
    DocumentPatch, EmbeddedProgramDescriptor, EmbeddedProgramSourceUnit, MapCamera, MapCoordinate,
    MapHitIdentity, MapInteractionPolicy, MapOverlayDescriptor, MapOverlayGeometry, MapOverlayId,
    MapOverlayPaint, MapTileSourceId, MapTileSourceRef, MapViewportBounds, MapViewportDescriptor,
    MapViewportGeneration, MaterializedRange, ProgramArtifactRetention, ProgramCapabilityProfile,
    SourceBinding, SourceBindingId, StyleMap, StylePatch, StyleValue, TextInputFocusRequest,
    TextInputId, TextValue,
};
use boon_plan::{
    DocumentArgumentRole, DocumentBuiltin, DocumentConstantId, DocumentConstantValue,
    DocumentConstructor, DocumentElementContextId, DocumentExprId, DocumentExprOp,
    DocumentFunctionId, DocumentMaterialization, DocumentMaterializationId,
    DocumentMaterializationSource, DocumentNameId, DocumentPattern, DocumentRead,
    DocumentRowIdentity, DocumentRuntimeLocalBinding, DocumentScalarOp, DocumentTemplateId,
    FieldId, FiniteReal, ImportId, ListId, MachinePlan, PlanRowExpressionId, ScopeId, SourceId,
    ValueRef,
};
use boon_plan_executor::{
    Delta, ExpressionLocalBinding, MachineInstance, RowId, Value, ValueTarget,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

const DEFAULT_VISIBLE_ITEMS: u64 = 16;
const DEFAULT_OVERSCAN_ITEMS: u64 = 4;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DocumentDependency {
    Value(ValueTarget),
    DistributedImport(ImportId),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ScopedMaterializationOwner {
    materialization: DocumentMaterializationId,
    parent_instance: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct ScopedMaterializationPartition {
    generation: u64,
    logical_len: usize,
}

#[derive(Clone, Debug, Default)]
struct ScopedMaterializationIdentities {
    next_generation: u64,
    partitions: BTreeMap<ScopedMaterializationOwner, ScopedMaterializationPartition>,
}

impl ScopedMaterializationIdentities {
    fn reconcile(
        &mut self,
        owner: ScopedMaterializationOwner,
        logical_len: usize,
    ) -> Result<u64, DocumentError> {
        let partition = self.partitions.entry(owner).or_default();
        if partition.generation == 0 || logical_len < partition.logical_len {
            self.next_generation = self.next_generation.checked_add(1).ok_or_else(|| {
                DocumentError::Evaluation(
                    "scoped document materialization generation overflow".to_owned(),
                )
            })?;
            partition.generation = self.next_generation;
        }
        partition.logical_len = logical_len;
        Ok(partition.generation)
    }

    fn retain_active(&mut self, active: &BTreeSet<ScopedMaterializationOwner>) {
        self.partitions.retain(|owner, _| active.contains(owner));
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DocumentItemIdentity {
    Stored(RowId),
    Scoped {
        scope: ScopeId,
        key: u64,
        generation: u64,
    },
}

impl DocumentItemIdentity {
    fn instance_fragment(&self) -> String {
        match self {
            Self::Stored(row) => {
                format!("row-{}-{}-{}", row.list.0, row.key, row.generation)
            }
            Self::Scoped {
                scope,
                key,
                generation,
            } => format!("scope-{}-{key}-{generation}", scope.0),
        }
    }
}

type RetainedScalarEvaluation = (
    EvalValue,
    BTreeSet<DocumentDependency>,
    BTreeMap<ValueTarget, BTreeSet<Value>>,
);
type ScalarDependentIndexes = (
    BTreeMap<DocumentDependency, BTreeSet<RetainedBindingKey>>,
    BTreeMap<ValueTarget, BTreeSet<RetainedBindingKey>>,
    BTreeMap<(ValueTarget, Value), BTreeSet<RetainedBindingKey>>,
);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentWindowDemand {
    pub materialization: DocumentMaterializationId,
    pub visible: Range<u64>,
    pub overscan: Range<u64>,
}

impl DocumentWindowDemand {
    pub fn new(materialization: DocumentMaterializationId, visible: Range<u64>) -> Self {
        let start = visible.start.saturating_sub(DEFAULT_OVERSCAN_ITEMS);
        let end = visible.end.saturating_add(DEFAULT_OVERSCAN_ITEMS);
        Self {
            materialization,
            visible,
            overscan: start..end,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DocumentMaterializationStats {
    pub logical_rows: usize,
    pub materialized_rows: usize,
    pub materialized_nodes: usize,
    pub full_evaluation_count: u64,
    pub retained_scalar_evaluation_count: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct DocumentRuntime {
    machine_plan: Arc<MachinePlan>,
    expression_ops: Vec<Arc<DocumentExprOp>>,
    routes: BTreeMap<SourceId, String>,
    field_names: BTreeMap<FieldId, Vec<String>>,
    field_state_aliases: BTreeMap<FieldId, boon_plan::StateId>,
    field_owners: BTreeMap<FieldId, ListId>,
    list_scopes: BTreeMap<ListId, ScopeId>,
    row_sources: BTreeMap<ListId, Vec<(String, SourceId)>>,
    windows: BTreeMap<DocumentMaterializationId, DocumentWindowDemand>,
    last_nonempty_windows: BTreeMap<DocumentMaterializationId, DocumentWindowDemand>,
    empty_source_windows: BTreeSet<DocumentMaterializationId>,
    frame: DocumentFrame,
    dependencies: BTreeSet<DocumentDependency>,
    structural_dependencies: BTreeSet<DocumentDependency>,
    structural_lists: BTreeSet<ListId>,
    structural_list_fields: BTreeSet<(ListId, FieldId)>,
    retained_nodes: BTreeMap<FrameNodeId, RetainedNode>,
    scalar_dependents: BTreeMap<DocumentDependency, BTreeSet<RetainedBindingKey>>,
    scalar_guarded_dependents: BTreeMap<ValueTarget, BTreeSet<RetainedBindingKey>>,
    scalar_guard_values: BTreeMap<(ValueTarget, Value), BTreeSet<RetainedBindingKey>>,
    target_values: BTreeMap<DocumentDependency, Value>,
    scoped_materialization_identities: ScopedMaterializationIdentities,
    full_evaluation_count: u64,
    retained_scalar_evaluation_count: u64,
    stats: DocumentMaterializationStats,
}

pub(crate) struct DocumentRollback(DocumentRollbackKind);

enum DocumentRollbackKind {
    Unchanged,
    Scalar {
        retained_nodes: Vec<(FrameNodeId, RetainedNode)>,
        frame_nodes: Vec<(FrameNodeId, DocumentNode)>,
        target_values: Vec<(DocumentDependency, Option<Value>)>,
        retained_scalar_evaluation_count: u64,
    },
    Rebuilt {
        evaluated: Box<EvaluatedDocument>,
        full_evaluation_count: u64,
    },
}

impl DocumentRollback {
    pub(crate) fn unchanged() -> Self {
        Self(DocumentRollbackKind::Unchanged)
    }

    pub(crate) fn is_unchanged(&self) -> bool {
        matches!(self.0, DocumentRollbackKind::Unchanged)
    }
}

enum ScalarPatchOutcome {
    Patched {
        patches: Vec<DocumentPatch>,
        retained_nodes: Vec<(FrameNodeId, RetainedNode)>,
        frame_nodes: Vec<(FrameNodeId, DocumentNode)>,
        retained_scalar_evaluation_count: u64,
    },
    Rebuild,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DocumentError {
    InvalidPlan(String),
    Evaluation(String),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPlan(detail) => write!(formatter, "invalid DocumentPlan: {detail}"),
            Self::Evaluation(detail) => write!(formatter, "document evaluation failed: {detail}"),
        }
    }
}

impl std::error::Error for DocumentError {}

impl DocumentRuntime {
    pub(crate) fn new(session: &mut MachineInstance) -> Result<Option<Self>, DocumentError> {
        let machine = session.shared_plan();
        let Some(plan) = machine.document_plan() else {
            return Ok(None);
        };
        let expression_ops = plan
            .expressions
            .iter()
            .map(|expression| Arc::new(expression.op.clone()))
            .collect();
        let routes = machine
            .source_routes
            .iter()
            .map(|route| (route.source_id, route.path.clone()))
            .collect();
        let field_names = field_name_index(&machine);
        let field_state_aliases = field_state_alias_index(&machine);
        let field_owners = machine
            .storage_layout
            .list_slots
            .iter()
            .flat_map(|slot| {
                slot.row_fields
                    .iter()
                    .map(|field| (field.field_id, slot.list_id))
            })
            .collect();
        let list_scopes = machine
            .storage_layout
            .list_slots
            .iter()
            .filter_map(|slot| slot.scope_id.map(|scope| (slot.list_id, scope)))
            .collect();
        let row_sources = machine
            .source_routes
            .iter()
            .filter_map(|route| {
                let scope = route.scope_id?;
                let list = machine
                    .storage_layout
                    .list_slots
                    .iter()
                    .find(|slot| slot.scope_id == Some(scope))?
                    .list_id;
                Some((list, (route.path.clone(), route.source_id)))
            })
            .fold(
                BTreeMap::<ListId, Vec<(String, SourceId)>>::new(),
                |mut sources, (list, source)| {
                    sources.entry(list).or_default().push(source);
                    sources
                },
            );
        let windows: BTreeMap<DocumentMaterializationId, DocumentWindowDemand> = plan
            .materializations
            .iter()
            .map(|materialization| {
                let visible = 0..DEFAULT_VISIBLE_ITEMS;
                let overscan = 0..DEFAULT_VISIBLE_ITEMS.saturating_add(DEFAULT_OVERSCAN_ITEMS);
                (
                    materialization.id,
                    DocumentWindowDemand {
                        materialization: materialization.id,
                        visible,
                        overscan,
                    },
                )
            })
            .collect();
        let root = frame_node_id(plan.root.node.0, None);
        let last_nonempty_windows = windows.clone();
        let mut runtime = Self {
            machine_plan: machine,
            expression_ops,
            routes,
            field_names,
            field_state_aliases,
            field_owners,
            list_scopes,
            row_sources,
            windows,
            last_nonempty_windows,
            empty_source_windows: BTreeSet::new(),
            frame: DocumentFrame::empty(root.0.clone()),
            dependencies: BTreeSet::new(),
            structural_dependencies: BTreeSet::new(),
            structural_lists: BTreeSet::new(),
            structural_list_fields: BTreeSet::new(),
            retained_nodes: BTreeMap::new(),
            scalar_dependents: BTreeMap::new(),
            scalar_guarded_dependents: BTreeMap::new(),
            scalar_guard_values: BTreeMap::new(),
            target_values: BTreeMap::new(),
            scoped_materialization_identities: ScopedMaterializationIdentities::default(),
            full_evaluation_count: 0,
            retained_scalar_evaluation_count: 0,
            stats: DocumentMaterializationStats::default(),
        };
        let evaluated = runtime.evaluate(session)?;
        runtime.install_evaluated(evaluated);
        runtime.full_evaluation_count = 1;
        Ok(Some(runtime))
    }

    pub(crate) fn frame(&self) -> &DocumentFrame {
        &self.frame
    }

    fn plan(&self) -> &boon_plan::DocumentPlan {
        self.machine_plan
            .document_plan()
            .expect("document runtime requires a document plan")
    }

    fn source_route_token(
        &self,
        session: &MachineInstance,
        source: SourceId,
        env: &EvalEnv,
    ) -> Result<boon_plan::SourceRouteToken, DocumentError> {
        if let Some(row) = env.active_row {
            return session
                .source_route_token_for_descendant_row(source, row)
                .map_err(|error| DocumentError::Evaluation(error.to_string()));
        }
        let route = self
            .machine_plan
            .source_routes
            .iter()
            .find(|route| route.source_id == source)
            .ok_or_else(|| {
                DocumentError::InvalidPlan(format!("source {} has no route", source.0))
            })?;
        let ancestors = route
            .owner
            .ancestors
            .iter()
            .map(|ancestor| {
                let row = env.rows.get(&ancestor.scope).copied().ok_or_else(|| {
                    DocumentError::Evaluation(format!(
                        "source {} owner scope {} is not active",
                        source.0, ancestor.scope.0
                    ))
                })?;
                if row.list != ancestor.list {
                    return Err(DocumentError::InvalidPlan(format!(
                        "source {} owner scope {} resolved list {}, expected {}",
                        source.0, ancestor.scope.0, row.list.0, ancestor.list.0
                    )));
                }
                Ok(row)
            })
            .collect::<Result<Vec<_>, DocumentError>>()?;
        session
            .source_route_token(source, &ancestors)
            .map_err(|error| DocumentError::Evaluation(error.to_string()))
    }

    pub(crate) fn stats(&self) -> DocumentMaterializationStats {
        DocumentMaterializationStats {
            full_evaluation_count: self.full_evaluation_count,
            retained_scalar_evaluation_count: self.retained_scalar_evaluation_count,
            ..self.stats
        }
    }

    pub(crate) fn demanded_targets(&self) -> Vec<ValueTarget> {
        self.dependencies
            .iter()
            .filter_map(|dependency| match *dependency {
                DocumentDependency::DistributedImport(_) => None,
                DocumentDependency::Value(target) => Some(match target {
                    ValueTarget::Field(field) => self
                        .field_state_aliases
                        .get(&field)
                        .copied()
                        .map(ValueTarget::State)
                        .unwrap_or(target),
                    _ => target,
                }),
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(crate) fn mount_patches(&self) -> Vec<DocumentPatch> {
        mount_patches(&self.frame)
    }

    fn resolve_row_source(&self, list: ListId, suffix: &str) -> Option<EvalValue> {
        let mut exact = None;
        let mut group = BTreeMap::new();
        for (path, source) in self.row_sources.get(&list)? {
            let Some(remainder) = row_source_remainder(path, suffix) else {
                continue;
            };
            if remainder.is_empty() {
                if exact.replace(*source).is_some() {
                    return None;
                }
            } else if !insert_row_source(&mut group, remainder, *source) {
                return None;
            }
        }
        match (exact, group.is_empty()) {
            (Some(source), true) => Some(EvalValue::Source(source)),
            (None, false) => Some(EvalValue::Record(group)),
            _ => None,
        }
    }

    pub(crate) fn apply_turn(
        &mut self,
        session: &mut MachineInstance,
        deltas: &[Delta],
    ) -> Result<(Vec<DocumentPatch>, DocumentRollback), DocumentError> {
        if self.turn_affects_structure(deltas) {
            return self.rebuild_transactional(session);
        }
        let target_values = self.capture_delta_values(deltas);
        let affected = self.affected_scalar_bindings(deltas);
        let outcome = if affected.is_empty() {
            ScalarPatchOutcome::Patched {
                patches: Vec::new(),
                retained_nodes: Vec::new(),
                frame_nodes: Vec::new(),
                retained_scalar_evaluation_count: self.retained_scalar_evaluation_count,
            }
        } else {
            self.patch_scalars(session, affected)?
        };
        let ScalarPatchOutcome::Patched {
            patches,
            retained_nodes,
            frame_nodes,
            retained_scalar_evaluation_count,
        } = outcome
        else {
            return self.rebuild_transactional(session);
        };
        self.record_delta_values(deltas);
        Ok((
            patches,
            DocumentRollback(DocumentRollbackKind::Scalar {
                retained_nodes,
                frame_nodes,
                target_values,
                retained_scalar_evaluation_count,
            }),
        ))
    }

    pub(crate) fn rollback_turn(&mut self, rollback: DocumentRollback) {
        match rollback.0 {
            DocumentRollbackKind::Unchanged => {}
            DocumentRollbackKind::Scalar {
                retained_nodes,
                frame_nodes,
                target_values,
                retained_scalar_evaluation_count,
            } => {
                for (id, retained) in retained_nodes {
                    self.retained_nodes.insert(id, retained);
                }
                for (id, node) in frame_nodes {
                    self.frame.nodes.insert(id, node);
                }
                for (target, previous) in target_values {
                    match previous {
                        Some(value) => {
                            self.target_values.insert(target, value);
                        }
                        None => {
                            self.target_values.remove(&target);
                        }
                    }
                }
                self.retained_scalar_evaluation_count = retained_scalar_evaluation_count;
                self.rebuild_scalar_dependency_indexes();
            }
            DocumentRollbackKind::Rebuilt {
                evaluated,
                full_evaluation_count,
            } => {
                self.replace_evaluated(*evaluated);
                self.full_evaluation_count = full_evaluation_count;
            }
        }
    }

    pub(crate) fn demand_window(
        &mut self,
        session: &mut MachineInstance,
        demand: DocumentWindowDemand,
    ) -> Result<Vec<DocumentPatch>, DocumentError> {
        if !self
            .plan()
            .materializations
            .iter()
            .any(|materialization| materialization.id == demand.materialization)
        {
            return Err(DocumentError::InvalidPlan(format!(
                "materialization {} does not exist",
                demand.materialization.0
            )));
        }
        if demand.visible.start > demand.visible.end
            || demand.overscan.start > demand.overscan.end
            || demand.overscan.start > demand.visible.start
            || demand.overscan.end < demand.visible.end
        {
            return Err(DocumentError::Evaluation(
                "materialization window must contain its visible range".to_owned(),
            ));
        }
        if self.windows.get(&demand.materialization) == Some(&demand) {
            return Ok(Vec::new());
        }
        let logical_source_is_empty = self.frame.nodes.values().any(|node| {
            node.materialized.iter().any(|range| {
                range.materialization == Some(demand.materialization.0)
                    && range.logical_item_count == 0
            })
        });
        if demand.overscan.start < demand.overscan.end {
            self.last_nonempty_windows
                .insert(demand.materialization, demand.clone());
            self.empty_source_windows.remove(&demand.materialization);
        } else if logical_source_is_empty {
            self.empty_source_windows.insert(demand.materialization);
        } else {
            self.empty_source_windows.remove(&demand.materialization);
        }
        self.windows.insert(demand.materialization, demand);
        self.rebuild(session)
    }

    fn rebuild(
        &mut self,
        session: &mut MachineInstance,
    ) -> Result<Vec<DocumentPatch>, DocumentError> {
        let evaluated = self.evaluate(session)?;
        let patches = diff_frames(&self.frame, &evaluated.frame);
        self.install_evaluated(evaluated);
        self.full_evaluation_count = self.full_evaluation_count.saturating_add(1);
        Ok(patches)
    }

    fn rebuild_transactional(
        &mut self,
        session: &mut MachineInstance,
    ) -> Result<(Vec<DocumentPatch>, DocumentRollback), DocumentError> {
        let evaluated = self.evaluate(session)?;
        let patches = diff_frames(&self.frame, &evaluated.frame);
        let previous = self.replace_evaluated(evaluated);
        let full_evaluation_count = self.full_evaluation_count;
        self.full_evaluation_count = self.full_evaluation_count.saturating_add(1);
        Ok((
            patches,
            DocumentRollback(DocumentRollbackKind::Rebuilt {
                evaluated: Box::new(previous),
                full_evaluation_count,
            }),
        ))
    }

    fn evaluate(&self, session: &mut MachineInstance) -> Result<EvaluatedDocument, DocumentError> {
        Evaluator::new(self, session).evaluate()
    }

    fn turn_affects_structure(&self, deltas: &[Delta]) -> bool {
        deltas.iter().any(|delta| match delta {
            Delta::SetValue { target, .. } => {
                self.structural_dependencies
                    .contains(&DocumentDependency::Value(*target))
                    || matches!(
                        target,
                        ValueTarget::RowField { row, field }
                            if self.structural_list_fields.contains(&(row.list, *field))
                    )
            }
            Delta::SetDistributedImport { import_id, .. } => self
                .structural_dependencies
                .contains(&DocumentDependency::DistributedImport(*import_id)),
            Delta::InsertRow { row } => {
                self.structural_lists.contains(&row.id.list)
                    || self.structural_dependencies.iter().any(|dependency| {
                        matches!(dependency, DocumentDependency::Value(ValueTarget::RowField { row: target_row, .. }) if target_row == &row.id)
                    })
            }
            Delta::RemoveRow { row } => self.structural_lists.contains(&row.list),
            Delta::BindSource { row, .. } | Delta::UnbindSource { row, .. } => {
                self.structural_lists.contains(&row.list)
            }
        })
    }

    fn install_evaluated(&mut self, evaluated: EvaluatedDocument) {
        self.replace_evaluated(evaluated);
    }

    fn replace_evaluated(&mut self, evaluated: EvaluatedDocument) -> EvaluatedDocument {
        let previous = EvaluatedDocument {
            frame: std::mem::replace(&mut self.frame, evaluated.frame),
            dependencies: std::mem::replace(&mut self.dependencies, evaluated.dependencies),
            structural_dependencies: std::mem::replace(
                &mut self.structural_dependencies,
                evaluated.structural_dependencies,
            ),
            structural_lists: std::mem::replace(
                &mut self.structural_lists,
                evaluated.structural_lists,
            ),
            structural_list_fields: std::mem::replace(
                &mut self.structural_list_fields,
                evaluated.structural_list_fields,
            ),
            retained_nodes: std::mem::replace(&mut self.retained_nodes, evaluated.retained_nodes),
            target_values: std::mem::replace(&mut self.target_values, evaluated.target_values),
            scoped_materialization_identities: std::mem::replace(
                &mut self.scoped_materialization_identities,
                evaluated.scoped_materialization_identities,
            ),
            stats: std::mem::replace(&mut self.stats, evaluated.stats),
        };
        (
            self.scalar_dependents,
            self.scalar_guarded_dependents,
            self.scalar_guard_values,
        ) = scalar_dependent_indexes(&self.retained_nodes);
        previous
    }

    fn rebuild_scalar_dependency_indexes(&mut self) {
        (
            self.scalar_dependents,
            self.scalar_guarded_dependents,
            self.scalar_guard_values,
        ) = scalar_dependent_indexes(&self.retained_nodes);
        self.dependencies = self
            .structural_dependencies
            .iter()
            .copied()
            .chain(self.scalar_dependents.keys().copied())
            .collect();
    }

    fn capture_delta_values(&self, deltas: &[Delta]) -> Vec<(DocumentDependency, Option<Value>)> {
        deltas
            .iter()
            .filter_map(|delta| {
                delta_dependency(delta).map(|(dependency, _)| {
                    (dependency, self.target_values.get(&dependency).cloned())
                })
            })
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect()
    }

    fn affected_scalar_bindings(&self, deltas: &[Delta]) -> BTreeSet<RetainedBindingKey> {
        let mut affected = BTreeSet::new();
        for delta in deltas {
            let Some((dependency, value)) = delta_dependency(delta) else {
                continue;
            };
            let Some(dependents) = self.scalar_dependents.get(&dependency) else {
                continue;
            };
            let DocumentDependency::Value(target) = dependency else {
                affected.extend(dependents.iter().cloned());
                continue;
            };
            let Some(guarded) = self.scalar_guarded_dependents.get(&target) else {
                affected.extend(dependents.iter().cloned());
                continue;
            };
            let Some(previous) = self.target_values.get(&dependency) else {
                affected.extend(dependents.iter().cloned());
                continue;
            };
            affected.extend(dependents.difference(guarded).cloned());
            if let Some(matches) = self.scalar_guard_values.get(&(target, previous.clone())) {
                affected.extend(matches.iter().cloned());
            }
            if let Some(matches) = self.scalar_guard_values.get(&(target, value.clone())) {
                affected.extend(matches.iter().cloned());
            }
        }
        affected
    }

    fn record_delta_values(&mut self, deltas: &[Delta]) {
        for delta in deltas {
            if let Some((dependency, value)) = delta_dependency(delta) {
                self.target_values.insert(dependency, value.clone());
            }
        }
    }

    fn patch_scalars(
        &mut self,
        session: &mut MachineInstance,
        affected: BTreeSet<RetainedBindingKey>,
    ) -> Result<ScalarPatchOutcome, DocumentError> {
        let requests = affected
            .iter()
            .map(|key| {
                let argument = self
                    .retained_nodes
                    .get(&key.node)
                    .and_then(|node| node.arguments.get(key.argument))
                    .ok_or_else(|| {
                        DocumentError::InvalidPlan(format!(
                            "retained binding {}:{} is missing",
                            key.node.0, key.argument
                        ))
                    })?;
                let binding = argument.binding.as_ref().ok_or_else(|| {
                    DocumentError::InvalidPlan(format!(
                        "retained argument {}:{} has no scalar binding",
                        key.node.0, key.argument
                    ))
                })?;
                Ok((
                    key.clone(),
                    argument.role,
                    binding.expression,
                    binding.environment.clone(),
                    binding.dependencies.clone(),
                    binding.guards.clone(),
                ))
            })
            .collect::<Result<Vec<_>, DocumentError>>()?;

        let evaluated_count = requests.len();
        let mut updates = Vec::with_capacity(evaluated_count);
        let mut evaluator = Evaluator::new(self, session);
        for (key, role, expression, mut environment, previous_dependencies, previous_guards) in
            requests
        {
            let (value, dependencies, guards) =
                evaluator.eval_retained_scalar(expression, &mut environment)?;
            if matches!(value, EvalValue::Nodes(_))
                || retained_value_affects_structure(role, &value)
            {
                drop(evaluator);
                return Ok(ScalarPatchOutcome::Rebuild);
            }
            updates.push((
                key,
                value,
                dependencies,
                previous_dependencies,
                guards,
                previous_guards,
            ));
        }
        drop(evaluator);

        let mut staged_retained_nodes = BTreeMap::<FrameNodeId, RetainedNode>::new();
        let mut changed_frame_nodes = BTreeSet::new();
        for (key, value, dependencies, _, guards, _) in &updates {
            let current = self
                .retained_nodes
                .get(&key.node)
                .and_then(|node| node.arguments.get(key.argument))
                .ok_or_else(|| {
                    DocumentError::InvalidPlan(format!(
                        "retained binding {}:{} disappeared",
                        key.node.0, key.argument
                    ))
                })?;
            let value_changed = current.value != *value;
            let dependencies_changed = current
                .binding
                .as_ref()
                .is_none_or(|binding| binding.dependencies != *dependencies);
            let guards_changed = current
                .binding
                .as_ref()
                .is_none_or(|binding| binding.guards != *guards);
            if !value_changed && !dependencies_changed && !guards_changed {
                continue;
            }
            if !staged_retained_nodes.contains_key(&key.node) {
                let retained = self.retained_nodes.get(&key.node).cloned().ok_or_else(|| {
                    DocumentError::InvalidPlan(format!("retained node {} disappeared", key.node.0))
                })?;
                staged_retained_nodes.insert(key.node.clone(), retained);
            }
            let argument = staged_retained_nodes
                .get_mut(&key.node)
                .and_then(|node| node.arguments.get_mut(key.argument))
                .ok_or_else(|| {
                    DocumentError::InvalidPlan(format!(
                        "retained binding {}:{} disappeared",
                        key.node.0, key.argument
                    ))
                })?;
            argument.value = value.clone();
            let binding = argument.binding.as_mut().expect("binding checked above");
            binding.dependencies.clone_from(dependencies);
            binding.guards.clone_from(guards);
            if value_changed {
                changed_frame_nodes.insert(key.node.clone());
            }
        }

        let mut patches = Vec::new();
        let mut staged_frame_nodes = Vec::with_capacity(changed_frame_nodes.len());
        for id in changed_frame_nodes {
            let previous = self.frame.nodes.get(&id).cloned().ok_or_else(|| {
                DocumentError::Evaluation(format!("retained node {} is not mounted", id.0))
            })?;
            let retained = staged_retained_nodes.get(&id).ok_or_else(|| {
                DocumentError::InvalidPlan(format!("retained node {} was not staged", id.0))
            })?;
            let next = self.rebuild_retained_node(session, &previous, retained)?;
            patches.extend(diff_node(&previous, &next));
            staged_frame_nodes.push((id, next));
        }

        let previous_retained_nodes = staged_retained_nodes
            .into_iter()
            .map(|(id, retained)| {
                let previous = self
                    .retained_nodes
                    .insert(id.clone(), retained)
                    .expect("staged retained node must replace an existing node");
                (id, previous)
            })
            .collect();
        let previous_frame_nodes = staged_frame_nodes
            .into_iter()
            .map(|(id, node)| {
                let previous = self
                    .frame
                    .nodes
                    .insert(id.clone(), node)
                    .expect("staged frame node must replace an existing node");
                (id, previous)
            })
            .collect();
        for (key, _, dependencies, previous_dependencies, guards, previous_guards) in &updates {
            self.replace_scalar_dependencies(
                key,
                previous_dependencies,
                dependencies,
                previous_guards,
                guards,
            );
        }
        self.retained_scalar_evaluation_count = self
            .retained_scalar_evaluation_count
            .saturating_add(evaluated_count as u64);
        Ok(ScalarPatchOutcome::Patched {
            patches,
            retained_nodes: previous_retained_nodes,
            frame_nodes: previous_frame_nodes,
            retained_scalar_evaluation_count: self
                .retained_scalar_evaluation_count
                .saturating_sub(evaluated_count as u64),
        })
    }

    fn replace_scalar_dependencies(
        &mut self,
        key: &RetainedBindingKey,
        previous: &BTreeSet<DocumentDependency>,
        next: &BTreeSet<DocumentDependency>,
        previous_guards: &BTreeMap<ValueTarget, BTreeSet<Value>>,
        next_guards: &BTreeMap<ValueTarget, BTreeSet<Value>>,
    ) {
        self.remove_scalar_guards(key, previous_guards);
        for target in previous.difference(next) {
            let remove_target = self
                .scalar_dependents
                .get_mut(target)
                .is_some_and(|dependents| {
                    dependents.remove(key);
                    dependents.is_empty()
                });
            if remove_target {
                self.scalar_dependents.remove(target);
                if !self.structural_dependencies.contains(target) {
                    self.dependencies.remove(target);
                }
            }
        }
        for target in next.difference(previous) {
            self.scalar_dependents
                .entry(*target)
                .or_default()
                .insert(key.clone());
            self.dependencies.insert(*target);
        }
        self.insert_scalar_guards(key, next_guards);
    }

    fn remove_scalar_guards(
        &mut self,
        key: &RetainedBindingKey,
        guards: &BTreeMap<ValueTarget, BTreeSet<Value>>,
    ) {
        for (target, values) in guards {
            let remove_target =
                self.scalar_guarded_dependents
                    .get_mut(target)
                    .is_some_and(|dependents| {
                        dependents.remove(key);
                        dependents.is_empty()
                    });
            if remove_target {
                self.scalar_guarded_dependents.remove(target);
            }
            for value in values {
                let guard = (*target, value.clone());
                let remove_guard =
                    self.scalar_guard_values
                        .get_mut(&guard)
                        .is_some_and(|dependents| {
                            dependents.remove(key);
                            dependents.is_empty()
                        });
                if remove_guard {
                    self.scalar_guard_values.remove(&guard);
                }
            }
        }
    }

    fn insert_scalar_guards(
        &mut self,
        key: &RetainedBindingKey,
        guards: &BTreeMap<ValueTarget, BTreeSet<Value>>,
    ) {
        for (target, values) in guards {
            self.scalar_guarded_dependents
                .entry(*target)
                .or_default()
                .insert(key.clone());
            for value in values {
                self.scalar_guard_values
                    .entry((*target, value.clone()))
                    .or_default()
                    .insert(key.clone());
            }
        }
    }

    fn rebuild_retained_node(
        &self,
        session: &MachineInstance,
        previous: &DocumentNode,
        retained: &RetainedNode,
    ) -> Result<DocumentNode, DocumentError> {
        let mut node = DocumentNode::new(previous.id.0.clone(), retained.kind.clone());
        node.parent = retained.parent.clone();
        if retained.kind == DocumentNodeKind::MapViewport {
            node.map_viewport = Some(Box::new(evaluate_map_viewport_descriptor(
                retained
                    .arguments
                    .iter()
                    .map(|argument| (argument.role, argument.value.clone())),
            )?));
        }
        for argument in &retained.arguments {
            apply_argument(
                self,
                session,
                &mut node,
                argument.name,
                argument.role,
                argument.value.clone(),
                &retained.environment,
            )?;
        }
        if node.kind == DocumentNodeKind::Button {
            node.style
                .entry("cursor".to_owned())
                .or_insert_with(|| StyleValue::Text("pointer".to_owned()));
        }
        if node.style.contains_key("to") {
            if let Some(url) = node.style.get("to").cloned() {
                node.style.insert("href".to_owned(), url);
            }
            node.style
                .insert("cursor".to_owned(), StyleValue::Text("pointer".to_owned()));
            node.style.insert("link".to_owned(), StyleValue::Bool(true));
        }
        node.children = previous.children.clone();
        node.scroll = previous.scroll;
        node.materialized = previous.materialized.clone();
        Ok(node)
    }
}

struct EvaluatedDocument {
    frame: DocumentFrame,
    dependencies: BTreeSet<DocumentDependency>,
    structural_dependencies: BTreeSet<DocumentDependency>,
    structural_lists: BTreeSet<ListId>,
    structural_list_fields: BTreeSet<(ListId, FieldId)>,
    retained_nodes: BTreeMap<FrameNodeId, RetainedNode>,
    target_values: BTreeMap<DocumentDependency, Value>,
    scoped_materialization_identities: ScopedMaterializationIdentities,
    stats: DocumentMaterializationStats,
}

fn delta_dependency(delta: &Delta) -> Option<(DocumentDependency, &Value)> {
    match delta {
        Delta::SetValue { target, value } => Some((DocumentDependency::Value(*target), value)),
        Delta::SetDistributedImport { import_id, value } => {
            Some((DocumentDependency::DistributedImport(*import_id), value))
        }
        Delta::InsertRow { .. }
        | Delta::RemoveRow { .. }
        | Delta::BindSource { .. }
        | Delta::UnbindSource { .. } => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
enum EvalValue {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    Bytes(Bytes),
    Enum(String),
    Record(BTreeMap<String, EvalValue>),
    MappedRow {
        id: RowId,
        fields: BTreeMap<String, EvalValue>,
    },
    Tagged(String, BTreeMap<String, EvalValue>),
    List(Vec<EvalValue>),
    RuntimeList {
        list: ListId,
        logical_len: u64,
    },
    Row {
        id: Option<RowId>,
        fields: BTreeMap<FieldId, Value>,
    },
    Source(SourceId),
    Nodes(Vec<FrameNodeId>),
}

#[derive(Clone, Debug, Default)]
struct EvalEnv {
    parameters: Arc<BTreeMap<boon_plan::DocumentParameterId, EvalValue>>,
    locals: Arc<BTreeMap<boon_plan::DocumentLocalId, EvalValue>>,
    matched: BTreeMap<usize, EvalValue>,
    rows: Arc<BTreeMap<ScopeId, RowId>>,
    active_row: Option<RowId>,
    parent: Option<FrameNodeId>,
    instance: Arc<Vec<String>>,
    element_context: Option<DocumentElementContextId>,
}

#[derive(Clone, Debug)]
struct RetainedNode {
    kind: DocumentNodeKind,
    parent: Option<FrameNodeId>,
    environment: EvalEnv,
    arguments: Vec<RetainedArgument>,
}

#[derive(Clone, Debug)]
struct RetainedArgument {
    name: DocumentNameId,
    role: DocumentArgumentRole,
    value: EvalValue,
    binding: Option<RetainedScalarBinding>,
}

#[derive(Clone, Debug)]
struct RetainedScalarBinding {
    expression: DocumentExprId,
    environment: EvalEnv,
    dependencies: BTreeSet<DocumentDependency>,
    guards: BTreeMap<ValueTarget, BTreeSet<Value>>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RetainedBindingKey {
    node: FrameNodeId,
    argument: usize,
}

enum ScalarTargetUse {
    Independent,
    Guarded(BTreeSet<Value>),
    Unguarded,
}

impl ScalarTargetUse {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unguarded, _) | (_, Self::Unguarded) => Self::Unguarded,
            (Self::Independent, other) | (other, Self::Independent) => other,
            (Self::Guarded(mut left), Self::Guarded(right)) => {
                left.extend(right);
                Self::Guarded(left)
            }
        }
    }
}

struct EvaluatedArgument {
    name: DocumentNameId,
    role: DocumentArgumentRole,
    value: EvalValue,
    binding: Option<RetainedScalarBinding>,
}

struct Evaluator<'a> {
    runtime: &'a DocumentRuntime,
    session: &'a mut MachineInstance,
    frame: DocumentFrame,
    projection_cache: BTreeMap<DocumentDependency, Value>,
    static_value_cache: BTreeMap<DocumentExprId, EvalValue>,
    dependencies: BTreeSet<DocumentDependency>,
    structural_dependencies: BTreeSet<DocumentDependency>,
    dependency_capture: Option<BTreeSet<DocumentDependency>>,
    structural_lists: BTreeSet<ListId>,
    structural_list_fields: BTreeSet<(ListId, FieldId)>,
    retained_nodes: BTreeMap<FrameNodeId, RetainedNode>,
    materialized_items: BTreeSet<DocumentItemIdentity>,
    scoped_materialization_identities: ScopedMaterializationIdentities,
    active_scoped_materialization_owners: BTreeSet<ScopedMaterializationOwner>,
    call_depth: usize,
}

impl<'a> Evaluator<'a> {
    fn new(runtime: &'a DocumentRuntime, session: &'a mut MachineInstance) -> Self {
        let root = frame_node_id(runtime.plan().root.node.0, None);
        Self {
            runtime,
            session,
            frame: DocumentFrame::empty(root.0),
            projection_cache: BTreeMap::new(),
            static_value_cache: BTreeMap::new(),
            dependencies: BTreeSet::new(),
            structural_dependencies: BTreeSet::new(),
            dependency_capture: None,
            structural_lists: BTreeSet::new(),
            structural_list_fields: BTreeSet::new(),
            retained_nodes: BTreeMap::new(),
            materialized_items: BTreeSet::new(),
            scoped_materialization_identities: runtime.scoped_materialization_identities.clone(),
            active_scoped_materialization_owners: BTreeSet::new(),
            call_depth: 0,
        }
    }

    fn evaluate(mut self) -> Result<EvaluatedDocument, DocumentError> {
        let root = self.frame.root.clone();
        let mut env = EvalEnv {
            parent: Some(root.clone()),
            instance: Arc::new(vec![format!(
                "root-{}",
                self.runtime.plan().root.template.0
            )]),
            ..EvalEnv::default()
        };
        let value = self.eval(self.runtime.plan().root.expression, &mut env)?;
        self.attach_nodes(&root, value);
        if let Some(root_node) = self.frame.nodes.get_mut(&root) {
            root_node
                .style
                .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
            root_node
                .style
                .insert("height".to_owned(), StyleValue::Text("Fill".to_owned()));
        }
        self.scoped_materialization_identities
            .retain_active(&self.active_scoped_materialization_owners);
        let stats = DocumentMaterializationStats {
            logical_rows: self.session.logical_row_count(),
            materialized_rows: self.materialized_items.len(),
            materialized_nodes: self.frame.nodes.len().saturating_sub(1),
            ..DocumentMaterializationStats::default()
        };
        Ok(EvaluatedDocument {
            frame: self.frame,
            dependencies: self.dependencies,
            structural_dependencies: self.structural_dependencies,
            structural_lists: self.structural_lists,
            structural_list_fields: self.structural_list_fields,
            retained_nodes: self.retained_nodes,
            target_values: self.projection_cache,
            scoped_materialization_identities: self.scoped_materialization_identities,
            stats,
        })
    }

    fn eval(
        &mut self,
        expression: DocumentExprId,
        env: &mut EvalEnv,
    ) -> Result<EvalValue, DocumentError> {
        if self.call_depth > 512 {
            return Err(DocumentError::Evaluation(
                "document expression recursion exceeded 512 calls".to_owned(),
            ));
        }
        let expression_plan = self
            .runtime
            .plan()
            .expressions
            .get(expression.0)
            .ok_or_else(|| {
                DocumentError::InvalidPlan(format!("expression {} is missing", expression.0))
            })?;
        let is_static = expression_plan.value_class == boon_plan::DocumentValueClass::Static;
        if is_static && let Some(value) = self.static_value_cache.get(&expression).cloned() {
            return Ok(value);
        }
        let op = self
            .runtime
            .expression_ops
            .get(expression.0)
            .ok_or_else(|| {
                DocumentError::InvalidPlan(format!("expression {} is missing", expression.0))
            })?
            .clone();
        self.call_depth += 1;
        let result = self.eval_op(expression, op.as_ref(), env);
        self.call_depth -= 1;
        let result = result.map_err(|error| match error {
            DocumentError::Evaluation(detail) => {
                DocumentError::Evaluation(format!("expression {}: {detail}", expression.0))
            }
            error => error,
        });
        if is_static && let Ok(value) = &result {
            self.static_value_cache.insert(expression, value.clone());
        }
        result
    }

    fn eval_op(
        &mut self,
        expression: DocumentExprId,
        op: &DocumentExprOp,
        env: &mut EvalEnv,
    ) -> Result<EvalValue, DocumentError> {
        match op {
            DocumentExprOp::Constant { constant } => self.constant(*constant),
            DocumentExprOp::Read { read } => self.read(read.clone(), env),
            DocumentExprOp::Project { input, field } => {
                let value = self.eval(*input, env)?;
                Ok(self.project(value, &[*field]))
            }
            DocumentExprOp::Record { fields } => {
                let mut record = BTreeMap::new();
                for field in fields {
                    let value = self.eval(field.value, env)?;
                    if field.spread {
                        spread_record(&mut record, value);
                    } else if let Some(name) = field.name {
                        record.insert(self.name(name)?.to_owned(), value);
                    }
                }
                Ok(EvalValue::Record(record))
            }
            DocumentExprOp::TaggedRecord { tag, fields } => {
                let mut record = BTreeMap::new();
                for field in fields {
                    let value = self.eval(field.value, env)?;
                    if field.spread {
                        spread_record(&mut record, value);
                    } else if let Some(name) = field.name {
                        record.insert(self.name(name)?.to_owned(), value);
                    }
                }
                Ok(EvalValue::Tagged(self.name(*tag)?.to_owned(), record))
            }
            DocumentExprOp::List { items } => {
                let mut list = Vec::new();
                for item in items {
                    let value = self.eval(item.value, env)?;
                    if item.spread {
                        spread_list(&mut list, value);
                    } else {
                        list.push(value);
                    }
                }
                Ok(EvalValue::List(list))
            }
            DocumentExprOp::TextTemplate { segments } => {
                let mut text = String::new();
                for segment in segments {
                    match segment {
                        boon_plan::DocumentTextSegment::Static { constant } => {
                            text.push_str(&self.constant(*constant)?.text())
                        }
                        boon_plan::DocumentTextSegment::Dynamic { value } => {
                            let value = self.eval(*value, env)?;
                            text.push_str(&value.text());
                        }
                    }
                }
                Ok(EvalValue::Text(text))
            }
            DocumentExprOp::LocalBlock { bindings, result } => {
                let old = Arc::clone(&env.locals);
                for binding in bindings {
                    let value = self.eval(binding.value, env)?;
                    Arc::make_mut(&mut env.locals).insert(binding.local, value);
                }
                let value = self.eval(*result, env);
                env.locals = old;
                value
            }
            DocumentExprOp::Builtin {
                builtin,
                input,
                arguments,
            } => {
                let input = (*input).map(|value| self.eval(value, env)).transpose()?;
                let mut values = Vec::new();
                for argument in arguments {
                    values.push((
                        self.name(argument.name)?.to_owned(),
                        self.eval(argument.value, env)?,
                    ));
                }
                self.eval_builtin_call(*builtin, input, values)
            }
            DocumentExprOp::Scalar {
                operation,
                left,
                right,
            } => {
                let left = self.eval(*left, env)?;
                let right = (*right).map(|value| self.eval(value, env)).transpose()?;
                Ok(eval_scalar(*operation, left, right))
            }
            DocumentExprOp::Select { input, arms } => {
                let input = self.eval(*input, env)?;
                for arm in arms {
                    if self.pattern_matches(&input, arm.pattern.clone())? {
                        let selector = self.runtime.plan().expressions[expression.0].compiler_id;
                        let mut arm_env = env.clone();
                        arm_env.matched.insert(selector, input.clone());
                        self.install_pattern_rows(&input, &arm.bindings, &mut arm_env)?;
                        return self.eval(arm.output, &mut arm_env);
                    }
                }
                Ok(EvalValue::Null)
            }
            DocumentExprOp::Latest { branches } => {
                for branch in branches {
                    let value = self.eval(*branch, env)?;
                    if !matches!(value, EvalValue::Null) {
                        return Ok(value);
                    }
                }
                Ok(EvalValue::Null)
            }
            DocumentExprOp::Then { input, output } => {
                let input = self.eval(*input, env)?;
                if input.truthy() {
                    (*output)
                        .map(|value| self.eval(value, env))
                        .unwrap_or(Ok(input))
                } else {
                    Ok(EvalValue::Null)
                }
            }
            DocumentExprOp::Constructor {
                template,
                constructor,
                element_context,
                arguments,
            } => self.constructor(
                *template,
                *constructor,
                *element_context,
                arguments.clone(),
                env,
            ),
            DocumentExprOp::Materialize { materialization } => {
                self.materialize(*materialization, env)
            }
            DocumentExprOp::RuntimeExpression {
                expression,
                bindings,
            } => self.runtime_expression(*expression, bindings, env),
            DocumentExprOp::NoElement => Ok(EvalValue::Nodes(Vec::new())),
        }
    }

    fn eval_builtin_call(
        &mut self,
        builtin: DocumentBuiltin,
        input: Option<EvalValue>,
        arguments: Vec<(String, EvalValue)>,
    ) -> Result<EvalValue, DocumentError> {
        let runtime_list = input
            .as_ref()
            .into_iter()
            .chain(arguments.iter().map(|(_, value)| value))
            .find_map(|value| match value {
                EvalValue::RuntimeList { list, logical_len } => Some((*list, *logical_len)),
                _ => None,
            });
        let Some((list, logical_len)) = runtime_list else {
            return Ok(eval_builtin(builtin, input, arguments));
        };
        match builtin {
            DocumentBuiltin::ListCount | DocumentBuiltin::ListLength => {
                Ok(EvalValue::Number(logical_len as f64))
            }
            DocumentBuiltin::ListIsNotEmpty => Ok(EvalValue::Bool(logical_len != 0)),
            DocumentBuiltin::ListGet => {
                let index = named_number(&arguments, "index").unwrap_or(0.0);
                if !index.is_finite() || index < 0.0 || index.fract() != 0.0 {
                    return Ok(EvalValue::Null);
                }
                self.runtime_list_row(list, logical_len, index as u64)
            }
            DocumentBuiltin::ListLatest => {
                if logical_len == 0 {
                    Ok(EvalValue::Null)
                } else {
                    self.runtime_list_row(list, logical_len, logical_len - 1)
                }
            }
            _ => Err(DocumentError::InvalidPlan(format!(
                "document builtin {builtin:?} received logical list {} without a bounded typed lowering",
                list.0
            ))),
        }
    }

    fn runtime_list_row(
        &mut self,
        list: ListId,
        logical_len: u64,
        index: u64,
    ) -> Result<EvalValue, DocumentError> {
        if index >= logical_len {
            return Ok(EvalValue::Null);
        }
        let end = index.checked_add(1).ok_or_else(|| {
            DocumentError::Evaluation("document list index overflowed".to_owned())
        })?;
        let (current_logical_len, mut rows) = self
            .session
            .list_row_snapshots_window_current(list, index..end)
            .map_err(|error| DocumentError::Evaluation(error.to_string()))?;
        if current_logical_len != logical_len {
            return Err(DocumentError::Evaluation(
                "document logical list changed during one bounded read".to_owned(),
            ));
        }
        let Some(row) = rows.pop() else {
            return Ok(EvalValue::Null);
        };
        Ok(EvalValue::Row {
            id: Some(row.id),
            fields: row.fields,
        })
    }

    fn runtime_expression(
        &mut self,
        expression: PlanRowExpressionId,
        bindings: &[DocumentRuntimeLocalBinding],
        env: &EvalEnv,
    ) -> Result<EvalValue, DocumentError> {
        let machine_plan = Arc::clone(&self.runtime.machine_plan);
        machine_plan
            .row_expressions
            .visit_inputs(expression, &mut |input| match input {
                ValueRef::State(state)
                | ValueRef::StateProjection {
                    state_id: state, ..
                } => {
                    let target = self.runtime_state_target(state, env);
                    self.record_dependency(DocumentDependency::Value(target));
                }
                ValueRef::Field(field) => {
                    let target = self.runtime_field_target(field, env);
                    self.record_dependency(DocumentDependency::Value(target));
                }
                ValueRef::List(list) => {
                    self.structural_lists.insert(list);
                }
                ValueRef::DistributedImport(import) => {
                    self.record_dependency(DocumentDependency::DistributedImport(import));
                }
                ValueRef::Source(_) | ValueRef::SourcePayload { .. } | ValueRef::Constant(_) => {}
            })
            .map_err(|error| DocumentError::InvalidPlan(error.to_string()))?;
        machine_plan
            .row_expressions
            .visit_list_fields(expression, &mut |list, field| {
                self.structural_list_fields.insert((list, field));
            })
            .map_err(|error| DocumentError::InvalidPlan(error.to_string()))?;

        let bindings = bindings
            .iter()
            .map(|binding| {
                let value = env.parameters.get(&binding.parameter).ok_or_else(|| {
                    DocumentError::InvalidPlan(format!(
                        "runtime expression local {}:{} references inactive parameter {}",
                        binding.owner.0, binding.local.0, binding.parameter.0
                    ))
                })?;
                let value = guard_value(value).ok_or_else(|| {
                    DocumentError::Evaluation(format!(
                        "runtime expression local {}:{} cannot cross the retained-value boundary",
                        binding.owner.0, binding.local.0
                    ))
                })?;
                Ok(ExpressionLocalBinding {
                    owner: binding.owner,
                    local: binding.local,
                    value,
                })
            })
            .collect::<Result<Vec<_>, DocumentError>>()?;
        self.session
            .evaluate_plan_expression_current(expression, env.active_row, &bindings)
            .map(machine_value_to_eval)
            .map_err(|error| DocumentError::Evaluation(error.to_string()))
    }

    fn runtime_field_target(&self, field: FieldId, env: &EvalEnv) -> ValueTarget {
        if let Some(row) = env.active_row
            && self.runtime.field_owners.get(&field) == Some(&row.list)
        {
            return ValueTarget::RowField { row, field };
        }
        self.runtime
            .field_state_aliases
            .get(&field)
            .copied()
            .map(|state| self.runtime_state_target(state, env))
            .unwrap_or(ValueTarget::Field(field))
    }

    fn runtime_state_target(&self, state: boon_plan::StateId, env: &EvalEnv) -> ValueTarget {
        if let Some(row) = env.active_row
            && let Some(slot) = self
                .runtime
                .machine_plan
                .storage_layout
                .scalar_slots
                .iter()
                .find(|slot| slot.state_id == state && slot.indexed)
            && slot.owner.ancestors.last().map(|owner| owner.list) == Some(row.list)
            && let Some(field) = slot.indexed_field_id
        {
            return ValueTarget::RowField { row, field };
        }
        ValueTarget::State(state)
    }

    fn constant(&self, id: DocumentConstantId) -> Result<EvalValue, DocumentError> {
        let value = &self
            .runtime
            .plan()
            .constants
            .get(id.0)
            .ok_or_else(|| DocumentError::InvalidPlan(format!("constant {} is missing", id.0)))?
            .value;
        Ok(match value {
            DocumentConstantValue::Text { value } => EvalValue::Text(value.clone()),
            DocumentConstantValue::Number { coefficient, scale } => {
                EvalValue::Number(*coefficient as f64 / 10f64.powi(*scale as i32))
            }
            DocumentConstantValue::Bool { value } => EvalValue::Bool(*value),
            DocumentConstantValue::Bytes { value } => EvalValue::Bytes(value.clone().into()),
            DocumentConstantValue::Enum { name } => EvalValue::Enum(self.name(*name)?.to_owned()),
        })
    }

    fn name(&self, id: DocumentNameId) -> Result<&str, DocumentError> {
        self.runtime
            .plan()
            .names
            .get(id.0)
            .map(String::as_str)
            .ok_or_else(|| DocumentError::InvalidPlan(format!("name {} is missing", id.0)))
    }

    fn read(&mut self, read: DocumentRead, env: &EvalEnv) -> Result<EvalValue, DocumentError> {
        match read {
            DocumentRead::State { state } => self.read_target(ValueTarget::State(state)),
            DocumentRead::Field { field } => {
                if let Some(row) = env.active_row
                    && self.runtime.field_owners.get(&field) == Some(&row.list)
                {
                    return self.read_target(ValueTarget::RowField { row, field });
                }
                if let Some(state) = self.runtime.field_state_aliases.get(&field).copied() {
                    return self.read_target(ValueTarget::State(state));
                }
                match self.read_target(ValueTarget::Field(field)) {
                    Ok(value) => Ok(value),
                    Err(error) => self
                        .runtime
                        .field_state_aliases
                        .get(&field)
                        .copied()
                        .map(|state| self.read_target(ValueTarget::State(state)))
                        .unwrap_or(Err(error)),
                }
            }
            DocumentRead::DistributedImport { import } => self.read_distributed_import(import),
            DocumentRead::List { list } => {
                self.structural_lists.insert(list);
                let logical_len = self
                    .session
                    .list_logical_len_current(list)
                    .map_err(|error| DocumentError::Evaluation(error.to_string()))?;
                Ok(EvalValue::RuntimeList { list, logical_len })
            }
            DocumentRead::Source { source } => Ok(EvalValue::Source(source)),
            DocumentRead::Parameter {
                parameter,
                projection,
            } => Ok(self.project(
                env.parameters
                    .get(&parameter)
                    .cloned()
                    .unwrap_or(EvalValue::Null),
                &projection,
            )),
            DocumentRead::Local { local, projection } => Ok(self.project(
                env.locals.get(&local).cloned().unwrap_or(EvalValue::Null),
                &projection,
            )),
            DocumentRead::Matched {
                selector,
                projection,
            } => Ok(self.project(
                env.matched
                    .get(&selector)
                    .cloned()
                    .unwrap_or(EvalValue::Null),
                &projection,
            )),
            DocumentRead::Row {
                scope,
                field,
                projection,
            } => {
                let row = env.rows.get(&scope).copied().ok_or_else(|| {
                    DocumentError::Evaluation(format!(
                        "row scope {} is not active while evaluating the document",
                        scope.0
                    ))
                })?;
                if let Some(field) = field {
                    let value = self.read_target(ValueTarget::RowField { row, field })?;
                    let projection = projection.get(1..).unwrap_or(&[]);
                    Ok(self.project(value, projection))
                } else {
                    let snapshot = self
                        .session
                        .row_snapshot(row)
                        .ok()
                        .map(|snapshot| EvalValue::Row {
                            id: Some(row),
                            fields: snapshot.fields.clone(),
                        })
                        .unwrap_or(EvalValue::Null);
                    Ok(self.project(snapshot, &projection))
                }
            }
            DocumentRead::ElementState {
                context,
                projection,
            } => {
                if env.element_context != Some(context) {
                    return Err(DocumentError::Evaluation(format!(
                        "element state context {:?} is not active",
                        context
                    )));
                }
                let state = EvalValue::Record(BTreeMap::from([
                    ("focused".to_owned(), EvalValue::Bool(false)),
                    ("hovered".to_owned(), EvalValue::Bool(false)),
                    ("pressed".to_owned(), EvalValue::Bool(false)),
                    ("selected".to_owned(), EvalValue::Bool(false)),
                ]));
                Ok(self.project(state, &projection))
            }
        }
    }

    fn read_target(&mut self, target: ValueTarget) -> Result<EvalValue, DocumentError> {
        let dependency = DocumentDependency::Value(target);
        if let Some(value) = self.projection_cache.get(&dependency).cloned() {
            self.record_dependency(dependency);
            return Ok(self.value(value));
        }
        let mut projected = self
            .session
            .project_current(&[target])
            .map_err(|error| DocumentError::Evaluation(error.to_string()))?;
        let value = projected.remove(&target).ok_or_else(|| {
            DocumentError::Evaluation(format!("value target {target:?} is not current"))
        })?;
        self.record_dependency(dependency);
        self.projection_cache.insert(dependency, value.clone());
        Ok(self.value(value))
    }

    fn read_distributed_import(&mut self, import: ImportId) -> Result<EvalValue, DocumentError> {
        let dependency = DocumentDependency::DistributedImport(import);
        if let Some(value) = self.projection_cache.get(&dependency).cloned() {
            self.record_dependency(dependency);
            return Ok(self.value(value));
        }
        let value = self
            .session
            .distributed_import_value_current(import)
            .map_err(|error| DocumentError::Evaluation(error.to_string()))?;
        self.record_dependency(dependency);
        self.projection_cache.insert(dependency, value.clone());
        Ok(self.value(value))
    }

    fn record_dependency(&mut self, dependency: DocumentDependency) {
        self.dependencies.insert(dependency);
        if let Some(capture) = self.dependency_capture.as_mut() {
            capture.insert(dependency);
        } else {
            self.structural_dependencies.insert(dependency);
        }
    }

    fn eval_retained_scalar(
        &mut self,
        expression: DocumentExprId,
        env: &mut EvalEnv,
    ) -> Result<RetainedScalarEvaluation, DocumentError> {
        if self.dependency_capture.is_some() {
            return Err(DocumentError::InvalidPlan(
                "nested retained scalar dependency capture".to_owned(),
            ));
        }
        self.dependency_capture = Some(BTreeSet::new());
        let result = self.eval(expression, env);
        let dependencies = self.dependency_capture.take().unwrap_or_default();
        let value = result?;
        let guards = self.scalar_guards(expression, env, &dependencies)?;
        Ok((value, dependencies, guards))
    }

    fn scalar_guards(
        &mut self,
        expression: DocumentExprId,
        env: &EvalEnv,
        dependencies: &BTreeSet<DocumentDependency>,
    ) -> Result<BTreeMap<ValueTarget, BTreeSet<Value>>, DocumentError> {
        let mut guards = BTreeMap::new();
        for dependency in dependencies {
            let DocumentDependency::Value(target) = dependency else {
                continue;
            };
            if let ScalarTargetUse::Guarded(values) =
                self.scalar_target_use(expression, env, *target, 0)?
                && !values.is_empty()
            {
                guards.insert(*target, values);
            }
        }
        Ok(guards)
    }

    fn scalar_target_use(
        &mut self,
        expression: DocumentExprId,
        env: &EvalEnv,
        target: ValueTarget,
        depth: usize,
    ) -> Result<ScalarTargetUse, DocumentError> {
        if depth > 512 {
            return Ok(ScalarTargetUse::Unguarded);
        }
        let op = self
            .runtime
            .expression_ops
            .get(expression.0)
            .ok_or_else(|| {
                DocumentError::InvalidPlan(format!("expression {} is missing", expression.0))
            })?
            .clone();
        let next = depth + 1;
        match op.as_ref() {
            DocumentExprOp::Constant { .. } | DocumentExprOp::NoElement => {
                Ok(ScalarTargetUse::Independent)
            }
            DocumentExprOp::Read { read } => {
                if self.direct_read_target(read, env) == Some(target) {
                    Ok(ScalarTargetUse::Unguarded)
                } else {
                    Ok(ScalarTargetUse::Independent)
                }
            }
            DocumentExprOp::Project { input, .. } => {
                self.scalar_target_use(*input, env, target, next)
            }
            DocumentExprOp::Record { fields } | DocumentExprOp::TaggedRecord { fields, .. } => self
                .merge_scalar_target_uses(
                    &fields.iter().map(|field| field.value).collect::<Vec<_>>(),
                    env,
                    target,
                    next,
                ),
            DocumentExprOp::List { items } => self.merge_scalar_target_uses(
                &items.iter().map(|item| item.value).collect::<Vec<_>>(),
                env,
                target,
                next,
            ),
            DocumentExprOp::TextTemplate { segments } => self.merge_scalar_target_uses(
                &segments
                    .iter()
                    .filter_map(|segment| match segment {
                        boon_plan::DocumentTextSegment::Static { .. } => None,
                        boon_plan::DocumentTextSegment::Dynamic { value } => Some(*value),
                    })
                    .collect::<Vec<_>>(),
                env,
                target,
                next,
            ),
            DocumentExprOp::LocalBlock { bindings, result } => {
                let mut expressions = bindings
                    .iter()
                    .map(|binding| binding.value)
                    .collect::<Vec<_>>();
                expressions.push(*result);
                self.merge_scalar_target_uses(&expressions, env, target, next)
            }
            DocumentExprOp::Builtin {
                input, arguments, ..
            } => {
                let mut expressions = input.iter().copied().collect::<Vec<_>>();
                expressions.extend(arguments.iter().map(|argument| argument.value));
                self.merge_scalar_target_uses(&expressions, env, target, next)
            }
            DocumentExprOp::Scalar {
                operation,
                left,
                right,
            } => {
                if matches!(
                    operation,
                    DocumentScalarOp::Equal | DocumentScalarOp::NotEqual
                ) && let Some(right) = right
                {
                    let guarded_side = if self.direct_target_expression(*left, env, target)
                        && self.guard_key_expression(*right, env, 0)
                    {
                        Some(*right)
                    } else if self.direct_target_expression(*right, env, target)
                        && self.guard_key_expression(*left, env, 0)
                    {
                        Some(*left)
                    } else {
                        None
                    };
                    if let Some(guarded_side) = guarded_side {
                        let mut guard_env = env.clone();
                        if let Some(value) = guard_value(&self.eval(guarded_side, &mut guard_env)?)
                        {
                            return Ok(ScalarTargetUse::Guarded(BTreeSet::from([value])));
                        }
                    }
                }
                let mut expressions = vec![*left];
                expressions.extend(*right);
                self.merge_scalar_target_uses(&expressions, env, target, next)
            }
            DocumentExprOp::Select { input, arms } => {
                let mut expressions = vec![*input];
                expressions.extend(arms.iter().map(|arm| arm.output));
                self.merge_scalar_target_uses(&expressions, env, target, next)
            }
            DocumentExprOp::Latest { branches } => {
                self.merge_scalar_target_uses(branches, env, target, next)
            }
            DocumentExprOp::Then { input, output } => {
                let mut expressions = vec![*input];
                expressions.extend(*output);
                self.merge_scalar_target_uses(&expressions, env, target, next)
            }
            DocumentExprOp::Constructor { .. }
            | DocumentExprOp::Materialize { .. }
            | DocumentExprOp::RuntimeExpression { .. } => Ok(ScalarTargetUse::Unguarded),
        }
    }

    fn merge_scalar_target_uses(
        &mut self,
        expressions: &[DocumentExprId],
        env: &EvalEnv,
        target: ValueTarget,
        depth: usize,
    ) -> Result<ScalarTargetUse, DocumentError> {
        let mut usage = ScalarTargetUse::Independent;
        for child in expressions {
            usage = usage.merge(self.scalar_target_use(*child, env, target, depth)?);
            if matches!(usage, ScalarTargetUse::Unguarded) {
                break;
            }
        }
        Ok(usage)
    }

    fn direct_target_expression(
        &self,
        expression: DocumentExprId,
        env: &EvalEnv,
        target: ValueTarget,
    ) -> bool {
        self.runtime
            .expression_ops
            .get(expression.0)
            .and_then(|op| match op.as_ref() {
                DocumentExprOp::Read { read } => self.direct_read_target(read, env),
                _ => None,
            })
            == Some(target)
    }

    fn direct_read_target(&self, read: &DocumentRead, env: &EvalEnv) -> Option<ValueTarget> {
        match read {
            DocumentRead::State { state } => Some(ValueTarget::State(*state)),
            DocumentRead::Field { field } => {
                if let Some(row) = env.active_row
                    && self.runtime.field_owners.get(field) == Some(&row.list)
                {
                    return Some(ValueTarget::RowField { row, field: *field });
                }
                Some(
                    self.runtime
                        .field_state_aliases
                        .get(field)
                        .copied()
                        .map(ValueTarget::State)
                        .unwrap_or(ValueTarget::Field(*field)),
                )
            }
            DocumentRead::Row {
                scope,
                field: Some(field),
                ..
            } => env
                .rows
                .get(scope)
                .copied()
                .map(|row| ValueTarget::RowField { row, field: *field }),
            DocumentRead::List { .. }
            | DocumentRead::DistributedImport { .. }
            | DocumentRead::Source { .. }
            | DocumentRead::Parameter { .. }
            | DocumentRead::Local { .. }
            | DocumentRead::Matched { .. }
            | DocumentRead::Row { field: None, .. }
            | DocumentRead::ElementState { .. } => None,
        }
    }

    fn guard_key_expression(
        &self,
        expression: DocumentExprId,
        env: &EvalEnv,
        depth: usize,
    ) -> bool {
        if depth > 512 {
            return false;
        }
        let Some(op) = self.runtime.expression_ops.get(expression.0) else {
            return false;
        };
        let next = depth + 1;
        let all = |expressions: &[DocumentExprId]| {
            expressions
                .iter()
                .all(|child| self.guard_key_expression(*child, env, next))
        };
        match op.as_ref() {
            DocumentExprOp::Constant { .. } | DocumentExprOp::NoElement => true,
            DocumentExprOp::Read { read } => match read {
                DocumentRead::Parameter { .. }
                | DocumentRead::Local { .. }
                | DocumentRead::Matched { .. }
                | DocumentRead::ElementState { .. } => true,
                DocumentRead::State { .. }
                | DocumentRead::Field { .. }
                | DocumentRead::DistributedImport { .. }
                | DocumentRead::List { .. }
                | DocumentRead::Source { .. }
                | DocumentRead::Row { .. } => false,
            },
            DocumentExprOp::Project { input, .. } => self.guard_key_expression(*input, env, next),
            DocumentExprOp::Record { fields } | DocumentExprOp::TaggedRecord { fields, .. } => {
                all(&fields.iter().map(|field| field.value).collect::<Vec<_>>())
            }
            DocumentExprOp::List { items } => {
                all(&items.iter().map(|item| item.value).collect::<Vec<_>>())
            }
            DocumentExprOp::TextTemplate { segments } => all(&segments
                .iter()
                .filter_map(|segment| match segment {
                    boon_plan::DocumentTextSegment::Static { .. } => None,
                    boon_plan::DocumentTextSegment::Dynamic { value } => Some(*value),
                })
                .collect::<Vec<_>>()),
            DocumentExprOp::LocalBlock { bindings, result } => {
                bindings
                    .iter()
                    .all(|binding| self.guard_key_expression(binding.value, env, next))
                    && self.guard_key_expression(*result, env, next)
            }
            DocumentExprOp::Builtin {
                input, arguments, ..
            } => {
                input
                    .iter()
                    .all(|input| self.guard_key_expression(*input, env, next))
                    && arguments
                        .iter()
                        .all(|argument| self.guard_key_expression(argument.value, env, next))
            }
            DocumentExprOp::Scalar { left, right, .. } => {
                self.guard_key_expression(*left, env, next)
                    && right
                        .iter()
                        .all(|right| self.guard_key_expression(*right, env, next))
            }
            DocumentExprOp::Select { input, arms } => {
                self.guard_key_expression(*input, env, next)
                    && arms
                        .iter()
                        .all(|arm| self.guard_key_expression(arm.output, env, next))
            }
            DocumentExprOp::Latest { branches } => all(branches),
            DocumentExprOp::Then { input, output } => {
                self.guard_key_expression(*input, env, next)
                    && output
                        .iter()
                        .all(|output| self.guard_key_expression(*output, env, next))
            }
            DocumentExprOp::Constructor { .. }
            | DocumentExprOp::Materialize { .. }
            | DocumentExprOp::RuntimeExpression { .. } => false,
        }
    }

    fn value(&self, value: Value) -> EvalValue {
        machine_value_to_eval(value)
    }

    fn project(&mut self, mut value: EvalValue, path: &[DocumentNameId]) -> EvalValue {
        if let Some(row) = match &value {
            EvalValue::Row { id: Some(row), .. } | EvalValue::MappedRow { id: row, .. } => {
                Some(*row)
            }
            _ => None,
        } {
            let names = path
                .iter()
                .filter_map(|name| self.name(*name).ok())
                .filter(|name| *name != "events")
                .collect::<Vec<_>>();
            let suffix = names.join(".");
            if !suffix.is_empty()
                && let Some(source) = self.runtime.resolve_row_source(row.list, &suffix)
            {
                return source;
            }
        }
        for name in path {
            let Ok(name) = self.name(*name).map(str::to_owned) else {
                return EvalValue::Null;
            };
            value = match value {
                EvalValue::Record(mut fields) | EvalValue::Tagged(_, mut fields) => {
                    fields.remove(&name).unwrap_or(EvalValue::Null)
                }
                EvalValue::MappedRow { mut fields, .. } => {
                    fields.remove(&name).unwrap_or(EvalValue::Null)
                }
                EvalValue::Row { id, fields } => {
                    let field = fields.keys().find(|field| {
                        self.runtime
                            .field_names
                            .get(field)
                            .is_some_and(|names| names.iter().any(|candidate| candidate == &name))
                    });
                    let field = field.copied().or_else(|| {
                        self.runtime.field_names.iter().find_map(|(field, names)| {
                            (id.is_none()
                                || id.is_some_and(|row| {
                                    self.runtime.field_owners.get(field) == Some(&row.list)
                                }))
                            .then(|| {
                                names
                                    .iter()
                                    .any(|candidate| candidate == &name)
                                    .then_some(*field)
                            })
                            .flatten()
                        })
                    });
                    id.zip(field)
                        .and_then(|(row, field)| {
                            self.read_target(ValueTarget::RowField { row, field }).ok()
                        })
                        .or_else(|| {
                            field
                                .as_ref()
                                .and_then(|field| fields.get(field))
                                .cloned()
                                .map(|value| self.value(value))
                        })
                        .or_else(|| {
                            (name == "key")
                                .then(|| id.map(|id| EvalValue::Number(id.key as f64)))
                                .flatten()
                        })
                        .unwrap_or(EvalValue::Null)
                }
                _ => EvalValue::Null,
            };
        }
        value
    }

    fn project_field(&mut self, value: EvalValue, field: FieldId) -> EvalValue {
        match value {
            EvalValue::MappedRow { mut fields, .. } => self
                .runtime
                .field_names
                .get(&field)
                .and_then(|names| names.last())
                .and_then(|name| fields.remove(name))
                .unwrap_or(EvalValue::Null),
            EvalValue::Row { id, fields } => id
                .and_then(|row| self.read_target(ValueTarget::RowField { row, field }).ok())
                .or_else(|| fields.get(&field).cloned().map(|value| self.value(value)))
                .unwrap_or(EvalValue::Null),
            value => self
                .runtime
                .field_names
                .get(&field)
                .and_then(|names| names.last())
                .and_then(|name| match value {
                    EvalValue::Record(mut fields) | EvalValue::Tagged(_, mut fields) => {
                        fields.remove(name)
                    }
                    _ => None,
                })
                .unwrap_or(EvalValue::Null),
        }
    }

    fn call_function(
        &mut self,
        function: DocumentFunctionId,
        parameters: BTreeMap<boon_plan::DocumentParameterId, EvalValue>,
        caller: &EvalEnv,
        instance: String,
    ) -> Result<EvalValue, DocumentError> {
        let function = self
            .runtime
            .plan()
            .functions
            .iter()
            .find(|candidate| candidate.id == function)
            .cloned()
            .ok_or_else(|| {
                DocumentError::InvalidPlan(format!("function {} is missing", function.0))
            })?;
        let mut env = caller.clone();
        env.parameters = Arc::new(parameters);
        env.locals = Arc::new(BTreeMap::new());
        Arc::make_mut(&mut env.instance).push(instance);
        self.eval(function.body, &mut env)
    }

    fn pattern_matches(
        &self,
        input: &EvalValue,
        pattern: DocumentPattern,
    ) -> Result<bool, DocumentError> {
        Ok(match pattern {
            DocumentPattern::Constant { constant } => {
                eval_values_equal(input, &self.constant(constant)?)
            }
            DocumentPattern::Tag { tag } => {
                let tag = self.name(tag)?;
                matches!(input, EvalValue::Enum(value) if value == tag)
                    || matches!(input, EvalValue::Tagged(value, _) if value == tag)
                    || matches!(input, EvalValue::Text(value) if value == tag)
            }
            DocumentPattern::Wildcard => true,
        })
    }

    fn install_pattern_rows(
        &mut self,
        input: &EvalValue,
        bindings: &[boon_plan::DocumentSelectBinding],
        env: &mut EvalEnv,
    ) -> Result<(), DocumentError> {
        let mut rows = BTreeMap::new();
        for binding in bindings {
            let value = self.project(input.clone(), &binding.projection);
            let row = match value {
                EvalValue::Row { id: Some(row), .. } | EvalValue::MappedRow { id: row, .. } => row,
                _ => continue,
            };
            let Some(scope) = self.runtime.list_scopes.get(&row.list).copied() else {
                return Err(DocumentError::InvalidPlan(format!(
                    "pattern-bound row from list {} has no canonical document scope",
                    row.list.0
                )));
            };
            if let Some(previous) = rows.insert(scope, row)
                && previous != row
            {
                return Err(DocumentError::Evaluation(format!(
                    "pattern bindings resolve multiple rows for document scope {}",
                    scope.0
                )));
            }
        }
        Arc::make_mut(&mut env.rows).extend(rows);
        Ok(())
    }

    fn materialize(
        &mut self,
        id: DocumentMaterializationId,
        env: &mut EvalEnv,
    ) -> Result<EvalValue, DocumentError> {
        let materialization = self
            .runtime
            .plan()
            .materializations
            .iter()
            .find(|candidate| candidate.id == id)
            .cloned()
            .ok_or_else(|| {
                DocumentError::InvalidPlan(format!("materialization {} is missing", id.0))
            })?;
        enum MaterializationSourceRows {
            DirectList(ListId),
            Values(Vec<EvalValue>),
        }
        let (source, logical_item_count) = match materialization.source.clone() {
            DocumentMaterializationSource::List { list } => {
                self.structural_lists.insert(list);
                let logical_item_count = self
                    .session
                    .list_logical_len_current(list)
                    .map_err(|error| DocumentError::Evaluation(error.to_string()))?;
                (
                    MaterializationSourceRows::DirectList(list),
                    logical_item_count,
                )
            }
            _ => {
                let source = self.materialization_source(&materialization, env)?;
                match source {
                    EvalValue::RuntimeList { list, logical_len } => {
                        self.structural_lists.insert(list);
                        (MaterializationSourceRows::DirectList(list), logical_len)
                    }
                    EvalValue::List(items) => {
                        let logical_item_count = u64::try_from(items.len()).map_err(|_| {
                            DocumentError::Evaluation(
                                "document materialization length does not fit the logical key space"
                                    .to_owned(),
                            )
                        })?;
                        (MaterializationSourceRows::Values(items), logical_item_count)
                    }
                    EvalValue::Null => (MaterializationSourceRows::Values(Vec::new()), 0),
                    value => (MaterializationSourceRows::Values(vec![value]), 1),
                }
            }
        };
        let demand =
            self.runtime
                .windows
                .get(&id)
                .cloned()
                .unwrap_or_else(|| DocumentWindowDemand {
                    materialization: id,
                    visible: 0..DEFAULT_VISIBLE_ITEMS,
                    overscan: 0..DEFAULT_VISIBLE_ITEMS.saturating_add(DEFAULT_OVERSCAN_ITEMS),
                });
        let demand = if logical_item_count != 0 && self.runtime.empty_source_windows.contains(&id) {
            self.runtime
                .last_nonempty_windows
                .get(&id)
                .cloned()
                .unwrap_or(demand)
        } else {
            demand
        };
        let logical_item_count_usize = usize::try_from(logical_item_count).map_err(|_| {
            DocumentError::Evaluation(
                "document materialization length does not fit this target".to_owned(),
            )
        })?;
        let range = clamp_range(demand.overscan.clone(), logical_item_count_usize);
        let items: Vec<(u64, EvalValue)> = match source {
            MaterializationSourceRows::DirectList(list) => {
                let (current_logical_item_count, rows) = self
                    .session
                    .list_row_snapshots_window_current(list, range.clone())
                    .map_err(|error| DocumentError::Evaluation(error.to_string()))?;
                if current_logical_item_count != logical_item_count {
                    return Err(DocumentError::Evaluation(
                        "document materialization source changed during one currentness read"
                            .to_owned(),
                    ));
                }
                rows.into_iter()
                    .enumerate()
                    .map(|(offset, row)| {
                        (
                            range.start.saturating_add(offset as u64),
                            EvalValue::Row {
                                id: Some(row.id),
                                fields: row.fields,
                            },
                        )
                    })
                    .collect()
            }
            MaterializationSourceRows::Values(items) => items
                .into_iter()
                .enumerate()
                .skip(range.start as usize)
                .take(range.end.saturating_sub(range.start) as usize)
                .map(|(index, item)| (index as u64, item))
                .collect(),
        };
        if let Some(parent) = env.parent.as_ref()
            && let Some(node) = self.frame.nodes.get_mut(parent)
        {
            let axis = if node.kind == DocumentNodeKind::Row {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            node.materialized.push(MaterializedRange {
                materialization: Some(id.0),
                axis,
                visible: clamp_range(demand.visible.clone(), logical_item_count_usize),
                overscan: range,
                logical_item_count,
            });
        }
        let static_arguments = materialization
            .template_arguments
            .iter()
            .map(|argument| {
                self.eval(argument.value, env)
                    .map(|value| (argument.parameter, value))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let scoped_generation = match materialization.row_identity {
            DocumentRowIdentity::ListHiddenKeyAndGeneration { .. } => None,
            DocumentRowIdentity::ScopedHiddenKeyAndGeneration { .. } => {
                let owner = ScopedMaterializationOwner {
                    materialization: id,
                    parent_instance: env.instance.as_ref().clone(),
                };
                self.active_scoped_materialization_owners
                    .insert(owner.clone());
                Some(
                    self.scoped_materialization_identities
                        .reconcile(owner, logical_item_count_usize)?,
                )
            }
        };
        let mut nodes = Vec::new();
        for (logical_index, item) in items {
            let row = self.materialization_row(&item);
            let identity = match materialization.row_identity {
                DocumentRowIdentity::ListHiddenKeyAndGeneration { list } => {
                    let row = row.ok_or_else(|| {
                        DocumentError::Evaluation(format!(
                            "document materialization {} item has no hidden row key and generation",
                            id.0
                        ))
                    })?;
                    if row.list != list {
                        return Err(DocumentError::InvalidPlan(format!(
                            "document materialization {} expected ListId {}, received ListId {}",
                            id.0, list.0, row.list.0
                        )));
                    }
                    DocumentItemIdentity::Stored(row)
                }
                DocumentRowIdentity::ScopedHiddenKeyAndGeneration { scope } => match row {
                    Some(row) => DocumentItemIdentity::Stored(row),
                    None => {
                        let generation = scoped_generation.ok_or_else(|| {
                            DocumentError::InvalidPlan(format!(
                                "document materialization {} has no scoped generation for item {}",
                                id.0, logical_index
                            ))
                        })?;
                        DocumentItemIdentity::Scoped {
                            scope,
                            key: logical_index.saturating_add(1),
                            generation,
                        }
                    }
                },
            };
            self.materialized_items.insert(identity.clone());
            let mut parameters = static_arguments.clone();
            parameters.insert(materialization.item_parameter, item);
            let mut row_env = env.clone();
            if let Some(row) = row {
                let rows = Arc::make_mut(&mut row_env.rows);
                let structural_rows = self
                    .session
                    .structural_owner_rows(row)
                    .map_err(|error| DocumentError::Evaluation(error.to_string()))?;
                for structural_row in structural_rows {
                    if let Some(scope) = self.runtime.list_scopes.get(&structural_row.list).copied()
                    {
                        rows.insert(scope, structural_row);
                    }
                }
                rows.insert(materialization.item_scope, row);
                row_env.active_row = Some(row);
            }
            let value = self.call_function(
                materialization.template_function,
                parameters,
                &row_env,
                format!("materialize-{}-{}", id.0, identity.instance_fragment()),
            )?;
            let created = value.node_ids();
            nodes.extend(created);
        }
        Ok(EvalValue::Nodes(nodes))
    }

    fn materialization_source(
        &mut self,
        materialization: &DocumentMaterialization,
        env: &mut EvalEnv,
    ) -> Result<EvalValue, DocumentError> {
        match materialization.source.clone() {
            DocumentMaterializationSource::List { .. } => Err(DocumentError::InvalidPlan(
                "direct list materialization bypassed its bounded window path".to_owned(),
            )),
            DocumentMaterializationSource::Field { field } => {
                if let Some(row) = env.active_row
                    && self.runtime.field_owners.get(&field) == Some(&row.list)
                {
                    return self.read_target(ValueTarget::RowField { row, field });
                }
                if let Some(state) = self.runtime.field_state_aliases.get(&field).copied() {
                    return self.read_target(ValueTarget::State(state));
                }
                self.read_target(ValueTarget::Field(field))
            }
            DocumentMaterializationSource::ScopedField { scope, field } => {
                let row = env.rows.get(&scope).copied().ok_or_else(|| {
                    DocumentError::Evaluation(format!(
                        "materialization {} requires inactive row scope {}",
                        materialization.id.0, scope.0
                    ))
                })?;
                self.read_target(ValueTarget::RowField { row, field })
            }
            DocumentMaterializationSource::ParameterField { parameter, field } => {
                let value = env
                    .parameters
                    .get(&parameter)
                    .cloned()
                    .unwrap_or(EvalValue::Null);
                if let EvalValue::Row { id: Some(row), .. } = value {
                    self.read_target(ValueTarget::RowField { row, field })
                } else {
                    Ok(self.project_field(value, field))
                }
            }
            DocumentMaterializationSource::Parameter {
                parameter,
                projection,
            } => Ok(self.project(
                env.parameters
                    .get(&parameter)
                    .cloned()
                    .unwrap_or(EvalValue::Null),
                &projection,
            )),
            DocumentMaterializationSource::Expression { expression } => self.eval(expression, env),
        }
    }

    fn materialization_row(&self, item: &EvalValue) -> Option<RowId> {
        match item {
            EvalValue::Row { id: Some(row), .. } | EvalValue::MappedRow { id: row, .. } => {
                Some(*row)
            }
            _ => None,
        }
    }

    fn constructor(
        &mut self,
        template: DocumentTemplateId,
        constructor: DocumentConstructor,
        element_context: Option<DocumentElementContextId>,
        arguments: Vec<boon_plan::DocumentConstructorArgument>,
        env: &mut EvalEnv,
    ) -> Result<EvalValue, DocumentError> {
        if matches!(
            constructor,
            DocumentConstructor::DocumentNew | DocumentConstructor::SceneNew
        ) {
            let mut nodes = Vec::new();
            for argument in arguments {
                let value = self.eval(argument.value, env)?;
                nodes.extend(value.node_ids());
            }
            return Ok(EvalValue::Nodes(nodes));
        }

        let template_node = self
            .runtime
            .plan()
            .templates
            .iter()
            .find(|candidate| candidate.id == template)
            .map(|template| template.node.0)
            .ok_or_else(|| {
                DocumentError::InvalidPlan(format!("template {} is missing", template.0))
            })?;
        let mut element_env = env.clone();
        element_env.element_context = element_context;
        let mut evaluated = Vec::new();
        let mut delayed = Vec::new();
        for argument in arguments {
            if matches!(
                argument.role,
                DocumentArgumentRole::Child | DocumentArgumentRole::Children
            ) {
                delayed.push(argument);
            } else {
                let class = self.runtime.plan().expressions[argument.value.0].value_class;
                let environment = element_env.clone();
                let retain_scalar = class == boon_plan::DocumentValueClass::DynamicScalar
                    && retained_argument_role(argument.role)
                    && self.name(argument.name)? != "direction";
                let (value, binding) = if retain_scalar {
                    let (value, dependencies, guards) =
                        self.eval_retained_scalar(argument.value, &mut element_env)?;
                    if retained_value_affects_structure(argument.role, &value) {
                        self.structural_dependencies
                            .extend(dependencies.iter().copied());
                        (value, None)
                    } else {
                        (
                            value,
                            Some(RetainedScalarBinding {
                                expression: argument.value,
                                environment,
                                dependencies,
                                guards,
                            }),
                        )
                    }
                } else {
                    (self.eval(argument.value, &mut element_env)?, None)
                };
                evaluated.push(EvaluatedArgument {
                    name: argument.name,
                    role: argument.role,
                    value,
                    binding,
                });
            }
        }
        let direction = evaluated.iter().find_map(|argument| {
            (self.name(argument.name).ok() == Some("direction")).then(|| argument.value.text())
        });
        let kind = constructor_kind(constructor, direction.as_deref());
        let row = env.active_row;
        let id = self.instance_node_id(template_node, env, row);
        let mut node = DocumentNode::new(id.0.clone(), kind);
        node.parent = env.parent.clone();
        if node.kind == DocumentNodeKind::MapViewport {
            node.map_viewport = Some(Box::new(evaluate_map_viewport_descriptor(
                evaluated
                    .iter()
                    .map(|argument| (argument.role, argument.value.clone())),
            )?));
        }
        let mut retained_arguments = Vec::new();
        for argument in evaluated {
            self.apply_argument(
                &mut node,
                argument.name,
                argument.role,
                argument.value.clone(),
                &element_env,
            )?;
            retained_arguments.push(RetainedArgument {
                name: argument.name,
                role: argument.role,
                value: argument.value,
                binding: argument.binding,
            });
        }
        if node.kind == DocumentNodeKind::Button {
            node.style
                .entry("cursor".to_owned())
                .or_insert_with(|| StyleValue::Text("pointer".to_owned()));
        }
        if node.style.contains_key("to") {
            if let Some(url) = node.style.get("to").cloned() {
                node.style.insert("href".to_owned(), url);
            }
            node.style
                .insert("cursor".to_owned(), StyleValue::Text("pointer".to_owned()));
            node.style.insert("link".to_owned(), StyleValue::Bool(true));
        }
        self.add_node(node);
        let mut child_env = element_env.clone();
        child_env.parent = Some(id.clone());
        Arc::make_mut(&mut child_env.instance).push(format!("node-{template_node}"));
        for argument in delayed {
            let class = self.runtime.plan().expressions[argument.value.0].value_class;
            let environment = child_env.clone();
            let retain_scalar = class == boon_plan::DocumentValueClass::DynamicScalar
                && retained_argument_role(argument.role);
            let (value, binding) = if retain_scalar {
                let (value, dependencies, guards) =
                    self.eval_retained_scalar(argument.value, &mut child_env)?;
                if retained_value_affects_structure(argument.role, &value) {
                    self.structural_dependencies
                        .extend(dependencies.iter().copied());
                    (value, None)
                } else {
                    (
                        value,
                        Some(RetainedScalarBinding {
                            expression: argument.value,
                            environment,
                            dependencies,
                            guards,
                        }),
                    )
                }
            } else {
                (self.eval(argument.value, &mut child_env)?, None)
            };
            match argument.role {
                DocumentArgumentRole::StaticText | DocumentArgumentRole::DynamicText => {
                    let name = self.name(argument.name).unwrap_or("text").to_owned();
                    if let Some(node) = self.frame.nodes.get_mut(&id) {
                        apply_text_argument(node, &name, value.clone());
                    }
                    retained_arguments.push(RetainedArgument {
                        name: argument.name,
                        role: argument.role,
                        value,
                        binding,
                    });
                }
                DocumentArgumentRole::EventBindings => {
                    if let Some(node) = self.frame.nodes.get_mut(&id) {
                        attach_sources(self.runtime, self.session, node, &value, &child_env)?;
                    }
                    retained_arguments.push(RetainedArgument {
                        name: argument.name,
                        role: argument.role,
                        value,
                        binding,
                    });
                }
                _ => self.attach_nodes(&id, value),
            }
        }
        self.configure_scroll_ranges(&id);
        self.retained_nodes.insert(
            id.clone(),
            RetainedNode {
                kind: self
                    .frame
                    .nodes
                    .get(&id)
                    .map(|node| node.kind.clone())
                    .unwrap_or(DocumentNodeKind::Stack),
                parent: env.parent.clone(),
                environment: element_env,
                arguments: retained_arguments,
            },
        );
        Ok(EvalValue::Nodes(vec![id]))
    }

    fn instance_node_id(
        &self,
        template_node: u64,
        env: &EvalEnv,
        row: Option<RowId>,
    ) -> FrameNodeId {
        let mut identity = env.instance.join("/");
        if let Some(row) = row {
            identity.push_str(&format!(
                "/row-{}-{}-{}",
                row.list.0, row.key, row.generation
            ));
        }
        frame_node_id(template_node, Some(&identity))
    }

    fn apply_argument(
        &self,
        node: &mut DocumentNode,
        name: DocumentNameId,
        role: DocumentArgumentRole,
        value: EvalValue,
        env: &EvalEnv,
    ) -> Result<(), DocumentError> {
        apply_argument(self.runtime, self.session, node, name, role, value, env)
    }

    fn add_node(&mut self, mut node: DocumentNode) {
        if let Some(parent) = node.parent.clone()
            && let Some(parent_node) = self.frame.nodes.get_mut(&parent)
            && !parent_node.children.contains(&node.id)
        {
            parent_node.children.push(node.id.clone());
        }
        node.children.clear();
        self.frame.nodes.insert(node.id.clone(), node);
    }

    fn attach_nodes(&mut self, parent: &FrameNodeId, value: EvalValue) {
        let mut inline_ordinal = 0usize;
        self.attach_content(parent, value, &mut inline_ordinal);
    }

    fn attach_content(
        &mut self,
        parent: &FrameNodeId,
        value: EvalValue,
        inline_ordinal: &mut usize,
    ) {
        match value {
            EvalValue::Nodes(children) => {
                for child in children {
                    self.attach_existing_node(parent, child);
                }
            }
            EvalValue::List(values) => {
                for value in values {
                    self.attach_content(parent, value, inline_ordinal);
                }
            }
            value => {
                let Some(text) = inline_content_text(&value).filter(|text| !text.is_empty()) else {
                    return;
                };
                let child = FrameNodeId(format!("{}:inline-{inline_ordinal}", parent.0));
                *inline_ordinal = inline_ordinal.saturating_add(1);
                let mut node = DocumentNode::new(child.0.clone(), DocumentNodeKind::Text);
                node.parent = Some(parent.clone());
                node.text = Some(TextValue { text });
                if let Some(parent_node) = self.frame.nodes.get(parent) {
                    inherit_inline_text_style(&parent_node.style, &mut node.style);
                }
                self.add_node(node);
            }
        }
    }

    fn attach_existing_node(&mut self, parent: &FrameNodeId, child: FrameNodeId) {
        if let Some(previous_parent) = self
            .frame
            .nodes
            .get(&child)
            .and_then(|node| node.parent.clone())
            && let Some(previous) = self.frame.nodes.get_mut(&previous_parent)
        {
            previous.children.retain(|candidate| candidate != &child);
        }
        if let Some(node) = self.frame.nodes.get_mut(&child) {
            node.parent = Some(parent.clone());
        }
        if let Some(parent_node) = self.frame.nodes.get_mut(parent)
            && !parent_node.children.contains(&child)
        {
            parent_node.children.push(child);
        }
    }

    fn configure_scroll_ranges(&mut self, id: &FrameNodeId) {
        let Some(node) = self.frame.nodes.get_mut(id) else {
            return;
        };
        let scroll = style_bool(&node.style, "scroll")
            || style_bool(&node.style, "scroll_y")
            || style_bool(&node.style, "scrollbars");
        let scroll_x = style_bool(&node.style, "scroll")
            || style_bool(&node.style, "scroll_x")
            || style_bool(&node.style, "scrollbars");
        if scroll
            && !node
                .materialized
                .iter()
                .any(|range| range.axis == Axis::Vertical)
        {
            node.materialized.push(MaterializedRange {
                materialization: None,
                axis: Axis::Vertical,
                visible: 0..DEFAULT_VISIBLE_ITEMS,
                overscan: 0..DEFAULT_VISIBLE_ITEMS.saturating_add(DEFAULT_OVERSCAN_ITEMS),
                logical_item_count: DEFAULT_VISIBLE_ITEMS.saturating_add(DEFAULT_OVERSCAN_ITEMS),
            });
        }
        if scroll_x
            && !node
                .materialized
                .iter()
                .any(|range| range.axis == Axis::Horizontal)
        {
            node.materialized.push(MaterializedRange {
                materialization: None,
                axis: Axis::Horizontal,
                visible: 0..8,
                overscan: 0..12,
                logical_item_count: 12,
            });
        }
    }
}

fn apply_argument(
    runtime: &DocumentRuntime,
    session: &MachineInstance,
    node: &mut DocumentNode,
    name: DocumentNameId,
    role: DocumentArgumentRole,
    value: EvalValue,
    env: &EvalEnv,
) -> Result<(), DocumentError> {
    let name = runtime
        .plan()
        .names
        .get(name.0)
        .map(String::as_str)
        .ok_or_else(|| DocumentError::InvalidPlan(format!("name {} is missing", name.0)))?;
    match role {
        DocumentArgumentRole::StaticStyle | DocumentArgumentRole::DynamicStyle => {
            if let EvalValue::Record(style) | EvalValue::Tagged(_, style) = value {
                lower_style_record(&style, &mut node.style);
            }
        }
        DocumentArgumentRole::StaticText | DocumentArgumentRole::DynamicText => {
            apply_text_argument(node, name, value)
        }
        DocumentArgumentRole::EventBindings => attach_sources(runtime, session, node, &value, env)?,
        DocumentArgumentRole::Value => apply_value_argument(node, name, value),
        DocumentArgumentRole::Child
        | DocumentArgumentRole::Children
        | DocumentArgumentRole::MapCamera
        | DocumentArgumentRole::MapBounds
        | DocumentArgumentRole::MapTileSource
        | DocumentArgumentRole::MapOverlays
        | DocumentArgumentRole::MapInteraction
        | DocumentArgumentRole::MapGeneration => {}
    }
    Ok(())
}

fn retained_argument_role(role: DocumentArgumentRole) -> bool {
    matches!(
        role,
        DocumentArgumentRole::StaticStyle
            | DocumentArgumentRole::DynamicStyle
            | DocumentArgumentRole::StaticText
            | DocumentArgumentRole::DynamicText
            | DocumentArgumentRole::EventBindings
            | DocumentArgumentRole::Value
            | DocumentArgumentRole::MapCamera
            | DocumentArgumentRole::MapBounds
            | DocumentArgumentRole::MapTileSource
            | DocumentArgumentRole::MapOverlays
            | DocumentArgumentRole::MapInteraction
            | DocumentArgumentRole::MapGeneration
    )
}

fn retained_value_affects_structure(role: DocumentArgumentRole, value: &EvalValue) -> bool {
    if !matches!(
        role,
        DocumentArgumentRole::StaticStyle | DocumentArgumentRole::DynamicStyle
    ) {
        return false;
    }
    let Some(style) = record_fields(value) else {
        return false;
    };
    ["scroll", "scroll_x", "scroll_y", "scrollbars"]
        .iter()
        .any(|key| style.contains_key(*key))
}

impl EvalValue {
    fn text(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Bool(value) => if *value { "True" } else { "False" }.to_owned(),
            Self::Number(value) => format_number(*value),
            Self::Text(value) | Self::Enum(value) => value.clone(),
            Self::Bytes(value) => String::from_utf8_lossy(value).into_owned(),
            Self::Record(fields) => fields
                .get("text")
                .map(Self::text)
                .unwrap_or_else(|| format_record(None, fields)),
            Self::MappedRow { fields, .. } => fields
                .get("text")
                .map(Self::text)
                .unwrap_or_else(|| format_record(None, fields)),
            Self::Tagged(tag, fields) => format_record(Some(tag), fields),
            Self::List(values) => values.iter().map(Self::text).collect::<Vec<_>>().join(""),
            Self::RuntimeList { .. } => String::new(),
            Self::Row { .. } => String::new(),
            Self::Source(_) => String::new(),
            Self::Nodes(_) => String::new(),
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::Number(value) => *value != 0.0,
            Self::Text(value) | Self::Enum(value) => !value.is_empty(),
            Self::Bytes(value) => !value.is_empty(),
            Self::Record(value) | Self::Tagged(_, value) => !value.is_empty(),
            Self::MappedRow { fields, .. } => !fields.is_empty(),
            Self::List(value) => !value.is_empty(),
            Self::RuntimeList { logical_len, .. } => *logical_len != 0,
            Self::Nodes(value) => !value.is_empty(),
            Self::Row { .. } => true,
            Self::Null | Self::Source(_) => false,
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(value) => value.parse().ok(),
            _ => None,
        }
    }

    fn node_ids(&self) -> Vec<FrameNodeId> {
        match self {
            Self::Nodes(nodes) => nodes.clone(),
            Self::List(values) => values.iter().flat_map(Self::node_ids).collect(),
            _ => Vec::new(),
        }
    }
}

fn inline_content_text(value: &EvalValue) -> Option<String> {
    match value {
        EvalValue::Bool(_)
        | EvalValue::Number(_)
        | EvalValue::Text(_)
        | EvalValue::Bytes(_)
        | EvalValue::Enum(_) => Some(value.text()),
        EvalValue::Null
        | EvalValue::Record(_)
        | EvalValue::MappedRow { .. }
        | EvalValue::Tagged(_, _)
        | EvalValue::List(_)
        | EvalValue::RuntimeList { .. }
        | EvalValue::Row { .. }
        | EvalValue::Source(_)
        | EvalValue::Nodes(_) => None,
    }
}

fn inherit_inline_text_style(parent: &StyleMap, child: &mut StyleMap) {
    for key in [
        "font",
        "font_style",
        "font_features",
        "weight",
        "size",
        "color",
        "line_height",
        "letter_spacing",
        "text_clip_padding",
        "vertical_align",
    ] {
        if let Some(value) = parent.get(key) {
            child.insert(key.to_owned(), value.clone());
        }
    }
    child.insert("width".to_owned(), StyleValue::Text("Auto".to_owned()));
    child.insert("height".to_owned(), StyleValue::Text("Fill".to_owned()));
    child.insert("auto_padding".to_owned(), StyleValue::Number(0.0));
    child.insert("text_inset".to_owned(), StyleValue::Number(0.0));
}

fn guard_value(value: &EvalValue) -> Option<Value> {
    match value {
        EvalValue::Null => Some(Value::Null),
        EvalValue::Bool(value) => Some(Value::Bool(*value)),
        EvalValue::Number(value) => FiniteReal::new(*value).ok().map(Value::Number),
        EvalValue::Text(value) | EvalValue::Enum(value) => Some(Value::Text(value.clone())),
        EvalValue::Bytes(value) => Some(Value::Bytes(value.clone())),
        EvalValue::Record(fields) => fields
            .iter()
            .map(|(name, value)| Some((name.clone(), guard_value(value)?)))
            .collect::<Option<BTreeMap<_, _>>>()
            .map(Value::Record),
        EvalValue::MappedRow { id, fields } => fields
            .iter()
            .map(|(name, value)| Some((name.clone(), guard_value(value)?)))
            .collect::<Option<BTreeMap<_, _>>>()
            .map(|fields| Value::MappedRow { id: *id, fields }),
        EvalValue::List(values) => values
            .iter()
            .map(guard_value)
            .collect::<Option<Vec<_>>>()
            .map(Value::List),
        EvalValue::Row {
            id: Some(id),
            fields,
        } => Some(Value::Row {
            id: *id,
            fields: fields.clone(),
        }),
        EvalValue::Tagged(tag, fields) => {
            let mut record = fields
                .iter()
                .map(|(name, value)| Some((name.clone(), guard_value(value)?)))
                .collect::<Option<BTreeMap<_, _>>>()?;
            if record
                .insert("$tag".to_owned(), Value::Text(tag.clone()))
                .is_some()
            {
                return None;
            }
            Some(Value::Record(record))
        }
        EvalValue::RuntimeList { .. }
        | EvalValue::Row { id: None, .. }
        | EvalValue::Source(_)
        | EvalValue::Nodes(_) => None,
    }
}

fn machine_value_to_eval(value: Value) -> EvalValue {
    match value {
        Value::Null => EvalValue::Null,
        Value::Bool(value) => EvalValue::Bool(value),
        Value::Number(value) => EvalValue::Number(value.get()),
        Value::Text(value) => EvalValue::Text(value),
        Value::Bytes(value) => EvalValue::Bytes(value),
        Value::List(values) => {
            EvalValue::List(values.into_iter().map(machine_value_to_eval).collect())
        }
        Value::Record(values) => {
            let mut fields = values
                .into_iter()
                .map(|(name, value)| (name, machine_value_to_eval(value)))
                .collect::<BTreeMap<_, _>>();
            match fields.remove("$tag") {
                Some(EvalValue::Text(tag)) => EvalValue::Tagged(tag, fields),
                Some(value) => {
                    fields.insert("$tag".to_owned(), value);
                    EvalValue::Record(fields)
                }
                None => EvalValue::Record(fields),
            }
        }
        Value::MappedRow { id, fields } => EvalValue::MappedRow {
            id,
            fields: fields
                .into_iter()
                .map(|(name, value)| (name, machine_value_to_eval(value)))
                .collect(),
        },
        Value::Row { id, fields } => EvalValue::Row {
            id: Some(id),
            fields,
        },
        Value::Error { code } => EvalValue::Text(code),
        Value::HostBound { visible, .. } => machine_value_to_eval(*visible),
    }
}

fn spread_record(record: &mut BTreeMap<String, EvalValue>, value: EvalValue) {
    match value {
        EvalValue::Record(fields)
        | EvalValue::MappedRow { fields, .. }
        | EvalValue::Tagged(_, fields) => record.extend(fields),
        _ => {}
    }
}

fn spread_list(list: &mut Vec<EvalValue>, value: EvalValue) {
    match value {
        EvalValue::List(values) => list.extend(values),
        EvalValue::Nodes(nodes) => {
            list.extend(nodes.into_iter().map(|node| EvalValue::Nodes(vec![node])))
        }
        value => list.push(value),
    }
}

fn eval_scalar(
    operation: DocumentScalarOp,
    left: EvalValue,
    right: Option<EvalValue>,
) -> EvalValue {
    let right = right.unwrap_or(EvalValue::Null);
    match operation {
        DocumentScalarOp::Add => match (left.number(), right.number()) {
            (Some(left), Some(right)) => EvalValue::Number(left + right),
            _ => EvalValue::Text(format!("{}{}", left.text(), right.text())),
        },
        DocumentScalarOp::Subtract => numeric_binary(left, right, |left, right| left - right),
        DocumentScalarOp::Multiply => numeric_binary(left, right, |left, right| left * right),
        DocumentScalarOp::Divide => {
            numeric_binary(
                left,
                right,
                |left, right| {
                    if right == 0.0 { 0.0 } else { left / right }
                },
            )
        }
        DocumentScalarOp::Remainder => {
            numeric_binary(
                left,
                right,
                |left, right| {
                    if right == 0.0 { 0.0 } else { left % right }
                },
            )
        }
        DocumentScalarOp::Equal => EvalValue::Bool(eval_values_equal(&left, &right)),
        DocumentScalarOp::NotEqual => EvalValue::Bool(!eval_values_equal(&left, &right)),
        DocumentScalarOp::Less => compare_binary(left, right, |ordering| ordering.is_lt()),
        DocumentScalarOp::LessOrEqual => compare_binary(left, right, |ordering| ordering.is_le()),
        DocumentScalarOp::Greater => compare_binary(left, right, |ordering| ordering.is_gt()),
        DocumentScalarOp::GreaterOrEqual => {
            compare_binary(left, right, |ordering| ordering.is_ge())
        }
        DocumentScalarOp::And => EvalValue::Bool(left.truthy() && right.truthy()),
        DocumentScalarOp::Or => EvalValue::Bool(left.truthy() || right.truthy()),
        DocumentScalarOp::Negate => EvalValue::Number(-left.number().unwrap_or(0.0)),
        DocumentScalarOp::Not => EvalValue::Bool(!left.truthy()),
    }
}

fn eval_values_equal(left: &EvalValue, right: &EvalValue) -> bool {
    match (left, right) {
        (EvalValue::Text(left), EvalValue::Enum(right))
        | (EvalValue::Enum(left), EvalValue::Text(right)) => left == right,
        _ => left == right,
    }
}

fn numeric_binary(left: EvalValue, right: EvalValue, apply: impl Fn(f64, f64) -> f64) -> EvalValue {
    EvalValue::Number(apply(
        left.number().unwrap_or(0.0),
        right.number().unwrap_or(0.0),
    ))
}

fn compare_binary(
    left: EvalValue,
    right: EvalValue,
    apply: impl Fn(std::cmp::Ordering) -> bool,
) -> EvalValue {
    let ordering = match (left.number(), right.number()) {
        (Some(left), Some(right)) => left.total_cmp(&right),
        _ => left.text().cmp(&right.text()),
    };
    EvalValue::Bool(apply(ordering))
}

fn eval_builtin(
    builtin: DocumentBuiltin,
    input: Option<EvalValue>,
    arguments: Vec<(String, EvalValue)>,
) -> EvalValue {
    let has_input = input.is_some();
    let mut values = input
        .into_iter()
        .chain(arguments.iter().map(|(_, value)| value.clone()));
    let first = values.next().unwrap_or(EvalValue::Null);
    match builtin {
        DocumentBuiltin::BoolAnd => {
            EvalValue::Bool(first.truthy() && values.all(|value| value.truthy()))
        }
        DocumentBuiltin::BoolNot | DocumentBuiltin::BoolToggle => EvalValue::Bool(!first.truthy()),
        DocumentBuiltin::BytesFind => {
            let needle = values.next().unwrap_or(EvalValue::Null).text();
            EvalValue::Number(first.text().find(&needle).unwrap_or(usize::MAX) as f64)
        }
        DocumentBuiltin::BytesSlice | DocumentBuiltin::TextSubstring => {
            let text = first.text();
            let start = named_number(&arguments, "from")
                .or_else(|| named_number(&arguments, "start"))
                .unwrap_or(0.0) as usize;
            let end = named_number(&arguments, "to")
                .or_else(|| named_number(&arguments, "end"))
                .map(|value| value as usize)
                .unwrap_or(text.len());
            EvalValue::Text(
                text.chars()
                    .skip(start)
                    .take(end.saturating_sub(start))
                    .collect(),
            )
        }
        DocumentBuiltin::BytesStartsWith | DocumentBuiltin::TextStartsWith => EvalValue::Bool(
            first
                .text()
                .starts_with(&values.next().unwrap_or(EvalValue::Null).text()),
        ),
        DocumentBuiltin::BytesToText => EvalValue::Text(first.text()),
        DocumentBuiltin::NumberToAsciiText => EvalValue::Text(
            first
                .number()
                .and_then(|value| FiniteReal::new(value).ok())
                .map(|value| {
                    format_number_ascii_text(
                        value,
                        named_number(&arguments, "width")
                            .and_then(|width| FiniteReal::new(width).ok()),
                    )
                })
                .unwrap_or_else(|| "?".to_owned()),
        ),
        DocumentBuiltin::ErrorNew => EvalValue::Tagged(
            "Error".to_owned(),
            BTreeMap::from([("text".to_owned(), first)]),
        ),
        DocumentBuiltin::ErrorText => match first {
            EvalValue::Tagged(_, mut fields) | EvalValue::Record(mut fields) => {
                fields.remove("text").unwrap_or(EvalValue::Null)
            }
            value => EvalValue::Text(value.text()),
        },
        DocumentBuiltin::ListAppend => {
            let mut list = match first {
                EvalValue::List(values) => values,
                EvalValue::Null => Vec::new(),
                value => vec![value],
            };
            list.extend(values);
            EvalValue::List(list)
        }
        DocumentBuiltin::ListChunk => {
            let size = named_number(&arguments, "size").unwrap_or(1.0).max(1.0) as usize;
            match first {
                EvalValue::List(values) => EvalValue::List(
                    values
                        .chunks(size)
                        .map(|chunk| EvalValue::List(chunk.to_vec()))
                        .collect(),
                ),
                _ => EvalValue::List(Vec::new()),
            }
        }
        DocumentBuiltin::ListCount | DocumentBuiltin::ListLength => {
            EvalValue::Number(match first {
                EvalValue::List(values) => values.len() as f64,
                _ => 0.0,
            })
        }
        DocumentBuiltin::ListGet => {
            let index = named_number(&arguments, "index").unwrap_or(0.0) as usize;
            match first {
                EvalValue::List(mut values) if index < values.len() => values.remove(index),
                _ => EvalValue::Null,
            }
        }
        DocumentBuiltin::ListIsNotEmpty => EvalValue::Bool(match first {
            EvalValue::List(values) => !values.is_empty(),
            _ => false,
        }),
        DocumentBuiltin::ListLatest => match first {
            EvalValue::List(mut values) => values.pop().unwrap_or(EvalValue::Null),
            value => value,
        },
        DocumentBuiltin::ListRange => {
            let from = named_number(&arguments, "from").unwrap_or(0.0) as i64;
            let to = named_number(&arguments, "to").unwrap_or(from as f64) as i64;
            EvalValue::List(
                (from..=to)
                    .map(|value| EvalValue::Number(value as f64))
                    .collect(),
            )
        }
        DocumentBuiltin::ListSortBy => first,
        DocumentBuiltin::ListSum => EvalValue::Number(match first {
            EvalValue::List(values) => values.iter().filter_map(EvalValue::number).sum(),
            value => value.number().unwrap_or(0.0),
        }),
        DocumentBuiltin::NumberBitWidth => EvalValue::Number(
            first
                .number()
                .and_then(|value| FiniteReal::new(value).ok())
                .and_then(|value| number_bit_width(value).ok())
                .map(FiniteReal::get)
                .unwrap_or(0.0),
        ),
        DocumentBuiltin::NumberCeil => EvalValue::Number(first.number().unwrap_or(0.0).ceil()),
        DocumentBuiltin::NumberFloor => EvalValue::Number(first.number().unwrap_or(0.0).floor()),
        DocumentBuiltin::NumberInterpolate => first,
        DocumentBuiltin::NumberMax => EvalValue::Number(
            std::iter::once(first.number().unwrap_or(0.0))
                .chain(values.filter_map(|value| value.number()))
                .fold(f64::NEG_INFINITY, f64::max),
        ),
        DocumentBuiltin::NumberMin => EvalValue::Number(
            std::iter::once(first.number().unwrap_or(0.0))
                .chain(values.filter_map(|value| value.number()))
                .fold(f64::INFINITY, f64::min),
        ),
        DocumentBuiltin::NumberProjectOffset
        | DocumentBuiltin::NumberProjectTime
        | DocumentBuiltin::NumberProjectWidth => EvalValue::Text(first.text()),
        DocumentBuiltin::NumberToText => {
            let integer_argument = |name: &str| {
                named_number(&arguments, name).and_then(|value| {
                    FiniteReal::new(value)
                        .ok()
                        .and_then(|value| value.to_i64_exact().ok())
                })
            };
            let format = NumberTextFormat {
                radix: integer_argument("radix")
                    .map(|value| u32::try_from(value).unwrap_or_default())
                    .unwrap_or(10),
                min_width: integer_argument("min_width")
                    .map(|value| usize::try_from(value).unwrap_or(usize::MAX))
                    .unwrap_or_default(),
                signed_width: integer_argument("signed_width")
                    .map(|value| u32::try_from(value).unwrap_or_default()),
                group_size: integer_argument("group_size")
                    .map(|value| usize::try_from(value).unwrap_or_default()),
                prefix: named_value(&arguments, "prefix").is_some_and(EvalValue::truthy),
            };
            first
                .number()
                .and_then(|value| FiniteReal::new(value).ok())
                .and_then(|value| format_number_text(value, format).ok())
                .map(EvalValue::Text)
                .unwrap_or(EvalValue::Null)
        }
        DocumentBuiltin::NumberRound => EvalValue::Number(first.number().unwrap_or(0.0).round()),
        DocumentBuiltin::NumberTruncate => EvalValue::Number(first.number().unwrap_or(0.0).trunc()),
        DocumentBuiltin::TextAllCharsIn => {
            let allowed = values.next().unwrap_or(EvalValue::Null).text();
            EvalValue::Bool(
                first
                    .text()
                    .chars()
                    .all(|character| allowed.contains(character)),
            )
        }
        DocumentBuiltin::TextConcat => {
            let separator = named_value(&arguments, "separator")
                .map(EvalValue::text)
                .unwrap_or_default();
            let mut parts = vec![first.text()];
            parts.extend(
                arguments
                    .iter()
                    .skip(usize::from(!has_input))
                    .filter(|(name, _)| name != "separator")
                    .map(|(_, value)| value.text()),
            );
            EvalValue::Text(parts.join(&separator))
        }
        DocumentBuiltin::TextContains => EvalValue::Bool(
            first
                .text()
                .contains(&values.next().unwrap_or(EvalValue::Null).text()),
        ),
        DocumentBuiltin::TextEmpty => EvalValue::Text(String::new()),
        DocumentBuiltin::TextIsEmpty => EvalValue::Bool(first.text().is_empty()),
        DocumentBuiltin::TextJoin => {
            let separator = named_value(&arguments, "separator")
                .map(EvalValue::text)
                .unwrap_or_default();
            let empty = named_value(&arguments, "empty")
                .map(EvalValue::text)
                .unwrap_or_default();
            EvalValue::Text(match first {
                EvalValue::List(values) if values.is_empty() => empty,
                EvalValue::List(values) => values
                    .into_iter()
                    .map(|value| value.text())
                    .collect::<Vec<_>>()
                    .join(&separator),
                value => value.text(),
            })
        }
        DocumentBuiltin::TextJoinLines => EvalValue::Text(match first {
            EvalValue::List(values) => values
                .into_iter()
                .map(|value| value.text())
                .collect::<Vec<_>>()
                .join("\n"),
            value => value.text(),
        }),
        DocumentBuiltin::TextLength => EvalValue::Number(first.text().chars().count() as f64),
        DocumentBuiltin::TextSpace => EvalValue::Text(" ".to_owned()),
        DocumentBuiltin::TextTimeRangeLabel => {
            let end = named_value(&arguments, "end")
                .map(EvalValue::text)
                .unwrap_or_default();
            let unit = named_value(&arguments, "unit")
                .map(EvalValue::text)
                .unwrap_or_default();
            EvalValue::Text(format!("{} {unit} - {end} {unit}", first.text()))
        }
        DocumentBuiltin::TextToBytes => EvalValue::Bytes(first.text().into_bytes().into()),
        DocumentBuiltin::TextToLowercase => EvalValue::Text(first.text().to_lowercase()),
        DocumentBuiltin::TextToNumber => {
            EvalValue::Number(first.text().parse().unwrap_or_default())
        }
        DocumentBuiltin::TextToUppercase => EvalValue::Text(first.text().to_uppercase()),
        DocumentBuiltin::TextTrim => EvalValue::Text(first.text().trim().to_owned()),
        DocumentBuiltin::LightAmbient
        | DocumentBuiltin::LightDirectional
        | DocumentBuiltin::LightSpot
        | DocumentBuiltin::Svg => EvalValue::Record(arguments.into_iter().collect()),
        DocumentBuiltin::RouterGoTo | DocumentBuiltin::RouterRoute => first,
        DocumentBuiltin::DirectoryEntries
        | DocumentBuiltin::FileWriteText
        | DocumentBuiltin::LogError
        | DocumentBuiltin::LogInfo
        | DocumentBuiltin::UlidGenerate
        | DocumentBuiltin::UrlEncode => first,
    }
}

fn named_value<'a>(arguments: &'a [(String, EvalValue)], name: &str) -> Option<&'a EvalValue> {
    arguments
        .iter()
        .find_map(|(candidate, value)| (candidate == name).then_some(value))
}

fn named_number(arguments: &[(String, EvalValue)], name: &str) -> Option<f64> {
    named_value(arguments, name).and_then(EvalValue::number)
}

fn constructor_kind(constructor: DocumentConstructor, direction: Option<&str>) -> DocumentNodeKind {
    match constructor {
        DocumentConstructor::ElementStripe | DocumentConstructor::SceneElementStripe => {
            if direction.is_some_and(|value| value.eq_ignore_ascii_case("row")) {
                DocumentNodeKind::Row
            } else {
                DocumentNodeKind::Stack
            }
        }
        DocumentConstructor::ElementParagraph | DocumentConstructor::SceneElementParagraph => {
            DocumentNodeKind::Row
        }
        DocumentConstructor::ElementText
        | DocumentConstructor::ElementLabel
        | DocumentConstructor::ElementLink
        | DocumentConstructor::SceneElementText
        | DocumentConstructor::SceneElementLabel
        | DocumentConstructor::SceneElementLink => DocumentNodeKind::Text,
        DocumentConstructor::ElementButton | DocumentConstructor::SceneElementButton => {
            DocumentNodeKind::Button
        }
        DocumentConstructor::ElementCheckbox | DocumentConstructor::SceneElementCheckbox => {
            DocumentNodeKind::Checkbox
        }
        DocumentConstructor::ElementTextInput | DocumentConstructor::SceneElementTextInput => {
            DocumentNodeKind::TextInput
        }
        DocumentConstructor::ElementProgram | DocumentConstructor::SceneElementProgram => {
            DocumentNodeKind::EmbeddedProgram
        }
        DocumentConstructor::ElementEmbeddedMedia
        | DocumentConstructor::SceneElementEmbeddedMedia => DocumentNodeKind::EmbeddedMedia,
        DocumentConstructor::ElementMap | DocumentConstructor::SceneElementMap => {
            DocumentNodeKind::MapViewport
        }
        DocumentConstructor::ElementContainer
        | DocumentConstructor::SceneElementBlock
        | DocumentConstructor::DocumentNew
        | DocumentConstructor::SceneNew => DocumentNodeKind::Stack,
    }
}

fn apply_text_argument(node: &mut DocumentNode, name: &str, value: EvalValue) {
    let text = if matches!(name, "label" | "placeholder") {
        record_value(&value, "text")
            .map(EvalValue::text)
            .unwrap_or_else(|| value.text())
    } else {
        value.text()
    };
    match name {
        "placeholder" => {
            node.style
                .insert("placeholder".to_owned(), StyleValue::Text(text));
        }
        "label" => {
            node.style
                .insert("label".to_owned(), StyleValue::Text(text.clone()));
            node.text = Some(TextValue { text });
        }
        "text" | "value" | "display_value" | "contents" => {
            node.text = Some(TextValue { text });
        }
        _ => {
            node.style.insert(name.to_owned(), StyleValue::Text(text));
        }
    }
}

fn apply_value_argument(node: &mut DocumentNode, name: &str, value: EvalValue) {
    if name == "direction" || name == "element" || name == "events" {
        return;
    }
    if name == "items" || name == "children" || name == "child" || name == "root" {
        return;
    }
    if name == "input_id" {
        if node.kind == DocumentNodeKind::TextInput {
            node.text_input_id = nonempty_text(&value).map(TextInputId);
        }
        return;
    }
    if name == "activate_focus" {
        node.activation_focus = text_input_focus_request(&value);
        return;
    }
    if node.kind == DocumentNodeKind::EmbeddedProgram {
        let program = node
            .embedded_program
            .get_or_insert_with(EmbeddedProgramDescriptor::default);
        match name {
            "source" => {
                program.source = value.text();
                program.source_digest = crate::sha256_bytes(program.source.as_bytes());
            }
            "revision" => {
                program.revision = value.number().unwrap_or(0.0).max(0.0) as u64;
            }
            "artifact_id" => {
                program.artifact_id = value.text();
            }
            "artifact_retention" => {
                program.artifact_retention = match value.text().as_str() {
                    "Replaceable" | "replaceable" => ProgramArtifactRetention::Replaceable,
                    "Archive" | "archive" => ProgramArtifactRetention::Archive,
                    _ => ProgramArtifactRetention::Ephemeral,
                };
            }
            "support_sources" => {
                program.support_sources = embedded_program_source_units(&value);
            }
            "bootstrap_source" => {
                program.bootstrap_source = value.text();
                program.bootstrap_source_digest =
                    crate::sha256_bytes(program.bootstrap_source.as_bytes());
            }
            "bootstrap_artifact_id" => {
                program.bootstrap_artifact_id = value.text();
            }
            "bootstrap_revision" => {
                program.bootstrap_revision = value.number().unwrap_or(0.0).max(0.0) as u64;
            }
            "capability_profile" => {
                program.capability_profile = match value.text().as_str() {
                    "PublicClient" | "public_client" => ProgramCapabilityProfile::PublicClient,
                    _ => ProgramCapabilityProfile::PublicClient,
                };
            }
            "session_key" => {
                program.session_key = value.text();
            }
            "mount" => {
                program.mount = value.truthy();
            }
            _ => {}
        }
        if matches!(
            name,
            "source"
                | "revision"
                | "artifact_id"
                | "artifact_retention"
                | "support_sources"
                | "bootstrap_source"
                | "bootstrap_artifact_id"
                | "bootstrap_revision"
                | "capability_profile"
                | "session_key"
                | "mount"
        ) {
            return;
        }
    }
    if let Some(style) = scalar_style_value(&value) {
        let key = match name {
            "rounded_corners" => "border_radius",
            other => other,
        };
        node.style.insert(key.to_owned(), style);
    }
}

fn nonempty_text(value: &EvalValue) -> Option<String> {
    let text = value.text();
    (!text.is_empty()).then_some(text)
}

fn text_input_focus_request(value: &EvalValue) -> Option<TextInputFocusRequest> {
    let fields = record_fields(value)?;
    let input_id = fields
        .get("input_id")
        .and_then(nonempty_text)
        .map(TextInputId)?;
    let coordinate = |name: &str| fields.get(name).and_then(positive_integral_u64);
    Some(TextInputFocusRequest {
        input_id,
        line: coordinate("line")?,
        column: coordinate("column")?,
    })
}

fn positive_integral_u64(value: &EvalValue) -> Option<u64> {
    let value = value.number()?;
    (value.is_finite() && value >= 1.0 && value <= u64::MAX as f64 && value.fract() == 0.0)
        .then_some(value as u64)
}

fn embedded_program_source_units(value: &EvalValue) -> Vec<EmbeddedProgramSourceUnit> {
    let EvalValue::List(values) = value else {
        return Vec::new();
    };
    values
        .iter()
        .map(|value| {
            let fields = match value {
                EvalValue::Record(fields) | EvalValue::Tagged(_, fields) => Some(fields),
                EvalValue::MappedRow { fields, .. } => Some(fields),
                _ => None,
            };
            let path = fields
                .and_then(|fields| fields.get("path"))
                .map(|value| value.text())
                .unwrap_or_default();
            let source = fields
                .and_then(|fields| fields.get("source"))
                .map(|value| value.text())
                .unwrap_or_default();
            let source_digest = crate::sha256_bytes(source.as_bytes());
            EmbeddedProgramSourceUnit {
                path,
                source,
                source_digest,
            }
        })
        .collect()
}

fn evaluate_map_viewport_descriptor(
    arguments: impl IntoIterator<Item = (DocumentArgumentRole, EvalValue)>,
) -> Result<MapViewportDescriptor, DocumentError> {
    let mut camera = None;
    let mut bounds = None;
    let mut tile_source = None;
    let mut overlays = None;
    let mut interaction = None;
    let mut generation = None;
    for (role, value) in arguments {
        match role {
            DocumentArgumentRole::MapCamera => {
                set_map_descriptor_part(&mut camera, map_camera(&value)?, "camera")?
            }
            DocumentArgumentRole::MapBounds => {
                set_map_descriptor_part(&mut bounds, map_bounds(&value)?, "bounds")?
            }
            DocumentArgumentRole::MapTileSource => {
                set_map_descriptor_part(&mut tile_source, map_tile_source(&value)?, "tile_source")?
            }
            DocumentArgumentRole::MapOverlays => {
                set_map_descriptor_part(&mut overlays, map_overlays(&value)?, "overlays")?
            }
            DocumentArgumentRole::MapInteraction => {
                set_map_descriptor_part(&mut interaction, map_interaction(&value)?, "interaction")?
            }
            DocumentArgumentRole::MapGeneration => set_map_descriptor_part(
                &mut generation,
                MapViewportGeneration(map_u64(&value, "generation")?),
                "generation",
            )?,
            _ => {}
        }
    }
    let descriptor = MapViewportDescriptor {
        generation: generation.unwrap_or_default(),
        camera: required_map_descriptor_part(camera, "camera")?,
        bounds: required_map_descriptor_part(bounds, "bounds")?,
        tile_source: required_map_descriptor_part(tile_source, "tile_source")?,
        overlays: required_map_descriptor_part(overlays, "overlays")?,
        interaction: required_map_descriptor_part(interaction, "interaction")?,
    };
    descriptor
        .validate()
        .map_err(|error| DocumentError::Evaluation(error.to_string()))?;
    Ok(descriptor)
}

fn set_map_descriptor_part<T>(
    target: &mut Option<T>,
    value: T,
    path: &str,
) -> Result<(), DocumentError> {
    if target.replace(value).is_some() {
        return Err(map_evaluation_error(
            path,
            "constructor argument is supplied more than once",
        ));
    }
    Ok(())
}

fn required_map_descriptor_part<T>(value: Option<T>, path: &str) -> Result<T, DocumentError> {
    value.ok_or_else(|| map_evaluation_error(path, "constructor argument is missing"))
}

fn map_camera(value: &EvalValue) -> Result<MapCamera, DocumentError> {
    let fields = map_record(value, "camera")?;
    Ok(MapCamera {
        longitude: map_required_number(fields, "camera", "longitude")?,
        latitude: map_required_number(fields, "camera", "latitude")?,
        zoom: map_required_number(fields, "camera", "zoom")?,
        bearing: map_required_number(fields, "camera", "bearing")?,
    })
}

fn map_bounds(value: &EvalValue) -> Result<MapViewportBounds, DocumentError> {
    let fields = map_record(value, "bounds")?;
    Ok(MapViewportBounds {
        width: map_required_number(fields, "bounds", "width")?,
        height: map_required_number(fields, "bounds", "height")?,
        scale: map_required_number(fields, "bounds", "scale")?,
    })
}

fn map_tile_source(value: &EvalValue) -> Result<MapTileSourceRef, DocumentError> {
    let fields = map_record(value, "tile_source")?;
    let allowed_origins = map_list(
        map_required_field(fields, "tile_source", "allowed_origins")?,
        "tile_source.allowed_origins",
    )?
    .iter()
    .enumerate()
    .map(|(index, origin)| map_text(origin, &format!("tile_source.allowed_origins[{index}]")))
    .collect::<Result<Vec<_>, _>>()?;
    Ok(MapTileSourceRef {
        id: MapTileSourceId(map_required_text(fields, "tile_source", "id")?),
        url_template_capability: map_required_text(
            fields,
            "tile_source",
            "url_template_capability",
        )?,
        min_zoom: map_u8(
            map_required_field(fields, "tile_source", "min_zoom")?,
            "tile_source.min_zoom",
        )?,
        max_zoom: map_u8(
            map_required_field(fields, "tile_source", "max_zoom")?,
            "tile_source.max_zoom",
        )?,
        tile_size: map_u16(
            map_required_field(fields, "tile_source", "tile_size")?,
            "tile_source.tile_size",
        )?,
        attribution: map_required_text(fields, "tile_source", "attribution")?,
        allowed_origins,
    })
}

fn map_interaction(value: &EvalValue) -> Result<MapInteractionPolicy, DocumentError> {
    let fields = map_record(value, "interaction")?;
    Ok(MapInteractionPolicy {
        pan: map_required_bool(fields, "interaction", "pan")?,
        wheel_zoom: map_required_bool(fields, "interaction", "wheel_zoom")?,
        pinch_zoom: map_required_bool(fields, "interaction", "pinch_zoom")?,
        keyboard_zoom: map_required_bool(fields, "interaction", "keyboard_zoom")?,
    })
}

fn map_overlays(value: &EvalValue) -> Result<Vec<MapOverlayDescriptor>, DocumentError> {
    map_list(value, "overlays")?
        .iter()
        .enumerate()
        .map(|(index, overlay)| map_overlay(overlay, index))
        .collect()
}

fn map_overlay(value: &EvalValue, index: usize) -> Result<MapOverlayDescriptor, DocumentError> {
    let path = format!("overlays[{index}]");
    let EvalValue::Tagged(kind, fields) = value else {
        return Err(map_evaluation_error(
            &path,
            "must be a Point, Cluster, Polyline, Polygon, or Label tagged record",
        ));
    };
    let paint = fields
        .get("paint")
        .map(|paint| map_overlay_paint(paint, &format!("{path}.paint")))
        .transpose()?
        .unwrap_or_default();
    let geometry = match kind.as_str() {
        "Point" => MapOverlayGeometry::Point {
            position: map_required_coordinate(fields, &path, "position")?,
            radius: map_required_number(fields, &path, "radius")?,
            symbol_ref: fields
                .get("symbol_ref")
                .map(|value| map_text(value, &format!("{path}.symbol_ref")))
                .transpose()?,
        },
        "Cluster" => MapOverlayGeometry::Cluster {
            position: map_required_coordinate(fields, &path, "position")?,
            count: map_u64(
                map_required_field(fields, &path, "count")?,
                &format!("{path}.count"),
            )?,
            radius: map_required_number(fields, &path, "radius")?,
        },
        "Polyline" => MapOverlayGeometry::Polyline {
            points: map_coordinate_list(
                map_required_field(fields, &path, "points")?,
                &format!("{path}.points"),
            )?,
        },
        "Polygon" => {
            let rings_path = format!("{path}.rings");
            let rings = map_list(map_required_field(fields, &path, "rings")?, &rings_path)?
                .iter()
                .enumerate()
                .map(|(ring_index, ring)| {
                    map_coordinate_list(ring, &format!("{rings_path}[{ring_index}]"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            MapOverlayGeometry::Polygon { rings }
        }
        "Label" => MapOverlayGeometry::Label {
            position: map_required_coordinate(fields, &path, "position")?,
            text: map_required_text(fields, &path, "text")?,
            collision_priority: map_i32(
                map_required_field(fields, &path, "collision_priority")?,
                &format!("{path}.collision_priority"),
            )?,
            font_size: map_required_number(fields, &path, "font_size")?,
        },
        other => {
            return Err(map_evaluation_error(
                &path,
                format!("has unsupported overlay kind `{other}`"),
            ));
        }
    };
    Ok(MapOverlayDescriptor {
        id: MapOverlayId(map_required_text(fields, &path, "id")?),
        hit_identity: MapHitIdentity(map_required_text(fields, &path, "hit_identity")?),
        z_order: map_i32(
            map_required_field(fields, &path, "z_order")?,
            &format!("{path}.z_order"),
        )?,
        selected: map_required_bool(fields, &path, "selected")?,
        focused: fields
            .get("focused")
            .map(|value| map_bool(value, &format!("{path}.focused")))
            .transpose()?
            .unwrap_or(false),
        paint,
        geometry,
    })
}

fn map_overlay_paint(value: &EvalValue, path: &str) -> Result<MapOverlayPaint, DocumentError> {
    let fields = map_record(value, path)?;
    Ok(MapOverlayPaint {
        fill: fields
            .get("fill")
            .map(|value| map_text(value, &format!("{path}.fill")))
            .transpose()?,
        stroke: fields
            .get("stroke")
            .map(|value| map_text(value, &format!("{path}.stroke")))
            .transpose()?,
        stroke_width: fields
            .get("stroke_width")
            .map(|value| map_number(value, &format!("{path}.stroke_width")))
            .transpose()?
            .unwrap_or(1.0),
        opacity: fields
            .get("opacity")
            .map(|value| map_number(value, &format!("{path}.opacity")))
            .transpose()?
            .unwrap_or(1.0),
    })
}

fn map_required_coordinate(
    fields: &BTreeMap<String, EvalValue>,
    path: &str,
    name: &str,
) -> Result<MapCoordinate, DocumentError> {
    map_coordinate(
        map_required_field(fields, path, name)?,
        &format!("{path}.{name}"),
    )
}

fn map_coordinate(value: &EvalValue, path: &str) -> Result<MapCoordinate, DocumentError> {
    let fields = map_record(value, path)?;
    Ok(MapCoordinate {
        longitude: map_required_number(fields, path, "longitude")?,
        latitude: map_required_number(fields, path, "latitude")?,
    })
}

fn map_coordinate_list(value: &EvalValue, path: &str) -> Result<Vec<MapCoordinate>, DocumentError> {
    map_list(value, path)?
        .iter()
        .enumerate()
        .map(|(index, coordinate)| map_coordinate(coordinate, &format!("{path}[{index}]")))
        .collect()
}

fn map_record<'a>(
    value: &'a EvalValue,
    path: &str,
) -> Result<&'a BTreeMap<String, EvalValue>, DocumentError> {
    record_fields(value).ok_or_else(|| map_evaluation_error(path, "must be a record"))
}

fn map_list<'a>(value: &'a EvalValue, path: &str) -> Result<&'a [EvalValue], DocumentError> {
    match value {
        EvalValue::List(values) => Ok(values),
        _ => Err(map_evaluation_error(path, "must be a list")),
    }
}

fn map_required_field<'a>(
    fields: &'a BTreeMap<String, EvalValue>,
    path: &str,
    name: &str,
) -> Result<&'a EvalValue, DocumentError> {
    fields
        .get(name)
        .ok_or_else(|| map_evaluation_error(format!("{path}.{name}"), "field is missing"))
}

fn map_required_number(
    fields: &BTreeMap<String, EvalValue>,
    path: &str,
    name: &str,
) -> Result<f64, DocumentError> {
    map_number(
        map_required_field(fields, path, name)?,
        &format!("{path}.{name}"),
    )
}

fn map_required_text(
    fields: &BTreeMap<String, EvalValue>,
    path: &str,
    name: &str,
) -> Result<String, DocumentError> {
    map_text(
        map_required_field(fields, path, name)?,
        &format!("{path}.{name}"),
    )
}

fn map_required_bool(
    fields: &BTreeMap<String, EvalValue>,
    path: &str,
    name: &str,
) -> Result<bool, DocumentError> {
    map_bool(
        map_required_field(fields, path, name)?,
        &format!("{path}.{name}"),
    )
}

fn map_number(value: &EvalValue, path: &str) -> Result<f64, DocumentError> {
    match value {
        EvalValue::Number(value) => Ok(*value),
        _ => Err(map_evaluation_error(path, "must be a number")),
    }
}

fn map_text(value: &EvalValue, path: &str) -> Result<String, DocumentError> {
    match value {
        EvalValue::Text(value) | EvalValue::Enum(value) => Ok(value.clone()),
        _ => Err(map_evaluation_error(path, "must be text")),
    }
}

fn map_bool(value: &EvalValue, path: &str) -> Result<bool, DocumentError> {
    match value {
        EvalValue::Bool(value) => Ok(*value),
        _ => Err(map_evaluation_error(path, "must be a boolean")),
    }
}

fn map_u8(value: &EvalValue, path: &str) -> Result<u8, DocumentError> {
    map_integral_number(value, path, 0.0, f64::from(u8::MAX)).map(|value| value as u8)
}

fn map_u16(value: &EvalValue, path: &str) -> Result<u16, DocumentError> {
    map_integral_number(value, path, 0.0, f64::from(u16::MAX)).map(|value| value as u16)
}

fn map_u64(value: &EvalValue, path: &str) -> Result<u64, DocumentError> {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    map_integral_number(value, path, 0.0, MAX_SAFE_INTEGER).map(|value| value as u64)
}

fn map_i32(value: &EvalValue, path: &str) -> Result<i32, DocumentError> {
    map_integral_number(value, path, f64::from(i32::MIN), f64::from(i32::MAX))
        .map(|value| value as i32)
}

fn map_integral_number(
    value: &EvalValue,
    path: &str,
    minimum: f64,
    maximum: f64,
) -> Result<f64, DocumentError> {
    let value = map_number(value, path)?;
    if value.is_finite() && value.fract() == 0.0 && value >= minimum && value <= maximum {
        Ok(value)
    } else {
        Err(map_evaluation_error(
            path,
            format!("must be an integer within {minimum}..={maximum}"),
        ))
    }
}

fn map_evaluation_error(path: impl AsRef<str>, message: impl AsRef<str>) -> DocumentError {
    DocumentError::Evaluation(format!(
        "MapViewport {}: {}",
        path.as_ref(),
        message.as_ref()
    ))
}

fn lower_style_record(record: &BTreeMap<String, EvalValue>, style: &mut StyleMap) {
    for (name, value) in record {
        match name.as_str() {
            "background" => {
                if let Some(color) = record_value(value, "color").and_then(scalar_style_value) {
                    style.insert("background".to_owned(), color);
                } else if let Some(value) = scalar_style_value(value) {
                    style.insert("background".to_owned(), value);
                }
                if let Some(url) = record_value(value, "url").and_then(scalar_style_value) {
                    style.insert("background_url".to_owned(), url.clone());
                    style.insert("asset_url".to_owned(), url);
                }
            }
            "font" => lower_font(value, style),
            "line" => lower_line(value, style),
            "align" => lower_align(value, style),
            "padding" | "move" => lower_spacing(name, value, style),
            "border" | "outline" => {
                if let Some(color) = record_value(value, "color").and_then(scalar_style_value) {
                    style.insert("border".to_owned(), color);
                } else if let Some(value) = scalar_style_value(value) {
                    style.insert("border".to_owned(), value);
                }
                if let Some(width) = record_value(value, "width").and_then(scalar_style_value) {
                    style.insert("border_width".to_owned(), width);
                }
            }
            "borders" => lower_borders(value, style),
            "rounded_corners" => {
                if let Some(value) = scalar_style_value(value) {
                    style.insert("border_radius".to_owned(), value);
                }
            }
            "width" | "height" => lower_dimension(name, value, style),
            "material" => lower_material(value, style),
            "shadows" => lower_shadows(value, style),
            "glow" => lower_glow(value, style),
            "spring_range" => lower_nested_scalars("spring_range", value, style),
            "transform" => {
                if let Some(value) = record_value(value, "rotate").and_then(scalar_style_value) {
                    style.insert("rotate".to_owned(), value);
                }
            }
            "size" => {
                if let Some(value) = scalar_style_value(value) {
                    if matches!(
                        node_kind_hint(style),
                        Some(DocumentNodeKind::Button | DocumentNodeKind::Checkbox)
                    ) {
                        style.insert("box_size".to_owned(), value.clone());
                    }
                    style.insert("size".to_owned(), value);
                }
            }
            _ => {
                if let Some(value) = scalar_style_value(value) {
                    style.insert(name.clone(), value);
                } else if let EvalValue::Record(fields) | EvalValue::Tagged(_, fields) = value {
                    for (nested, value) in fields {
                        if let Some(value) = scalar_style_value(value) {
                            style.insert(format!("{name}_{nested}"), value);
                        }
                    }
                }
            }
        }
    }
}

fn lower_dimension(name: &str, value: &EvalValue, style: &mut StyleMap) {
    if let Some(value) = scalar_style_value(value) {
        style.insert(name.to_owned(), value);
        return;
    }
    let Some(fields) = record_fields(value) else {
        return;
    };
    for (field, key) in [
        ("sizing", name.to_owned()),
        ("minimum", format!("min_{name}")),
        ("maximum", format!("max_{name}")),
    ] {
        let Some(mut value) = fields.get(field).and_then(scalar_style_value) else {
            continue;
        };
        if value == StyleValue::Text("Screen".to_owned()) {
            value = StyleValue::Text("Fill".to_owned());
        }
        style.insert(key, value);
    }
}

fn node_kind_hint(_style: &StyleMap) -> Option<DocumentNodeKind> {
    None
}

fn lower_font(value: &EvalValue, style: &mut StyleMap) {
    let Some(fields) = record_fields(value) else {
        return;
    };
    for (name, value) in fields {
        let key = match name.as_str() {
            "family" => "font",
            "style" => "font_style",
            "size" | "color" | "weight" => name,
            "line" => {
                if let Some(line) = record_fields(value) {
                    if let Some(value) = line.get("strike").and_then(scalar_style_value) {
                        style.insert("strikethrough".to_owned(), value);
                    }
                    if let Some(value) = line.get("underline").and_then(scalar_style_value) {
                        style.insert("underline_if".to_owned(), value.clone());
                        style.insert("__hover_underline_if".to_owned(), value);
                    }
                }
                continue;
            }
            _ => continue,
        };
        if let Some(value) = scalar_style_value(value) {
            style.insert(key.to_owned(), value);
        }
    }
}

fn lower_line(value: &EvalValue, style: &mut StyleMap) {
    let Some(line) = record_fields(value) else {
        return;
    };
    if let Some(value) = line.get("strike").and_then(scalar_style_value) {
        style.insert("strikethrough".to_owned(), value);
    }
    if let Some(value) = line.get("underline").and_then(scalar_style_value) {
        style.insert("underline_if".to_owned(), value.clone());
        style.insert("__hover_underline_if".to_owned(), value);
    }
}

fn lower_align(value: &EvalValue, style: &mut StyleMap) {
    if let Some(fields) = record_fields(value) {
        if let Some(row) = fields.get("row") {
            lower_align_axis("x", row, style);
        }
        if let Some(column) = fields.get("column") {
            lower_align_axis("y", column, style);
        }
    } else {
        lower_align_axis("x", value, style);
    }
}

fn lower_align_axis(axis: &str, value: &EvalValue, style: &mut StyleMap) {
    let value = value.text().to_ascii_lowercase();
    if value == "center" && axis == "x" {
        style.insert("center".to_owned(), StyleValue::Bool(true));
    }
    if !value.is_empty() {
        style.insert(format!("align_{axis}"), StyleValue::Text(value));
    }
}

fn lower_spacing(prefix: &str, value: &EvalValue, style: &mut StyleMap) {
    let Some(fields) = record_fields(value) else {
        if let Some(value) = scalar_style_value(value) {
            style.insert(prefix.to_owned(), value);
        }
        return;
    };
    for (name, value) in fields {
        let keys: &[&str] = match name.as_str() {
            "row" => &["left", "right"],
            "column" => &["top", "bottom"],
            "top" => &["top"],
            "right" => &["right"],
            "bottom" => &["bottom"],
            "left" => &["left"],
            "closer" => &["closer"],
            "further" => &["further"],
            "up" => &["up"],
            _ => &[],
        };
        if let Some(value) = scalar_style_value(value) {
            for key in keys {
                style.insert(format!("{prefix}_{key}"), value.clone());
            }
        }
    }
}

fn lower_borders(value: &EvalValue, style: &mut StyleMap) {
    let Some(sides) = record_fields(value) else {
        return;
    };
    for (side, value) in sides {
        if let Some(color) = record_value(value, "color").and_then(scalar_style_value) {
            style.insert(format!("border_{side}"), color);
        }
        if let Some(width) = record_value(value, "width").and_then(scalar_style_value) {
            style.insert(format!("border_{side}_width"), width);
        }
    }
}

fn lower_material(value: &EvalValue, style: &mut StyleMap) {
    let Some(fields) = record_fields(value) else {
        if let Some(value) = scalar_style_value(value) {
            style.insert("material".to_owned(), value);
        }
        return;
    };
    for (name, value) in fields {
        if name == "color" {
            if let Some(value) = scalar_style_value(value) {
                style.insert("background".to_owned(), value.clone());
                style.insert("material_color".to_owned(), value);
            }
        } else if let Some(value) = scalar_style_value(value) {
            style.insert(name.clone(), value);
        }
    }
}

fn lower_shadows(value: &EvalValue, style: &mut StyleMap) {
    let EvalValue::List(shadows) = value else {
        return;
    };
    for (offset, shadow) in shadows.iter().enumerate() {
        let Some(fields) = record_fields(shadow) else {
            continue;
        };
        let index = offset + 1;
        for (name, value) in fields {
            let key = match name.as_str() {
                "x" | "y" | "blur" | "spread" | "color" => {
                    format!("box_shadow_{index}_{name}")
                }
                "direction" if value.text().eq_ignore_ascii_case("inwards") => {
                    style.insert(format!("box_shadow_{index}_inset"), StyleValue::Bool(true));
                    continue;
                }
                _ => continue,
            };
            if let Some(value) = scalar_style_value(value) {
                style.insert(key, value);
            }
        }
    }
}

fn lower_glow(value: &EvalValue, style: &mut StyleMap) {
    if let Some(color) = record_value(value, "color").and_then(scalar_style_value) {
        style.insert("glow_color".to_owned(), color.clone());
        style.insert("box_shadow_8_color".to_owned(), color);
        style
            .entry("box_shadow_8_x".to_owned())
            .or_insert(StyleValue::Number(0.0));
        style
            .entry("box_shadow_8_y".to_owned())
            .or_insert(StyleValue::Number(0.0));
        style
            .entry("box_shadow_8_blur".to_owned())
            .or_insert(StyleValue::Number(18.0));
    }
    if let Some(intensity) = record_value(value, "intensity").and_then(scalar_style_value) {
        style.insert("glow_intensity".to_owned(), intensity);
    }
}

fn lower_nested_scalars(prefix: &str, value: &EvalValue, style: &mut StyleMap) {
    let Some(fields) = record_fields(value) else {
        return;
    };
    for (name, value) in fields {
        if let Some(value) = scalar_style_value(value) {
            style.insert(format!("{prefix}_{name}"), value);
        }
    }
}

fn scalar_style_value(value: &EvalValue) -> Option<StyleValue> {
    match value {
        EvalValue::Bool(value) => Some(StyleValue::Bool(*value)),
        EvalValue::Number(value) => Some(StyleValue::Number(*value)),
        EvalValue::Text(value) | EvalValue::Enum(value) => Some(StyleValue::Text(value.clone())),
        EvalValue::Tagged(_, _) => Some(StyleValue::Text(value.text())),
        _ => None,
    }
}

fn record_fields(value: &EvalValue) -> Option<&BTreeMap<String, EvalValue>> {
    match value {
        EvalValue::Record(fields)
        | EvalValue::MappedRow { fields, .. }
        | EvalValue::Tagged(_, fields) => Some(fields),
        _ => None,
    }
}

fn record_value<'a>(value: &'a EvalValue, name: &str) -> Option<&'a EvalValue> {
    record_fields(value)?.get(name)
}

fn attach_sources(
    runtime: &DocumentRuntime,
    session: &MachineInstance,
    node: &mut DocumentNode,
    value: &EvalValue,
    env: &EvalEnv,
) -> Result<(), DocumentError> {
    let mut sources = Vec::new();
    collect_sources(value, None, true, &mut sources);
    sources.sort_unstable_by(|left, right| {
        (left.0.0, left.1.as_deref()).cmp(&(right.0.0, right.1.as_deref()))
    });
    sources.dedup();
    for (ordinal, (source, intent)) in sources.into_iter().enumerate() {
        let path = runtime.routes.get(&source).ok_or_else(|| {
            DocumentError::InvalidPlan(format!("source {} has no route path", source.0))
        })?;
        let route = runtime.source_route_token(session, source, env)?;
        let route_intent = path.rsplit('.').next().unwrap_or("source");
        let intent = intent
            .or_else(|| host_source_intent(route_intent).then(|| route_intent.to_owned()))
            .unwrap_or_else(|| "source".to_owned());
        node.source_bindings.push(SourceBinding {
            id: SourceBindingId(format!(
                "source:{}:{}:{}:{}",
                node.id.0, source.0, ordinal, route.binding_epoch
            )),
            source_path: path.clone(),
            intent,
            route: Some(route),
        });
    }
    Ok(())
}

fn collect_sources(
    value: &EvalValue,
    inherited_intent: Option<&str>,
    allow_unqualified: bool,
    sources: &mut Vec<(SourceId, Option<String>)>,
) {
    match value {
        EvalValue::Source(source) if inherited_intent.is_some() || allow_unqualified => {
            sources.push((*source, inherited_intent.map(str::to_owned)));
        }
        EvalValue::Record(fields)
        | EvalValue::MappedRow { fields, .. }
        | EvalValue::Tagged(_, fields) => {
            for (name, value) in fields {
                let explicit_intent = host_source_intent(name).then_some(name.as_str());
                let intent = explicit_intent.or(inherited_intent);
                collect_sources(
                    value,
                    intent,
                    explicit_intent.is_some() || inherited_intent.is_some(),
                    sources,
                );
            }
        }
        EvalValue::List(values) => {
            for value in values {
                collect_sources(value, inherited_intent, allow_unqualified, sources);
            }
        }
        _ => {}
    }
}

fn row_source_remainder<'a>(path: &'a str, suffix: &str) -> Option<&'a str> {
    if path == suffix {
        return Some("");
    }
    let qualified = format!(".{suffix}");
    let offset = path.rfind(&qualified)?;
    let remainder = &path[offset + qualified.len()..];
    if remainder.is_empty() {
        Some("")
    } else {
        remainder.strip_prefix('.')
    }
}

fn insert_row_source(
    fields: &mut BTreeMap<String, EvalValue>,
    path: &str,
    source: SourceId,
) -> bool {
    let mut parts = path.splitn(2, '.');
    let Some(head) = parts.next().filter(|part| !part.is_empty()) else {
        return false;
    };
    let Some(tail) = parts.next() else {
        return fields
            .insert(head.to_owned(), EvalValue::Source(source))
            .is_none();
    };
    let value = fields
        .entry(head.to_owned())
        .or_insert_with(|| EvalValue::Record(BTreeMap::new()));
    let EvalValue::Record(children) = value else {
        return false;
    };
    insert_row_source(children, tail, source)
}

fn host_source_intent(value: &str) -> bool {
    matches!(
        value,
        "activate"
            | "blur"
            | "cancel"
            | "change"
            | "click"
            | "compiled"
            | "commit"
            | "double_click"
            | "escape"
            | "focus"
            | "input"
            | "key_down"
            | "open"
            | "press"
            | "rejected"
            | "select"
            | "submit"
            | "text"
            | "toggle"
    )
}

fn style_bool(style: &StyleMap, name: &str) -> bool {
    match style.get(name) {
        Some(StyleValue::Bool(value)) => *value,
        Some(StyleValue::Text(value)) => value.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn field_name_index(plan: &MachinePlan) -> BTreeMap<FieldId, Vec<String>> {
    let mut result: BTreeMap<FieldId, Vec<String>> = BTreeMap::new();
    for entry in &plan.debug_map.fields {
        let Some(id) = entry
            .id
            .rsplit(':')
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .map(FieldId)
        else {
            continue;
        };
        let names = result.entry(id).or_default();
        names.push(entry.label.clone());
        if let Some(name) = entry.label.rsplit('.').next()
            && !names.iter().any(|candidate| candidate == name)
        {
            names.push(name.to_owned());
        }
    }
    result
}

fn field_state_alias_index(plan: &MachinePlan) -> BTreeMap<FieldId, boon_plan::StateId> {
    let states = plan
        .debug_map
        .state_slots
        .iter()
        .filter_map(|entry| {
            entry
                .id
                .rsplit(':')
                .next()
                .and_then(|value| value.parse().ok())
                .map(boon_plan::StateId)
                .map(|id| (entry.label.as_str(), id))
        })
        .collect::<BTreeMap<_, _>>();
    plan.debug_map
        .fields
        .iter()
        .filter_map(|entry| {
            let field = entry
                .id
                .rsplit(':')
                .next()
                .and_then(|value| value.parse().ok())
                .map(FieldId)?;
            states
                .get(entry.label.as_str())
                .copied()
                .map(|state| (field, state))
        })
        .collect()
}

fn frame_node_id(plan_id: u64, instance: Option<&str>) -> FrameNodeId {
    match instance {
        Some(instance) if !instance.is_empty() => FrameNodeId(format!("node:{plan_id}:{instance}")),
        _ => FrameNodeId(format!("node:{plan_id}")),
    }
}

fn clamp_range(range: Range<u64>, len: usize) -> Range<u64> {
    let len = len as u64;
    range.start.min(len)..range.end.min(len).max(range.start.min(len))
}

fn format_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        let value = format!("{value:.12}");
        value.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

fn format_record(tag: Option<&str>, fields: &BTreeMap<String, EvalValue>) -> String {
    let body = fields
        .iter()
        .map(|(name, value)| format!("{name}:{}", value.text()))
        .collect::<Vec<_>>()
        .join(",");
    tag.map(|tag| format!("{tag}[{body}]"))
        .unwrap_or_else(|| body)
}

fn scalar_dependent_indexes(nodes: &BTreeMap<FrameNodeId, RetainedNode>) -> ScalarDependentIndexes {
    let mut index: BTreeMap<DocumentDependency, BTreeSet<RetainedBindingKey>> = BTreeMap::new();
    let mut guarded: BTreeMap<ValueTarget, BTreeSet<RetainedBindingKey>> = BTreeMap::new();
    let mut guard_values: BTreeMap<(ValueTarget, Value), BTreeSet<RetainedBindingKey>> =
        BTreeMap::new();
    for (node, retained) in nodes {
        for (argument, value) in retained.arguments.iter().enumerate() {
            let Some(binding) = value.binding.as_ref() else {
                continue;
            };
            let key = RetainedBindingKey {
                node: node.clone(),
                argument,
            };
            for dependency in &binding.dependencies {
                index.entry(*dependency).or_default().insert(key.clone());
            }
            for (target, values) in &binding.guards {
                guarded.entry(*target).or_default().insert(key.clone());
                for value in values {
                    guard_values
                        .entry((*target, value.clone()))
                        .or_default()
                        .insert(key.clone());
                }
            }
        }
    }
    (index, guarded, guard_values)
}

fn diff_node(previous: &DocumentNode, next: &DocumentNode) -> Vec<DocumentPatch> {
    if previous.id != next.id
        || previous.kind != next.kind
        || previous.parent != next.parent
        || previous.children != next.children
        || previous.materialized != next.materialized
        || previous.map_viewport != next.map_viewport
    {
        return vec![DocumentPatch::UpsertNode(next.clone())];
    }
    if previous.text.is_some() && next.text.is_none()
        || previous.source_bindings.len() > next.source_bindings.len()
        || previous.scroll.is_some() && next.scroll.is_none()
    {
        return vec![DocumentPatch::UpsertNode(next.clone())];
    }

    let mut patches = Vec::new();
    if previous.embedded_program != next.embedded_program
        && let Some(program) = next.embedded_program.clone()
    {
        patches.push(DocumentPatch::SetEmbeddedProgram {
            id: next.id.clone(),
            program,
        });
    }
    if previous.text != next.text
        && let Some(text) = next.text.clone()
    {
        patches.push(DocumentPatch::SetText {
            id: next.id.clone(),
            text,
        });
    }
    let style = diff_style(&previous.style, &next.style);
    if !style.is_empty() {
        patches.push(DocumentPatch::SetStyle {
            id: next.id.clone(),
            patch: style,
        });
    }
    for (ordinal, binding) in next.source_bindings.iter().enumerate() {
        if previous.source_bindings.get(ordinal) != Some(binding) {
            patches.push(DocumentPatch::SetBindingAt {
                id: next.id.clone(),
                ordinal: ordinal as u32,
                binding: binding.clone(),
            });
        }
    }
    if previous.text_input_id != next.text_input_id
        || previous.activation_focus != next.activation_focus
    {
        patches.push(DocumentPatch::SetTextInputFocus {
            id: next.id.clone(),
            text_input_id: next.text_input_id.clone(),
            activation_focus: next.activation_focus.clone(),
        });
    }
    if previous.scroll != next.scroll
        && let Some(scroll) = next.scroll
    {
        patches.push(DocumentPatch::SetScroll {
            id: next.id.clone(),
            scroll,
        });
    }
    patches
}

fn mount_patches(frame: &DocumentFrame) -> Vec<DocumentPatch> {
    let mut order = Vec::new();
    collect_preorder(frame, &frame.root, &mut order);
    order
        .into_iter()
        .filter_map(|id| frame.nodes.get(&id).cloned())
        .map(|mut node| {
            node.children.clear();
            DocumentPatch::UpsertNode(node)
        })
        .collect()
}

fn collect_preorder(frame: &DocumentFrame, id: &FrameNodeId, order: &mut Vec<FrameNodeId>) {
    let Some(node) = frame.nodes.get(id) else {
        return;
    };
    order.push(id.clone());
    for child in &node.children {
        collect_preorder(frame, child, order);
    }
}

pub(crate) fn diff_frames(previous: &DocumentFrame, next: &DocumentFrame) -> Vec<DocumentPatch> {
    let mut patches = Vec::new();
    let removed = previous
        .nodes
        .keys()
        .filter(|id| !next.nodes.contains_key(*id))
        .cloned()
        .collect::<BTreeSet<_>>();
    for id in &removed {
        let parent_removed = previous
            .nodes
            .get(id)
            .and_then(|node| node.parent.as_ref())
            .is_some_and(|parent| removed.contains(parent));
        if !parent_removed {
            patches.push(DocumentPatch::RemoveNode { id: id.clone() });
        }
    }

    let mut order = Vec::new();
    collect_preorder(next, &next.root, &mut order);
    for id in &order {
        if previous.nodes.contains_key(id) {
            continue;
        }
        let Some(mut node) = next.nodes.get(id).cloned() else {
            continue;
        };
        node.children.clear();
        patches.push(DocumentPatch::UpsertNode(node));
    }

    for id in &order {
        let (Some(previous_node), Some(next_node)) = (previous.nodes.get(id), next.nodes.get(id))
        else {
            continue;
        };
        if previous_node.kind != next_node.kind
            || previous_node.parent != next_node.parent
            || previous_node.materialized != next_node.materialized
            || previous_node.map_viewport != next_node.map_viewport
        {
            patches.push(DocumentPatch::UpsertNode(next_node.clone()));
            continue;
        }
        if previous_node.text != next_node.text {
            if let Some(text) = next_node.text.clone() {
                patches.push(DocumentPatch::SetText {
                    id: id.clone(),
                    text,
                });
            } else {
                patches.push(DocumentPatch::UpsertNode(next_node.clone()));
                continue;
            }
        }
        if previous_node.embedded_program != next_node.embedded_program
            && let Some(program) = next_node.embedded_program.clone()
        {
            patches.push(DocumentPatch::SetEmbeddedProgram {
                id: id.clone(),
                program,
            });
        }
        let style = diff_style(&previous_node.style, &next_node.style);
        if !style.is_empty() {
            patches.push(DocumentPatch::SetStyle {
                id: id.clone(),
                patch: style,
            });
        }
        if previous_node.source_bindings.len() > next_node.source_bindings.len() {
            patches.push(DocumentPatch::UpsertNode(next_node.clone()));
            continue;
        }
        for (ordinal, binding) in next_node.source_bindings.iter().enumerate() {
            if previous_node.source_bindings.get(ordinal) != Some(binding) {
                patches.push(DocumentPatch::SetBindingAt {
                    id: id.clone(),
                    ordinal: ordinal as u32,
                    binding: binding.clone(),
                });
            }
        }
        if previous_node.text_input_id != next_node.text_input_id
            || previous_node.activation_focus != next_node.activation_focus
        {
            patches.push(DocumentPatch::SetTextInputFocus {
                id: id.clone(),
                text_input_id: next_node.text_input_id.clone(),
                activation_focus: next_node.activation_focus.clone(),
            });
        }
        if previous_node.scroll != next_node.scroll
            && let Some(scroll) = next_node.scroll
        {
            patches.push(DocumentPatch::SetScroll {
                id: id.clone(),
                scroll,
            });
        }
    }

    for id in order {
        let Some(next_node) = next.nodes.get(&id) else {
            continue;
        };
        let previous_children = previous
            .nodes
            .get(&id)
            .map(|node| node.children.as_slice())
            .unwrap_or(&[]);
        for (index, child) in next_node.children.iter().enumerate() {
            if previous_children.get(index) != Some(child) {
                patches.push(DocumentPatch::MoveChild {
                    child: child.clone(),
                    new_parent: id.clone(),
                    index,
                });
            }
        }
    }
    patches
}

fn diff_style(previous: &StyleMap, next: &StyleMap) -> StylePatch {
    previous
        .keys()
        .chain(next.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|name| previous.get(*name) != next.get(*name))
        .map(|name| (name.clone(), next.get(name).cloned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_tagged_values_keep_their_tag_and_payload_in_documents() {
        let value = Value::Record(BTreeMap::from([
            ("$tag".to_owned(), Value::Text("Found".to_owned())),
            ("value".to_owned(), Value::Text("current".to_owned())),
        ]));
        let expected = EvalValue::Tagged(
            "Found".to_owned(),
            BTreeMap::from([("value".to_owned(), EvalValue::Text("current".to_owned()))]),
        );

        assert_eq!(machine_value_to_eval(value.clone()), expected);
        assert_eq!(guard_value(&expected), Some(value));
    }

    #[test]
    fn scoped_materialization_identity_metadata_is_constant_space_and_rotates_on_shrink() {
        let owner = ScopedMaterializationOwner {
            materialization: DocumentMaterializationId(7),
            parent_instance: vec!["root".to_owned()],
        };
        let mut identities = ScopedMaterializationIdentities::default();

        let initial = identities.reconcile(owner.clone(), 1_000_000).unwrap();
        let grown = identities.reconcile(owner.clone(), 2_000_000).unwrap();
        assert_eq!(initial, grown);
        assert_eq!(identities.partitions.len(), 1);
        assert_eq!(identities.partitions[&owner].logical_len, 2_000_000);

        let shrunk = identities.reconcile(owner.clone(), 10).unwrap();
        assert_ne!(shrunk, initial);
        let regrown = identities.reconcile(owner.clone(), 1_000_000).unwrap();
        assert_eq!(regrown, shrunk);
        assert_eq!(identities.partitions.len(), 1);
    }

    fn map_descriptor(generation: u64, longitude: f64) -> MapViewportDescriptor {
        MapViewportDescriptor {
            generation: MapViewportGeneration(generation),
            camera: MapCamera {
                longitude,
                latitude: 0.0,
                zoom: 2.0,
                bearing: 0.0,
            },
            bounds: MapViewportBounds {
                width: 512.0,
                height: 320.0,
                scale: 1.0,
            },
            tile_source: MapTileSourceRef {
                id: MapTileSourceId("runtime-fixture".to_owned()),
                url_template_capability: "fixture_xyz".to_owned(),
                min_zoom: 0,
                max_zoom: 6,
                tile_size: 256,
                attribution: "Runtime fixture".to_owned(),
                allowed_origins: vec!["boon-local://runtime-map".to_owned()],
            },
            interaction: MapInteractionPolicy::default(),
            overlays: Vec::new(),
        }
    }

    fn eval_record<const N: usize>(fields: [(&str, EvalValue); N]) -> EvalValue {
        EvalValue::Record(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        )
    }

    fn eval_position(longitude: f64, latitude: f64) -> EvalValue {
        eval_record([
            ("longitude", EvalValue::Number(longitude)),
            ("latitude", EvalValue::Number(latitude)),
        ])
    }

    fn eval_overlay(
        kind: &str,
        id: &str,
        geometry: impl IntoIterator<Item = (&'static str, EvalValue)>,
    ) -> EvalValue {
        let mut fields = BTreeMap::from([
            ("id".to_owned(), EvalValue::Text(id.to_owned())),
            (
                "hit_identity".to_owned(),
                EvalValue::Text(format!("hit-{id}")),
            ),
            ("z_order".to_owned(), EvalValue::Number(3.0)),
            ("selected".to_owned(), EvalValue::Bool(false)),
        ]);
        fields.extend(
            geometry
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value)),
        );
        EvalValue::Tagged(kind.to_owned(), fields)
    }

    #[test]
    fn descriptor_evaluation_decodes_every_generic_overlay_kind() {
        let overlays = EvalValue::List(vec![
            eval_overlay(
                "Point",
                "point",
                [
                    ("position", eval_position(-1.0, 1.0)),
                    ("radius", EvalValue::Number(7.0)),
                    ("symbol_ref", EvalValue::Text("circle".to_owned())),
                    (
                        "paint",
                        eval_record([
                            ("fill", EvalValue::Text("#267a66".to_owned())),
                            ("opacity", EvalValue::Number(0.8)),
                        ]),
                    ),
                ],
            ),
            eval_overlay(
                "Cluster",
                "cluster",
                [
                    ("position", eval_position(1.0, 1.0)),
                    ("count", EvalValue::Number(12.0)),
                    ("radius", EvalValue::Number(15.0)),
                ],
            ),
            eval_overlay(
                "Polyline",
                "line",
                [(
                    "points",
                    EvalValue::List(vec![eval_position(-2.0, -1.0), eval_position(2.0, 1.0)]),
                )],
            ),
            eval_overlay(
                "Polygon",
                "polygon",
                [(
                    "rings",
                    EvalValue::List(vec![EvalValue::List(vec![
                        eval_position(-3.0, -2.0),
                        eval_position(3.0, -2.0),
                        eval_position(0.0, 3.0),
                    ])]),
                )],
            ),
            eval_overlay(
                "Label",
                "label",
                [
                    ("position", eval_position(0.0, 0.0)),
                    ("text", EvalValue::Text("Origin".to_owned())),
                    ("collision_priority", EvalValue::Number(100.0)),
                    ("font_size", EvalValue::Number(14.0)),
                ],
            ),
        ]);
        let descriptor = evaluate_map_viewport_descriptor(vec![
            (DocumentArgumentRole::MapGeneration, EvalValue::Number(9.0)),
            (
                DocumentArgumentRole::MapCamera,
                eval_record([
                    ("longitude", EvalValue::Number(0.0)),
                    ("latitude", EvalValue::Number(0.0)),
                    ("zoom", EvalValue::Number(2.5)),
                    ("bearing", EvalValue::Number(15.0)),
                ]),
            ),
            (
                DocumentArgumentRole::MapBounds,
                eval_record([
                    ("width", EvalValue::Number(720.0)),
                    ("height", EvalValue::Number(480.0)),
                    ("scale", EvalValue::Number(1.25)),
                ]),
            ),
            (
                DocumentArgumentRole::MapTileSource,
                eval_record([
                    ("id", EvalValue::Text("fixture".to_owned())),
                    (
                        "url_template_capability",
                        EvalValue::Text("fixture_xyz".to_owned()),
                    ),
                    ("min_zoom", EvalValue::Number(0.0)),
                    ("max_zoom", EvalValue::Number(6.0)),
                    ("tile_size", EvalValue::Number(256.0)),
                    ("attribution", EvalValue::Text("Fixture tiles".to_owned())),
                    (
                        "allowed_origins",
                        EvalValue::List(vec![EvalValue::Text("boon-local://fixture".to_owned())]),
                    ),
                ]),
            ),
            (DocumentArgumentRole::MapOverlays, overlays),
            (
                DocumentArgumentRole::MapInteraction,
                eval_record([
                    ("pan", EvalValue::Bool(true)),
                    ("wheel_zoom", EvalValue::Bool(true)),
                    ("pinch_zoom", EvalValue::Bool(true)),
                    ("keyboard_zoom", EvalValue::Bool(true)),
                ]),
            ),
        ])
        .unwrap();

        assert_eq!(descriptor.generation, MapViewportGeneration(9));
        assert!(matches!(
            descriptor.overlays[0].geometry,
            MapOverlayGeometry::Point { .. }
        ));
        assert!(matches!(
            descriptor.overlays[1].geometry,
            MapOverlayGeometry::Cluster { .. }
        ));
        assert!(matches!(
            descriptor.overlays[2].geometry,
            MapOverlayGeometry::Polyline { .. }
        ));
        assert!(matches!(
            descriptor.overlays[3].geometry,
            MapOverlayGeometry::Polygon { .. }
        ));
        assert!(matches!(
            descriptor.overlays[4].geometry,
            MapOverlayGeometry::Label { .. }
        ));
    }

    #[test]
    fn frame_diff_patches_map_descriptor_with_the_same_node_identity() {
        let mut previous = DocumentFrame::empty("root");
        let mut map = DocumentNode::new("map", DocumentNodeKind::MapViewport);
        map.parent = Some(previous.root.clone());
        map.map_viewport = Some(Box::new(map_descriptor(3, 0.0)));
        previous
            .nodes
            .get_mut(&previous.root)
            .unwrap()
            .children
            .push(map.id.clone());
        previous.nodes.insert(map.id.clone(), map);
        let mut next = previous.clone();
        next.nodes
            .get_mut(&FrameNodeId("map".to_owned()))
            .unwrap()
            .map_viewport = Some(Box::new(map_descriptor(4, 1.0)));

        assert!(matches!(
            diff_frames(&previous, &next).as_slice(),
            [DocumentPatch::UpsertNode(node)]
                if node.id.0 == "map"
                    && node.map_viewport.as_ref().is_some_and(|descriptor|
                        descriptor.generation == MapViewportGeneration(4)
                            && descriptor.camera.longitude == 1.0)
        ));
    }

    #[test]
    fn frame_diff_emits_embedded_program_descriptor_changes() {
        let mut previous = DocumentFrame::empty("root");
        let mut program = DocumentNode::new("program", DocumentNodeKind::EmbeddedProgram);
        program.parent = Some(previous.root.clone());
        program.embedded_program = Some(EmbeddedProgramDescriptor {
            source: "first".to_owned(),
            source_digest: crate::sha256_bytes(b"first"),
            revision: 1,
            ..EmbeddedProgramDescriptor::default()
        });
        previous
            .nodes
            .get_mut(&previous.root)
            .unwrap()
            .children
            .push(program.id.clone());
        previous.nodes.insert(program.id.clone(), program);
        let mut next = previous.clone();
        let descriptor = next
            .nodes
            .get_mut(&FrameNodeId("program".to_owned()))
            .unwrap()
            .embedded_program
            .as_mut()
            .unwrap();
        descriptor.source = "second".to_owned();
        descriptor.source_digest = crate::sha256_bytes(b"second");
        descriptor.revision = 2;

        assert!(matches!(
            diff_frames(&previous, &next).as_slice(),
            [DocumentPatch::SetEmbeddedProgram { id, program }]
                if id.0 == "program" && program.revision == 2
        ));
    }

    #[test]
    fn value_arguments_lower_typed_text_input_focus_without_style_metadata() {
        let mut input = DocumentNode::new("input", DocumentNodeKind::TextInput);
        apply_value_argument(
            &mut input,
            "input_id",
            EvalValue::Text("profile-source".to_owned()),
        );
        assert_eq!(
            input.text_input_id,
            Some(TextInputId("profile-source".to_owned()))
        );
        assert!(!input.style.contains_key("input_id"));

        let mut diagnostic = DocumentNode::new("diagnostic", DocumentNodeKind::Button);
        apply_value_argument(
            &mut diagnostic,
            "activate_focus",
            EvalValue::Record(BTreeMap::from([
                (
                    "input_id".to_owned(),
                    EvalValue::Text("profile-source".to_owned()),
                ),
                ("line".to_owned(), EvalValue::Number(7.0)),
                ("column".to_owned(), EvalValue::Text("3".to_owned())),
            ])),
        );
        assert_eq!(
            diagnostic.activation_focus,
            Some(TextInputFocusRequest {
                input_id: TextInputId("profile-source".to_owned()),
                line: 7,
                column: 3,
            })
        );
        assert!(!diagnostic.style.contains_key("activate_focus"));

        for invalid_line in [EvalValue::Number(0.0), EvalValue::Number(1.5)] {
            let mut invalid = DocumentNode::new("invalid", DocumentNodeKind::Button);
            apply_value_argument(
                &mut invalid,
                "activate_focus",
                EvalValue::Record(BTreeMap::from([
                    (
                        "input_id".to_owned(),
                        EvalValue::Text("profile-source".to_owned()),
                    ),
                    ("line".to_owned(), invalid_line),
                    ("column".to_owned(), EvalValue::Number(1.0)),
                ])),
            );
            assert_eq!(invalid.activation_focus, None);
        }
    }

    #[test]
    fn frame_diff_emits_nonstructural_text_input_focus_changes() {
        let mut previous = DocumentFrame::empty("root");
        let mut diagnostic = DocumentNode::new("diagnostic", DocumentNodeKind::Button);
        diagnostic.parent = Some(previous.root.clone());
        previous
            .nodes
            .get_mut(&previous.root)
            .unwrap()
            .children
            .push(diagnostic.id.clone());
        previous.nodes.insert(diagnostic.id.clone(), diagnostic);
        let mut next = previous.clone();
        next.nodes
            .get_mut(&FrameNodeId("diagnostic".to_owned()))
            .unwrap()
            .activation_focus = Some(TextInputFocusRequest {
            input_id: TextInputId("profile-source".to_owned()),
            line: 9,
            column: 4,
        });

        assert!(matches!(
            diff_frames(&previous, &next).as_slice(),
            [DocumentPatch::SetTextInputFocus {
                id,
                text_input_id: None,
                activation_focus: Some(request),
            }] if id.0 == "diagnostic"
                && request.input_id.0 == "profile-source"
                && request.line == 9
                && request.column == 4
        ));
    }

    #[test]
    fn text_concat_uses_separator_between_values_only() {
        assert_eq!(
            eval_builtin(
                DocumentBuiltin::TextConcat,
                Some(EvalValue::Number(3.0)),
                vec![
                    ("with".to_owned(), EvalValue::Text("items left".to_owned())),
                    ("separator".to_owned(), EvalValue::Text(" ".to_owned())),
                ],
            ),
            EvalValue::Text("3 items left".to_owned())
        );
    }

    #[test]
    fn time_range_label_formats_both_endpoints_with_the_unit() {
        assert_eq!(
            eval_builtin(
                DocumentBuiltin::TextTimeRangeLabel,
                Some(EvalValue::Number(0.0)),
                vec![
                    ("end".to_owned(), EvalValue::Number(240.0)),
                    ("unit".to_owned(), EvalValue::Text("ns".to_owned())),
                ],
            ),
            EvalValue::Text("0 ns - 240 ns".to_owned())
        );
    }

    #[test]
    fn number_to_ascii_text_respects_signal_bit_width() {
        let ascii = |value, width| {
            eval_builtin(
                DocumentBuiltin::NumberToAsciiText,
                Some(EvalValue::Number(value)),
                vec![("width".to_owned(), EvalValue::Number(width))],
            )
        };
        assert_eq!(ascii(0x48 as f64, 8.0), EvalValue::Text("H".to_owned()));
        assert_eq!(ascii(0x4845 as f64, 16.0), EvalValue::Text("HE".to_owned()));
        assert_eq!(ascii(0.0, 7.0), EvalValue::Text("-".to_owned()));
        assert_eq!(ascii(0.0, 8.0), EvalValue::Text("?".to_owned()));
        assert_eq!(ascii(1.0, 8.0), EvalValue::Text("?".to_owned()));
    }

    #[test]
    fn structured_dimensions_lower_to_layout_constraints() {
        let mut style = StyleMap::new();
        lower_dimension(
            "width",
            &EvalValue::Record(BTreeMap::from([
                ("sizing".to_owned(), EvalValue::Enum("Fill".to_owned())),
                ("minimum".to_owned(), EvalValue::Number(230.0)),
                ("maximum".to_owned(), EvalValue::Number(552.0)),
            ])),
            &mut style,
        );

        assert_eq!(
            style.get("width"),
            Some(&StyleValue::Text("Fill".to_owned()))
        );
        assert_eq!(style.get("min_width"), Some(&StyleValue::Number(230.0)));
        assert_eq!(style.get("max_width"), Some(&StyleValue::Number(552.0)));
    }
}
