use super::{
    ActivationBatch, DurableOutboxState, OutboxItemId, RestoreImage, StoredList, StoredRow,
    StoredScalar, StoredValue, validate_outbox_item_schema,
};
use boon_data::{FiniteReal, NumberTextFormat, format_number_text};
use boon_plan::{
    ApplicationIdentity, DataTypePlan, MachinePlan, MemoryId, MemoryLeafId,
    MigrationArgumentValuePlan, MigrationEdgeId, MigrationEdgePlan, MigrationExpressionPlan,
    MigrationInputId, MigrationInputPlan, MigrationRecipePlan, MigrationTransferKindPlan,
    MigrationTransferPlan, MigrationTransformPlan, PersistencePlan,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationStepPreview {
    pub edge_id: MigrationEdgeId,
    pub source_schema_version: u64,
    pub target_schema_version: u64,
    pub transfer_count: usize,
    pub transformed_row_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationPreview {
    pub source_schema_version: u64,
    pub source_schema_hash: [u8; 32],
    pub target_schema_version: u64,
    pub target_schema_hash: [u8; 32],
    pub steps: Vec<MigrationStepPreview>,
    pub scalar_count_before: usize,
    pub scalar_count_after: usize,
    pub list_count_before: usize,
    pub list_count_after: usize,
    pub row_count_before: usize,
    pub row_count_after: usize,
    pub deleted_memory: Vec<MemoryId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedMigration {
    pub candidate: RestoreImage,
    pub activation: ActivationBatch,
    pub preview: MigrationPreview,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MigrationError {
    ApplicationIdentityMismatch,
    TargetOlderThanStored {
        stored: u64,
        target: u64,
    },
    UnsupportedSource {
        version: u64,
        schema_hash: [u8; 32],
    },
    AmbiguousChain {
        version: u64,
        schema_hash: [u8; 32],
    },
    MissingRecipe(MigrationEdgeId),
    RepeatedEdge(MigrationEdgeId),
    MissingAuthority {
        memory_id: MemoryId,
        leaf_id: MemoryLeafId,
    },
    InvalidTransfer(String),
    Evaluation(String),
    OutputTypeMismatch(String),
    InvalidCandidate(String),
    IncompatibleOutbox {
        item_id: OutboxItemId,
        detail: String,
    },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApplicationIdentityMismatch => {
                formatter.write_str("stored and target application identities differ")
            }
            Self::TargetOlderThanStored { stored, target } => write!(
                formatter,
                "stored schema version {stored} is newer than target version {target}"
            ),
            Self::UnsupportedSource {
                version,
                schema_hash,
            } => write!(
                formatter,
                "no migration chain supports schema version {version} with hash {}",
                digest_hex(schema_hash)
            ),
            Self::AmbiguousChain {
                version,
                schema_hash,
            } => write!(
                formatter,
                "multiple migration chains support schema version {version} with hash {}",
                digest_hex(schema_hash)
            ),
            Self::MissingRecipe(edge) => {
                write!(
                    formatter,
                    "migration edge {edge} references a missing recipe"
                )
            }
            Self::RepeatedEdge(edge) => {
                write!(formatter, "migration edge {edge} was already completed")
            }
            Self::MissingAuthority { memory_id, leaf_id } => write!(
                formatter,
                "migration input {memory_id}/{leaf_id} has no stored authority"
            ),
            Self::InvalidTransfer(detail) => {
                write!(formatter, "invalid migration transfer: {detail}")
            }
            Self::Evaluation(detail) => write!(formatter, "migration evaluation failed: {detail}"),
            Self::OutputTypeMismatch(detail) => {
                write!(formatter, "migration output type mismatch: {detail}")
            }
            Self::InvalidCandidate(detail) => {
                write!(formatter, "staged migration candidate is invalid: {detail}")
            }
            Self::IncompatibleOutbox { item_id, detail } => {
                write!(
                    formatter,
                    "pending outbox item {item_id} blocks migration: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for MigrationError {}

pub fn stage_migration(
    stored: &RestoreImage,
    target: &MachinePlan,
) -> Result<StagedMigration, MigrationError> {
    stage_migration_plan(stored, &target.application.identity, &target.persistence)
}

pub fn stage_migration_plan(
    stored: &RestoreImage,
    target_application: &ApplicationIdentity,
    persistence: &PersistencePlan,
) -> Result<StagedMigration, MigrationError> {
    if &stored.application != target_application {
        return Err(MigrationError::ApplicationIdentityMismatch);
    }
    if stored.schema_version > persistence.schema_version {
        return Err(MigrationError::TargetOlderThanStored {
            stored: stored.schema_version,
            target: persistence.schema_version,
        });
    }

    let chain = migration_chain(stored, persistence)?;
    let scalar_count_before = stored.scalars.len();
    let list_count_before = stored.lists.len();
    let row_count_before = row_count(stored);
    let mut candidate = stored.clone();
    let mut steps = Vec::with_capacity(chain.len());

    for (index, edge) in chain.iter().enumerate() {
        if candidate
            .completed_migration_edges
            .contains(&edge.migration_edge_id)
        {
            return Err(MigrationError::RepeatedEdge(edge.migration_edge_id));
        }
        let recipe = persistence
            .migration_recipes
            .iter()
            .find(|recipe| recipe.migration_recipe_id == edge.migration_recipe_id)
            .ok_or(MigrationError::MissingRecipe(edge.migration_edge_id))?;
        let snapshot = candidate.clone();
        let transformed_row_count = apply_recipe(&snapshot, &mut candidate, recipe)?;
        candidate
            .completed_migration_edges
            .insert(edge.migration_edge_id);
        candidate.schema_version = edge.target_schema_version;
        candidate.schema_hash = chain
            .get(index + 1)
            .map_or(persistence.schema_hash, |next| next.source_schema_hash);
        steps.push(MigrationStepPreview {
            edge_id: edge.migration_edge_id,
            source_schema_version: edge.source_schema_version,
            target_schema_version: edge.target_schema_version,
            transfer_count: recipe.transfers.len(),
            transformed_row_count,
        });
    }

    candidate.outbox.retain(|_, item| {
        persistence
            .effect_outbox
            .iter()
            .any(|schema| schema.effect_id == item.effect_id)
    });
    for (item_id, item) in &stored.outbox {
        let schema = persistence
            .effect_outbox
            .iter()
            .find(|schema| schema.effect_id == item.effect_id);
        if schema.is_none() && !matches!(item.state, DurableOutboxState::Completed { .. }) {
            return Err(MigrationError::IncompatibleOutbox {
                item_id: *item_id,
                detail: "target schema removes the effect while work is unfinished".to_owned(),
            });
        }
        if let Some(schema) = schema
            && let Err(error) = validate_outbox_item_schema(item, schema)
        {
            return Err(MigrationError::IncompatibleOutbox {
                item_id: *item_id,
                detail: error.to_string(),
            });
        }
    }

    let target_scalars = persistence
        .memory
        .iter()
        .map(|memory| memory.memory_id)
        .collect::<BTreeSet<_>>();
    let target_lists = persistence
        .lists
        .iter()
        .map(|list| list.memory_id)
        .collect::<BTreeSet<_>>();
    let mut deleted_memory = candidate
        .scalars
        .keys()
        .filter(|memory| !target_scalars.contains(memory))
        .chain(
            candidate
                .lists
                .keys()
                .filter(|memory| !target_lists.contains(memory)),
        )
        .copied()
        .collect::<Vec<_>>();
    deleted_memory.sort();
    deleted_memory.dedup();
    candidate
        .scalars
        .retain(|memory, _| target_scalars.contains(memory));
    candidate
        .lists
        .retain(|memory, _| target_lists.contains(memory));
    candidate.schema_version = persistence.schema_version;
    candidate.schema_hash = persistence.schema_hash;

    let activation = ActivationBatch::between(stored, &candidate)
        .map_err(|error| MigrationError::InvalidCandidate(error.to_string()))?;
    let preview = MigrationPreview {
        source_schema_version: stored.schema_version,
        source_schema_hash: stored.schema_hash,
        target_schema_version: candidate.schema_version,
        target_schema_hash: candidate.schema_hash,
        steps,
        scalar_count_before,
        scalar_count_after: candidate.scalars.len(),
        list_count_before,
        list_count_after: candidate.lists.len(),
        row_count_before,
        row_count_after: row_count(&candidate),
        deleted_memory,
    };
    Ok(StagedMigration {
        candidate,
        activation,
        preview,
    })
}

fn migration_chain<'a>(
    stored: &RestoreImage,
    persistence: &'a PersistencePlan,
) -> Result<Vec<&'a MigrationEdgePlan>, MigrationError> {
    if stored.schema_version == persistence.schema_version {
        return if stored.schema_hash == persistence.schema_hash {
            Ok(Vec::new())
        } else {
            Err(MigrationError::UnsupportedSource {
                version: stored.schema_version,
                schema_hash: stored.schema_hash,
            })
        };
    }

    let mut paths = Vec::new();
    collect_migration_paths(
        &persistence.migration_edges,
        stored.schema_version,
        stored.schema_hash,
        persistence.schema_version,
        persistence.schema_hash,
        &mut Vec::new(),
        &mut paths,
    );
    match paths.len() {
        0 => Err(MigrationError::UnsupportedSource {
            version: stored.schema_version,
            schema_hash: stored.schema_hash,
        }),
        1 => Ok(paths.pop().expect("one migration path exists")),
        _ => Err(MigrationError::AmbiguousChain {
            version: stored.schema_version,
            schema_hash: stored.schema_hash,
        }),
    }
}

fn collect_migration_paths<'a>(
    edges: &'a [MigrationEdgePlan],
    version: u64,
    schema_hash: [u8; 32],
    target_version: u64,
    target_hash: [u8; 32],
    current: &mut Vec<&'a MigrationEdgePlan>,
    paths: &mut Vec<Vec<&'a MigrationEdgePlan>>,
) {
    if paths.len() > 1 {
        return;
    }
    if version == target_version {
        if schema_hash == target_hash {
            paths.push(current.clone());
        }
        return;
    }
    for edge in edges.iter().filter(|edge| {
        edge.source_schema_version == version
            && edge.source_schema_hash == schema_hash
            && edge.target_schema_version <= target_version
    }) {
        current.push(edge);
        if edge.target_schema_version == target_version {
            collect_migration_paths(
                edges,
                target_version,
                target_hash,
                target_version,
                target_hash,
                current,
                paths,
            );
        } else {
            let hashes = edges
                .iter()
                .filter(|next| next.source_schema_version == edge.target_schema_version)
                .map(|next| next.source_schema_hash)
                .collect::<BTreeSet<_>>();
            for next_hash in hashes {
                collect_migration_paths(
                    edges,
                    edge.target_schema_version,
                    next_hash,
                    target_version,
                    target_hash,
                    current,
                    paths,
                );
            }
        }
        current.pop();
    }
}

fn apply_recipe(
    snapshot: &RestoreImage,
    candidate: &mut RestoreImage,
    recipe: &MigrationRecipePlan,
) -> Result<usize, MigrationError> {
    let mut consumed_scalars = BTreeSet::new();
    let mut scalar_destinations = BTreeMap::<MemoryId, usize>::new();
    let mut consumed_list_fields = BTreeMap::<MemoryId, BTreeSet<MemoryLeafId>>::new();
    let mut list_moves = Vec::new();
    let mut transformed_rows = 0;

    for transfer in &recipe.transfers {
        if transfer.transfer_kind == MigrationTransferKindPlan::Scalar {
            *scalar_destinations
                .entry(transfer.destination.memory_id)
                .or_default() += 1;
        }
    }

    for transfer in &recipe.transfers {
        match transfer.transfer_kind {
            MigrationTransferKindPlan::Scalar => {
                let Some(inputs) = scalar_inputs(snapshot, &transfer.inputs)? else {
                    continue;
                };
                let value = evaluate_transform(&transfer.transform, &inputs)?;
                ensure_value_type(&value, &transfer.destination.data_type)?;
                let merge_record = scalar_destinations
                    .get(&transfer.destination.memory_id)
                    .copied()
                    .unwrap_or_default()
                    > 1;
                write_scalar_destination(candidate, transfer, value, merge_record)?;
                for input in &transfer.inputs {
                    for leaf in &input.leaves {
                        consumed_scalars.insert(leaf.memory_id);
                    }
                }
            }
            MigrationTransferKindPlan::List => {
                if !matches!(transfer.transform, MigrationTransformPlan::Identity { .. })
                    || transfer.inputs.len() != 1
                    || transfer.inputs[0].leaves.len() != 1
                {
                    return Err(MigrationError::InvalidTransfer(
                        "whole-list migration must be one identity transfer".to_owned(),
                    ));
                }
                let source = transfer.inputs[0].leaves[0].memory_id;
                if let Some(list) = snapshot.lists.get(&source) {
                    let field_ids = transfer
                        .list_row_fields
                        .iter()
                        .map(|field| {
                            (
                                field.source.leaf_id,
                                field
                                    .destination
                                    .as_ref()
                                    .map(|destination| destination.leaf_id),
                            )
                        })
                        .collect::<BTreeMap<_, _>>();
                    let mut migrated = list.clone();
                    for row in &mut migrated.rows {
                        let mut fields = BTreeMap::new();
                        for (source_field, value) in std::mem::take(&mut row.fields) {
                            let destination_field = field_ids.get(&source_field).ok_or_else(|| {
                                MigrationError::InvalidTransfer(format!(
                                    "whole-list migration has no destination for stored row leaf {source_field}"
                                ))
                            })?;
                            if let Some(destination_field) = destination_field {
                                fields.insert(*destination_field, value);
                            }
                        }
                        let mut touched_fields = BTreeSet::new();
                        for source_field in std::mem::take(&mut row.touched_fields) {
                            let destination_field = field_ids.get(&source_field).ok_or_else(|| {
                                MigrationError::InvalidTransfer(format!(
                                    "whole-list migration has no destination for touched row leaf {source_field}"
                                ))
                            })?;
                            if let Some(destination_field) = destination_field {
                                touched_fields.insert(*destination_field);
                            }
                        }
                        row.fields = fields;
                        row.touched_fields = touched_fields;
                    }
                    candidate
                        .lists
                        .insert(transfer.destination.memory_id, migrated);
                }
                list_moves.push((source, transfer.destination.memory_id));
            }
            MigrationTransferKindPlan::IndexedRowField => {
                transformed_rows += apply_indexed_transfer(snapshot, candidate, transfer)?;
                let owner = transfer
                    .indexed_list_owner
                    .as_ref()
                    .ok_or_else(|| {
                        MigrationError::InvalidTransfer(
                            "indexed transfer has no stable list owner".to_owned(),
                        )
                    })?
                    .memory_id;
                for input in &transfer.inputs {
                    for leaf in &input.leaves {
                        consumed_list_fields
                            .entry(owner)
                            .or_default()
                            .insert(leaf.leaf_id);
                    }
                }
            }
        }
    }

    let destination_scalars = scalar_destinations.keys().copied().collect::<BTreeSet<_>>();
    for source in consumed_scalars {
        if !destination_scalars.contains(&source) {
            candidate.scalars.remove(&source);
        }
    }
    for (source, destination) in list_moves {
        if source != destination {
            candidate.lists.remove(&source);
        }
    }
    for (memory_id, fields) in consumed_list_fields {
        let destination_fields = recipe
            .transfers
            .iter()
            .filter(|transfer| {
                transfer.transfer_kind == MigrationTransferKindPlan::IndexedRowField
                    && transfer.destination.memory_id == memory_id
            })
            .map(|transfer| transfer.destination.leaf_id)
            .collect::<BTreeSet<_>>();
        if let Some(list) = candidate.lists.get_mut(&memory_id) {
            for row in &mut list.rows {
                for field in fields.difference(&destination_fields) {
                    row.fields.remove(field);
                    row.touched_fields.remove(field);
                }
            }
            if !list.touched {
                list.rows.retain(|row| !row.touched_fields.is_empty());
            }
        }
    }
    Ok(transformed_rows)
}

fn scalar_inputs(
    snapshot: &RestoreImage,
    inputs: &[MigrationInputPlan],
) -> Result<Option<BTreeMap<MigrationInputId, StoredValue>>, MigrationError> {
    let mut values = BTreeMap::new();
    for input in inputs {
        let mut leaves = BTreeMap::new();
        for leaf in &input.leaves {
            let Some(scalar) = snapshot.scalars.get(&leaf.memory_id) else {
                return Ok(None);
            };
            let value = if input.leaves.len() == 1 {
                scalar.value.clone()
            } else {
                project_stored_field(
                    &scalar.value,
                    leaf.semantic_path.rsplit('.').next().unwrap_or(""),
                )?
            };
            leaves.insert(
                leaf.semantic_path
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_owned(),
                value,
            );
        }
        let value = if input.leaves.len() == 1 {
            leaves.into_values().next().expect("one input leaf exists")
        } else {
            StoredValue::Record(leaves)
        };
        values.insert(input.input_id, value);
    }
    Ok(Some(values))
}

fn write_scalar_destination(
    candidate: &mut RestoreImage,
    transfer: &MigrationTransferPlan,
    value: StoredValue,
    merge_record: bool,
) -> Result<(), MigrationError> {
    if merge_record {
        let field = transfer
            .destination
            .semantic_path
            .rsplit('.')
            .next()
            .filter(|field| !field.is_empty())
            .ok_or_else(|| {
                MigrationError::InvalidTransfer("record destination has no field name".to_owned())
            })?;
        let scalar = candidate
            .scalars
            .entry(transfer.destination.memory_id)
            .or_insert_with(|| StoredScalar {
                touched: true,
                value: StoredValue::Record(BTreeMap::new()),
            });
        let StoredValue::Record(fields) = &mut scalar.value else {
            return Err(MigrationError::InvalidTransfer(
                "multiple destination leaves require record authority".to_owned(),
            ));
        };
        fields.insert(field.to_owned(), value);
        scalar.touched = true;
    } else {
        candidate.scalars.insert(
            transfer.destination.memory_id,
            StoredScalar {
                touched: true,
                value,
            },
        );
    }
    Ok(())
}

fn apply_indexed_transfer(
    snapshot: &RestoreImage,
    candidate: &mut RestoreImage,
    transfer: &MigrationTransferPlan,
) -> Result<usize, MigrationError> {
    if transfer
        .inputs
        .first()
        .and_then(|input| input.leaves.first())
        .is_none()
    {
        return Err(MigrationError::InvalidTransfer(
            "indexed transfer has no source leaf".to_owned(),
        ));
    }
    let owner = transfer
        .indexed_list_owner
        .as_ref()
        .ok_or_else(|| {
            MigrationError::InvalidTransfer(
                "indexed row-field migration has no stable list owner".to_owned(),
            )
        })?
        .memory_id;
    let Some(source_list) = snapshot.lists.get(&owner) else {
        return Ok(0);
    };
    candidate
        .lists
        .entry(owner)
        .or_insert_with(|| source_list.clone());
    let mut transformed = 0;
    for source_row in &source_list.rows {
        let mut inputs = BTreeMap::new();
        let mut complete = true;
        for input in &transfer.inputs {
            let mut fields = BTreeMap::new();
            for leaf in &input.leaves {
                let Some(value) = source_row.fields.get(&leaf.leaf_id) else {
                    complete = false;
                    break;
                };
                fields.insert(
                    leaf.semantic_path
                        .rsplit('.')
                        .next()
                        .unwrap_or("")
                        .to_owned(),
                    value.clone(),
                );
            }
            if !complete {
                break;
            }
            let value = if input.leaves.len() == 1 {
                fields.into_values().next().expect("one row input exists")
            } else {
                StoredValue::Record(fields)
            };
            inputs.insert(input.input_id, value);
        }
        if !complete {
            continue;
        }
        let value = evaluate_transform(&transfer.transform, &inputs)?;
        ensure_value_type(&value, &transfer.destination.data_type)?;
        let target_list = candidate
            .lists
            .get_mut(&owner)
            .expect("destination list was installed");
        let target_row = ensure_target_row(target_list, source_row)?;
        target_row
            .fields
            .insert(transfer.destination.leaf_id, value);
        target_row
            .touched_fields
            .insert(transfer.destination.leaf_id);
        transformed += 1;
    }
    Ok(transformed)
}

fn ensure_target_row<'a>(
    list: &'a mut StoredList,
    source: &StoredRow,
) -> Result<&'a mut StoredRow, MigrationError> {
    if let Some(index) = list
        .rows
        .iter()
        .position(|row| row.key == source.key && row.generation == source.generation)
    {
        return Ok(&mut list.rows[index]);
    }
    if list.touched {
        return Err(MigrationError::InvalidTransfer(format!(
            "materialized list has no destination row {}:{}",
            source.key, source.generation
        )));
    }
    list.rows.push(StoredRow {
        key: source.key,
        generation: source.generation,
        fields: BTreeMap::new(),
        touched_fields: BTreeSet::new(),
    });
    Ok(list.rows.last_mut().expect("row was appended"))
}

fn evaluate_transform(
    transform: &MigrationTransformPlan,
    inputs: &BTreeMap<MigrationInputId, StoredValue>,
) -> Result<StoredValue, MigrationError> {
    match transform {
        MigrationTransformPlan::Identity { input_id } => inputs
            .get(input_id)
            .cloned()
            .ok_or_else(|| MigrationError::Evaluation("identity input is missing".to_owned())),
        MigrationTransformPlan::Expression { root } => evaluate_expression(root, inputs, &[]),
    }
}

#[derive(Clone)]
enum EvaluatedArgument {
    Value(StoredValue),
    Lambda {
        parameter_count: u16,
        body: MigrationExpressionPlan,
    },
}

fn evaluate_expression(
    expression: &MigrationExpressionPlan,
    inputs: &BTreeMap<MigrationInputId, StoredValue>,
    parameters: &[StoredValue],
) -> Result<StoredValue, MigrationError> {
    match expression {
        MigrationExpressionPlan::Input { input_id } => inputs
            .get(input_id)
            .cloned()
            .ok_or_else(|| MigrationError::Evaluation("expression input is missing".to_owned())),
        MigrationExpressionPlan::Parameter { index } => parameters
            .get(usize::from(*index))
            .cloned()
            .ok_or_else(|| MigrationError::Evaluation("lambda parameter is missing".to_owned())),
        MigrationExpressionPlan::Text { value } => Ok(StoredValue::Text(value.clone())),
        MigrationExpressionPlan::Number { value } => Ok(StoredValue::Number(*value)),
        MigrationExpressionPlan::Bool { value } => Ok(StoredValue::Bool(*value)),
        MigrationExpressionPlan::Variant { tag } => Ok(StoredValue::Variant {
            tag: tag.clone(),
            fields: BTreeMap::new(),
        }),
        MigrationExpressionPlan::Tagged { tag, fields } => Ok(StoredValue::Variant {
            tag: tag.clone(),
            fields: evaluate_fields(fields, inputs, parameters)?,
        }),
        MigrationExpressionPlan::Project { input, fields } => {
            let mut value = evaluate_expression(input, inputs, parameters)?;
            for field in fields {
                value = project_stored_field(&value, field)?;
            }
            Ok(value)
        }
        MigrationExpressionPlan::Call {
            function,
            input,
            arguments,
        } => {
            let input = input
                .as_deref()
                .map(|value| evaluate_expression(value, inputs, parameters))
                .transpose()?;
            let mut evaluated = Vec::with_capacity(arguments.len());
            for argument in arguments {
                let value = match &argument.value {
                    MigrationArgumentValuePlan::Expression { value } => {
                        EvaluatedArgument::Value(evaluate_expression(value, inputs, parameters)?)
                    }
                    MigrationArgumentValuePlan::Lambda {
                        parameter_count,
                        body,
                    } => EvaluatedArgument::Lambda {
                        parameter_count: *parameter_count,
                        body: body.as_ref().clone(),
                    },
                };
                evaluated.push((argument.name.as_deref(), value));
            }
            evaluate_call(function, input, &evaluated, inputs)
        }
        MigrationExpressionPlan::Infix {
            operator,
            left,
            right,
        } => evaluate_infix(
            operator,
            evaluate_expression(left, inputs, parameters)?,
            evaluate_expression(right, inputs, parameters)?,
        ),
        MigrationExpressionPlan::Record { fields } => Ok(StoredValue::Record(evaluate_fields(
            fields, inputs, parameters,
        )?)),
        MigrationExpressionPlan::List { items } => Ok(StoredValue::List(
            items
                .iter()
                .map(|item| evaluate_expression(item, inputs, parameters))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        MigrationExpressionPlan::Bytes { items } => {
            let values = items
                .iter()
                .map(|item| evaluate_expression(item, inputs, parameters))
                .collect::<Result<Vec<_>, _>>()?;
            let bytes = values
                .into_iter()
                .map(|value| match value {
                    StoredValue::Number(value) => value
                        .to_i64_exact()
                        .ok()
                        .and_then(|value| u8::try_from(value).ok())
                        .ok_or_else(|| {
                            MigrationError::Evaluation("byte literal is out of range".to_owned())
                        }),
                    _ => Err(MigrationError::Evaluation(
                        "byte expression did not produce a number".to_owned(),
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(StoredValue::Bytes(bytes))
        }
        MigrationExpressionPlan::Match { input, arms } => {
            let value = evaluate_expression(input, inputs, parameters)?;
            let tag = stored_tag(&value);
            let arm = arms
                .iter()
                .find(|arm| {
                    arm.pattern
                        .iter()
                        .any(|pattern| pattern == "_" || pattern == tag)
                })
                .ok_or_else(|| {
                    MigrationError::Evaluation(format!("no migration match arm accepts `{tag}`"))
                })?;
            evaluate_expression(&arm.output, inputs, parameters)
        }
    }
}

fn evaluate_fields(
    fields: &[boon_plan::MigrationObjectFieldPlan],
    inputs: &BTreeMap<MigrationInputId, StoredValue>,
    parameters: &[StoredValue],
) -> Result<BTreeMap<String, StoredValue>, MigrationError> {
    fields
        .iter()
        .map(|field| {
            Ok((
                field.name.clone(),
                evaluate_expression(&field.value, inputs, parameters)?,
            ))
        })
        .collect()
}

fn stored_integer(value: i64) -> Result<StoredValue, MigrationError> {
    FiniteReal::from_i64_exact(value)
        .map(StoredValue::Number)
        .map_err(|error| MigrationError::Evaluation(error.to_string()))
}

fn stored_number_result(value: f64) -> Result<StoredValue, MigrationError> {
    FiniteReal::new(value)
        .map(StoredValue::Number)
        .map_err(|error| MigrationError::Evaluation(error.to_string()))
}

fn evaluate_call(
    function: &str,
    input: Option<StoredValue>,
    arguments: &[(Option<&str>, EvaluatedArgument)],
    inputs: &BTreeMap<MigrationInputId, StoredValue>,
) -> Result<StoredValue, MigrationError> {
    let first_value = || {
        arguments.iter().find_map(|(_, argument)| match argument {
            EvaluatedArgument::Value(value) => Some(value.clone()),
            EvaluatedArgument::Lambda { .. } => None,
        })
    };
    match function {
        "Bool/not" => match input.or_else(first_value) {
            Some(StoredValue::Bool(value)) => Ok(StoredValue::Bool(!value)),
            _ => Err(MigrationError::Evaluation(
                "Bool/not requires one boolean".to_owned(),
            )),
        },
        "Number/to_text" => {
            let named_value = |name: &str| {
                arguments.iter().find_map(|(candidate, argument)| {
                    (*candidate == Some(name)).then(|| match argument {
                        EvaluatedArgument::Value(value) => Some(value.clone()),
                        EvaluatedArgument::Lambda { .. } => None,
                    })?
                })
            };
            let positional_value = || {
                arguments.iter().find_map(|(name, argument)| {
                    name.is_none().then(|| match argument {
                        EvaluatedArgument::Value(value) => Some(value.clone()),
                        EvaluatedArgument::Lambda { .. } => None,
                    })?
                })
            };
            let Some(StoredValue::Number(value)) = input
                .or_else(|| named_value("value"))
                .or_else(positional_value)
            else {
                return Err(MigrationError::Evaluation(
                    "Number/to_text requires one number".to_owned(),
                ));
            };
            let integer_arg = |name: &str| -> Result<Option<i64>, MigrationError> {
                named_value(name)
                    .map(|value| match value {
                        StoredValue::Number(value) => value
                            .to_i64_exact()
                            .map_err(|error| MigrationError::Evaluation(error.to_string())),
                        _ => Err(MigrationError::Evaluation(format!(
                            "Number/to_text {name} must be a whole Number"
                        ))),
                    })
                    .transpose()
            };
            let prefix = match named_value("prefix") {
                Some(StoredValue::Bool(value)) => value,
                Some(_) => {
                    return Err(MigrationError::Evaluation(
                        "Number/to_text prefix must be Bool".to_owned(),
                    ));
                }
                None => false,
            };
            let radix = integer_arg("radix")?.unwrap_or(10);
            let min_width = integer_arg("min_width")?.unwrap_or(0);
            let signed_width =
                integer_arg("signed_width")?.map(|value| u32::try_from(value).unwrap_or_default());
            let group_size =
                integer_arg("group_size")?.map(|value| usize::try_from(value).unwrap_or_default());
            let text = format_number_text(
                value,
                NumberTextFormat {
                    radix: u32::try_from(radix).unwrap_or_default(),
                    min_width: usize::try_from(min_width).unwrap_or(usize::MAX),
                    signed_width,
                    group_size,
                    prefix,
                },
            )
            .map_err(|error| MigrationError::Evaluation(error.to_string()))?;
            Ok(StoredValue::Text(text))
        }
        "Text/to_number" => match input.or_else(first_value) {
            Some(StoredValue::Text(value)) => value
                .parse::<FiniteReal>()
                .map(StoredValue::Number)
                .map_err(|_| MigrationError::Evaluation("text is not a number".to_owned())),
            _ => Err(MigrationError::Evaluation(
                "Text/to_number requires one text value".to_owned(),
            )),
        },
        "Text/concat" => {
            let mut parts = Vec::new();
            if let Some(StoredValue::Text(value)) = input {
                parts.push(value);
            }
            for (_, argument) in arguments {
                if let EvaluatedArgument::Value(StoredValue::Text(value)) = argument {
                    parts.push(value.clone());
                }
            }
            Ok(StoredValue::Text(parts.concat()))
        }
        "Text/is_empty" => match input.or_else(first_value) {
            Some(StoredValue::Text(value)) => Ok(StoredValue::Bool(value.is_empty())),
            _ => Err(MigrationError::Evaluation(
                "Text/is_empty requires one text value".to_owned(),
            )),
        },
        "List/count" | "List/length" => match input.or_else(first_value) {
            Some(StoredValue::List(values)) => {
                stored_integer(i64::try_from(values.len()).map_err(|_| {
                    MigrationError::Evaluation("list length exceeds number range".to_owned())
                })?)
            }
            _ => Err(MigrationError::Evaluation(
                "List/count requires one list".to_owned(),
            )),
        },
        "List/map" => {
            let StoredValue::List(values) = input.or_else(first_value).ok_or_else(|| {
                MigrationError::Evaluation("List/map requires a list input".to_owned())
            })?
            else {
                return Err(MigrationError::Evaluation(
                    "List/map requires a list input".to_owned(),
                ));
            };
            let (parameter_count, body) = arguments
                .iter()
                .find_map(|(_, argument)| match argument {
                    EvaluatedArgument::Lambda {
                        parameter_count,
                        body,
                    } => Some((*parameter_count, body)),
                    EvaluatedArgument::Value(_) => None,
                })
                .ok_or_else(|| {
                    MigrationError::Evaluation("List/map requires a lambda".to_owned())
                })?;
            if parameter_count != 1 {
                return Err(MigrationError::Evaluation(
                    "List/map migration lambda must take one parameter".to_owned(),
                ));
            }
            Ok(StoredValue::List(
                values
                    .iter()
                    .map(|value| evaluate_expression(body, inputs, std::slice::from_ref(value)))
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        other => Err(MigrationError::Evaluation(format!(
            "unsupported target-neutral migration call `{other}`"
        ))),
    }
}

fn evaluate_infix(
    operator: &str,
    left: StoredValue,
    right: StoredValue,
) -> Result<StoredValue, MigrationError> {
    match (operator, left, right) {
        ("+", StoredValue::Number(left), StoredValue::Number(right)) => {
            stored_number_result(left.get() + right.get())
        }
        ("-", StoredValue::Number(left), StoredValue::Number(right)) => {
            stored_number_result(left.get() - right.get())
        }
        ("*", StoredValue::Number(left), StoredValue::Number(right)) => {
            stored_number_result(left.get() * right.get())
        }
        ("/", StoredValue::Number(_), StoredValue::Number(right)) if right.get() == 0.0 => Err(
            MigrationError::Evaluation("number division by zero".to_owned()),
        ),
        ("/", StoredValue::Number(left), StoredValue::Number(right)) => {
            stored_number_result(left.get() / right.get())
        }
        ("==", left, right) => Ok(StoredValue::Bool(left == right)),
        ("!=", left, right) => Ok(StoredValue::Bool(left != right)),
        ("&&", StoredValue::Bool(left), StoredValue::Bool(right)) => {
            Ok(StoredValue::Bool(left && right))
        }
        ("||", StoredValue::Bool(left), StoredValue::Bool(right)) => {
            Ok(StoredValue::Bool(left || right))
        }
        ("+", StoredValue::Text(left), StoredValue::Text(right)) => {
            Ok(StoredValue::Text(left + &right))
        }
        (operator, _, _) => Err(MigrationError::Evaluation(format!(
            "unsupported migration infix operator `{operator}` for these values"
        ))),
    }
}

fn project_stored_field(value: &StoredValue, field: &str) -> Result<StoredValue, MigrationError> {
    let fields = match value {
        StoredValue::Record(fields)
        | StoredValue::Variant { fields, .. }
        | StoredValue::Error { fields, .. } => fields,
        _ => {
            return Err(MigrationError::Evaluation(format!(
                "cannot project field `{field}` from a non-record value"
            )));
        }
    };
    fields
        .get(field)
        .cloned()
        .ok_or_else(|| MigrationError::Evaluation(format!("record has no field `{field}`")))
}

fn stored_tag(value: &StoredValue) -> &str {
    match value {
        StoredValue::Bool(true) => "True",
        StoredValue::Bool(false) => "False",
        StoredValue::Variant { tag, .. } | StoredValue::Error { code: tag, .. } => tag,
        StoredValue::Null => "Null",
        StoredValue::Number(_) => "Number",
        StoredValue::Text(value) => value,
        StoredValue::Bytes(_) => "Bytes",
        StoredValue::List(_) => "List",
        StoredValue::Record(_) => "Record",
    }
}

fn ensure_value_type(value: &StoredValue, data_type: &DataTypePlan) -> Result<(), MigrationError> {
    let valid = match (value, data_type) {
        (_, DataTypePlan::Unknown) => true,
        (StoredValue::Null, DataTypePlan::Null) => true,
        (StoredValue::Bool(_), DataTypePlan::Bool) => true,
        (StoredValue::Number(_), DataTypePlan::Number) => true,
        (StoredValue::Text(_), DataTypePlan::Text) => true,
        (StoredValue::Bytes(value), DataTypePlan::Bytes { fixed_len }) => {
            fixed_len.is_none_or(|expected| u64::try_from(value.len()) == Ok(expected))
        }
        (StoredValue::Text(tag), DataTypePlan::Variant { variants }) => {
            variants.iter().any(|variant| variant.tag == *tag)
        }
        (StoredValue::Variant { tag, fields }, DataTypePlan::Variant { variants }) => variants
            .iter()
            .find(|variant| variant.tag == *tag)
            .is_some_and(|variant| {
                (variant.open || fields.len() == variant.fields.len())
                    && variant.fields.iter().all(|field| {
                        fields
                            .get(&field.name)
                            .is_some_and(|value| ensure_value_type(value, &field.data_type).is_ok())
                    })
            }),
        (StoredValue::Record(values), DataTypePlan::Record { fields, open }) => {
            (*open || values.len() == fields.len())
                && fields.iter().all(|field| {
                    values
                        .get(&field.name)
                        .is_some_and(|value| ensure_value_type(value, &field.data_type).is_ok())
                })
        }
        (StoredValue::List(values), DataTypePlan::List { item }) => values
            .iter()
            .all(|value| ensure_value_type(value, item).is_ok()),
        (StoredValue::Error { fields: values, .. }, DataTypePlan::Error { fields, open }) => {
            (*open || values.len() == fields.len())
                && fields.iter().all(|field| {
                    values
                        .get(&field.name)
                        .is_some_and(|value| ensure_value_type(value, &field.data_type).is_ok())
                })
        }
        _ => false,
    };
    valid.then_some(()).ok_or_else(|| {
        MigrationError::OutputTypeMismatch(format!("value {value:?} does not match {data_type:?}"))
    })
}

fn row_count(image: &RestoreImage) -> usize {
    image.lists.values().map(|list| list.rows.len()).sum()
}

fn digest_hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_plan::{
        ApplicationPlan, DataTypeFieldPlan, InitialProvenance, ListMemoryPlan, MemoryKind,
        MemoryLeafPlan, MemoryOwnerPath, MemoryPlan, MigrationDestinationPlan,
        MigrationLeafRefPlan, MigrationListRowFieldPlan, PlanStorageId,
    };

    fn number(value: i64) -> StoredValue {
        StoredValue::integer(value).unwrap()
    }

    #[test]
    fn migration_number_expression_preserves_fractional_values() {
        let value = FiniteReal::new(1.25).unwrap();
        let evaluated = evaluate_expression(
            &MigrationExpressionPlan::Number { value },
            &BTreeMap::new(),
            &[],
        )
        .unwrap();

        assert_eq!(evaluated, StoredValue::Number(value));
    }

    fn scalar_memory(slot: usize, path: &str) -> MemoryPlan {
        MemoryPlan::new(
            PlanStorageId(slot),
            MemoryKind::Scalar,
            path,
            DataTypePlan::Number,
            InitialProvenance::ReconstructableDefault,
            MemoryOwnerPath {
                canonical_module: "app".to_owned(),
                named_owner_path: "store".to_owned(),
            },
        )
        .unwrap()
    }

    #[test]
    fn scalar_rename_is_staged_without_mutating_source() {
        let application = ApplicationPlan::new(ApplicationIdentity::new(
            "dev.boon.migration-test",
            "scalar-rename",
            "test",
        ))
        .unwrap();
        let source_memory = scalar_memory(0, "store.count");
        let target_memory = scalar_memory(0, "store.click_count");
        let source_plan = PersistencePlan::new(
            &application,
            1,
            vec![source_memory.clone()],
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let source_leaf = MigrationLeafRefPlan::new(
            source_memory.memory_id,
            source_memory.semantic_path.clone(),
            DataTypePlan::Number,
        )
        .unwrap();
        let input = MigrationInputPlan::new(vec![source_leaf], DataTypePlan::Number).unwrap();
        let destination = MigrationDestinationPlan::new(
            target_memory.memory_id,
            target_memory.semantic_path.clone(),
            DataTypePlan::Number,
        )
        .unwrap();
        let recipe = MigrationRecipePlan::new(vec![MigrationTransferPlan {
            transfer_kind: MigrationTransferKindPlan::Scalar,
            indexed_list_owner: None,
            list_row_fields: Vec::new(),
            transform: MigrationTransformPlan::Identity {
                input_id: input.input_id,
            },
            inputs: vec![input],
            destination,
        }])
        .unwrap();
        let edge =
            MigrationEdgePlan::new(1, 2, source_plan.schema_hash, recipe.migration_recipe_id)
                .unwrap();
        let target_plan = PersistencePlan::new_with_migrations(
            &application,
            2,
            vec![target_memory.clone()],
            Vec::new(),
            vec![recipe],
            Some(edge.migration_recipe_id),
            vec![edge],
        )
        .unwrap();
        let mut stored =
            RestoreImage::empty(application.identity.clone(), 1, source_plan.schema_hash);
        stored.scalars.insert(
            source_memory.memory_id,
            StoredScalar {
                touched: true,
                value: number(41),
            },
        );

        let staged = stage_migration_plan(&stored, &application.identity, &target_plan).unwrap();

        assert_eq!(stored.schema_version, 1);
        assert!(stored.scalars.contains_key(&source_memory.memory_id));
        assert_eq!(staged.candidate.schema_version, 2);
        assert_eq!(staged.candidate.schema_hash, target_plan.schema_hash);
        assert!(
            !staged
                .candidate
                .scalars
                .contains_key(&source_memory.memory_id)
        );
        assert_eq!(
            staged.candidate.scalars.get(&target_memory.memory_id),
            Some(&StoredScalar {
                touched: true,
                value: number(41),
            })
        );
        assert_eq!(staged.preview.steps.len(), 1);
        assert_eq!(staged.activation.authority_changes.len(), 1);
        assert_eq!(
            staged.activation.deleted_memory,
            vec![source_memory.memory_id]
        );
    }

    fn list_memory(
        slot: usize,
        path: &str,
        fields: &[&str],
        owner: MemoryOwnerPath,
    ) -> ListMemoryPlan {
        let memory_id = MemoryId::from_identity(&owner, path, MemoryKind::List).unwrap();
        let row_fields = fields
            .iter()
            .map(|field| {
                MemoryLeafPlan::new(
                    memory_id,
                    None,
                    format!("{path}.{field}"),
                    DataTypePlan::Text,
                )
                .unwrap()
            })
            .collect();
        ListMemoryPlan::new(
            PlanStorageId(slot),
            path,
            DataTypePlan::List {
                item: Box::new(DataTypePlan::Record {
                    fields: fields
                        .iter()
                        .map(|field| DataTypeFieldPlan {
                            name: (*field).to_owned(),
                            data_type: DataTypePlan::Text,
                        })
                        .collect(),
                    open: false,
                }),
            },
            InitialProvenance::ReconstructableDefault,
            owner,
            "TodoKey",
            true,
            row_fields,
        )
        .unwrap()
    }

    #[test]
    fn whole_list_migration_preserves_identity_order_allocator_and_sparse_fields() {
        let application = ApplicationPlan::new(ApplicationIdentity::new(
            "dev.boon.migration-test",
            "whole-list",
            "test",
        ))
        .unwrap();
        let owner = MemoryOwnerPath {
            canonical_module: "app".to_owned(),
            named_owner_path: "store".to_owned(),
        };
        let source = list_memory(0, "store.todos", &["title", "$input$title"], owner.clone());
        let target = list_memory(0, "store.tasks", &["title"], owner);
        let source_plan = PersistencePlan::new(
            &application,
            1,
            Vec::new(),
            vec![source.clone()],
            Vec::new(),
        )
        .unwrap();
        let source_input = MigrationInputPlan::new(
            vec![
                MigrationLeafRefPlan::new(
                    source.memory_id,
                    source.semantic_path.clone(),
                    source.data_type.clone(),
                )
                .unwrap(),
            ],
            source.data_type.clone(),
        )
        .unwrap();
        let source_title =
            MigrationLeafRefPlan::new(source.memory_id, "store.todos.title", DataTypePlan::Text)
                .unwrap();
        let source_constructor = MigrationLeafRefPlan::new(
            source.memory_id,
            "store.todos.$input$title",
            DataTypePlan::Text,
        )
        .unwrap();
        let target_title = MigrationDestinationPlan::new(
            target.memory_id,
            "store.tasks.title",
            DataTypePlan::Text,
        )
        .unwrap();
        let destination = MigrationDestinationPlan::new(
            target.memory_id,
            target.semantic_path.clone(),
            target.data_type.clone(),
        )
        .unwrap();
        let recipe = MigrationRecipePlan::new(vec![MigrationTransferPlan {
            transfer_kind: MigrationTransferKindPlan::List,
            indexed_list_owner: None,
            list_row_fields: vec![
                MigrationListRowFieldPlan {
                    source: source_title.clone(),
                    destination: Some(target_title.clone()),
                },
                MigrationListRowFieldPlan {
                    source: source_constructor.clone(),
                    destination: None,
                },
            ],
            transform: MigrationTransformPlan::Identity {
                input_id: source_input.input_id,
            },
            inputs: vec![source_input],
            destination,
        }])
        .unwrap();
        let edge =
            MigrationEdgePlan::new(1, 2, source_plan.schema_hash, recipe.migration_recipe_id)
                .unwrap();
        let target_plan = PersistencePlan::new_with_migrations(
            &application,
            2,
            Vec::new(),
            vec![target.clone()],
            vec![recipe],
            Some(edge.migration_recipe_id),
            vec![edge],
        )
        .unwrap();
        let mut stored =
            RestoreImage::empty(application.identity.clone(), 1, source_plan.schema_hash);
        stored.lists.insert(
            source.memory_id,
            StoredList {
                touched: true,
                next_key: 42,
                rows: vec![
                    StoredRow {
                        key: 9,
                        generation: 3,
                        fields: BTreeMap::from([
                            (source_title.leaf_id, StoredValue::Text("first".to_owned())),
                            (
                                source_constructor.leaf_id,
                                StoredValue::Text("constructor".to_owned()),
                            ),
                        ]),
                        touched_fields: BTreeSet::from([
                            source_title.leaf_id,
                            source_constructor.leaf_id,
                        ]),
                    },
                    StoredRow {
                        key: 4,
                        generation: 8,
                        fields: BTreeMap::new(),
                        touched_fields: BTreeSet::new(),
                    },
                ],
            },
        );

        let staged = stage_migration_plan(&stored, &application.identity, &target_plan).unwrap();
        let migrated = staged.candidate.lists.get(&target.memory_id).unwrap();
        assert!(migrated.touched);
        assert_eq!(migrated.next_key, 42);
        assert_eq!(
            migrated
                .rows
                .iter()
                .map(|row| (row.key, row.generation))
                .collect::<Vec<_>>(),
            vec![(9, 3), (4, 8)]
        );
        assert_eq!(
            migrated.rows[0].fields,
            BTreeMap::from([(target_title.leaf_id, StoredValue::Text("first".to_owned()))])
        );
        assert_eq!(
            migrated.rows[0].touched_fields,
            BTreeSet::from([target_title.leaf_id])
        );
        assert!(migrated.rows[1].fields.is_empty());
        assert!(migrated.rows[1].touched_fields.is_empty());
        assert!(!staged.candidate.lists.contains_key(&source.memory_id));
    }
}
