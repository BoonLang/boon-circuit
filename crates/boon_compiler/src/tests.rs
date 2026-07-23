use super::*;
use boon_plan::{
    DistributedRouteScopePlan, HostPortPlan, ListInitializerKind, PlanInfixOp,
    PlanListAccessSelection, PlanListProjection, PlanListRowFieldRole, PlanRowBuiltin,
    SourcePayloadField, distributed_graph_schema_hash,
};
use std::collections::BTreeSet;

#[test]
fn nested_effect_guards_lower_to_bounded_selector_conjunctions() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "nested-effect-guards.bn",
        r#"
store: [
    read: SOURCE
    selected: PackageAsset[url: TEXT { asset://files/primary.vcd }]
    file_result:
        NotStarted |> HOLD file_result {
            read |> THEN {
                File/read_stream(file: selected, retain_content: True)
            }
        }
    waveform_result:
        NotStarted |> HOLD waveform_result {
            file_result |> WHEN {
                Finished => file_result.retained |> WHEN {
                    Retained => Wellen/open(content: file_result.retained.content)
                    __ => SKIP
                }
                __ => SKIP
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let gates = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if compiled.plan.effects.iter().any(|contract| {
                contract.effect_id == effect.effect_id && contract.host_operation == "Wellen/open"
            }) =>
            {
                Some(&effect.gate)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [gate] = gates.as_slice() else {
        panic!("expected exactly one nested selector gate, got {gates:#?}");
    };
    let PlanRowExpressionNode::Select {
        input: outer_input,
        arms: outer_arms,
    } = row_node(&compiled.plan.row_expressions, **gate)
    else {
        panic!("nested selector gate lost its outer selector: {gates:#?}");
    };
    assert!(matches!(
        row_node(&compiled.plan.row_expressions, *outer_input),
        PlanRowExpressionNode::Field {
            input: ValueRef::State(_),
        }
    ));
    let retained_gate = outer_arms
        .iter()
        .find_map(|arm| {
            match (
                &arm.pattern,
                row_node(&compiled.plan.row_expressions, arm.value),
            ) {
                (
                    PlanRowSelectPattern::Text { value },
                    PlanRowExpressionNode::Select { input, arms },
                ) if value == "Finished" => Some((*input, arms)),
                _ => None,
            }
        })
        .expect("Finished arm must contain the retained-value selector");
    assert!(matches!(
        row_node(&compiled.plan.row_expressions, retained_gate.0),
        PlanRowExpressionNode::Field {
            input: ValueRef::StateProjection { field_path, .. },
        } if field_path == &["retained".to_owned()]
    ));
    let true_constant = retained_gate
        .1
        .iter()
        .find_map(|arm| {
            match (
                &arm.pattern,
                row_node(&compiled.plan.row_expressions, arm.value),
            ) {
                (
                    PlanRowSelectPattern::Text { value },
                    PlanRowExpressionNode::Constant { constant_id },
                ) if value == "Retained" => Some(*constant_id),
                _ => None,
            }
        })
        .expect("Retained arm must reach the effect");
    assert!(matches!(
        compiled
            .plan
            .constants
            .iter()
            .find(|constant| constant.id == true_constant)
            .map(|constant| &constant.value),
        Some(PlanConstantValue::Bool { value: true })
    ));
}

fn fixture_program_role(source: &str) -> ProgramRole {
    if source.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("document:") || line.starts_with("scene:")
    }) {
        ProgramRole::Client
    } else {
        ProgramRole::Server
    }
}

fn compile_fixture_source_text_to_machine_plan(
    source_label: &str,
    source: &str,
    target_profile: TargetProfile,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_source_text_to_machine_plan_for_role(
        source_label,
        source,
        target_profile,
        fixture_program_role(source),
    )
}

#[test]
fn nested_when_with_multiline_pipeline_arms_has_a_typed_derived_expression() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "nested-when-multiline-pipeline.bn",
        r#"
store: [
    active_file: TEXT { main.vcd }
    compare_file: TEXT { none }
    file_compare_status:
        compare_file |> WHEN {
            TEXT { none } => active_file |> WHEN {
                TEXT { none } => TEXT { no waveform loaded }
                file => TEXT { single file: } |> Text/concat(with: file, separator: " ")
            }
            compare =>
                TEXT { comparing }
                |> Text/concat(with: compare, separator: " ")
                |> Text/concat(with: TEXT { to }, separator: " ")
                |> Text/concat(with: active_file, separator: " ")
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let field = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.file_compare_status")
        .and_then(|entry| entry.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .expect("file_compare_status field id");
    let op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::Field(field)))
        .expect("file_compare_status derived op");
    assert!(
        matches!(
            op.kind,
            PlanOpKind::DerivedValue {
                expression: Some(_),
                ..
            }
        ),
        "nested WHEN output lost its typed expression: {op:#?}"
    );
}

#[test]
fn document_scalar_field_lowers_a_multiline_pipeline_as_one_value() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "document-multiline-scalar-pipeline.bn",
        r#"
store: [suffix: TEXT { ready }]

document: Document/new(
    root: Element/text(
        element: []
        text:
            TEXT { Reference }
            |> Text/concat(with: store.suffix, separator: ": ")
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled.plan.document.as_ref().unwrap();

    assert!(document.expressions.iter().any(|expression| {
        matches!(
            expression.op,
            DocumentExprOp::Builtin {
                builtin: boon_plan::DocumentBuiltin::TextConcat,
                input: Some(_),
                ..
            }
        )
    }));
}

fn compile_fixture_source_text_to_machine_plan_with_identity(
    source_label: &str,
    source: &str,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_source_text_to_machine_plan_for_role_with_identity(
        source_label,
        source,
        target_profile,
        fixture_program_role(source),
        application_identity,
    )
}

fn compile_fixture_runtime_source_text_with_persistence_identity(
    source_label: &str,
    source: &str,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
    schema_version: u64,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_text_to_machine_plan_for_role_with_persistence_catalog(
        source_label,
        source,
        target_profile,
        fixture_program_role(source),
        application_identity,
        schema_version,
        &[],
    )
}

fn compile_fixture_runtime_source_text_with_persistence_catalog(
    source_label: &str,
    source: &str,
    target_profile: TargetProfile,
    application_identity: ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_runtime_source_text_to_machine_plan_for_role_with_persistence_catalog(
        source_label,
        source,
        target_profile,
        fixture_program_role(source),
        application_identity,
        schema_version,
        migration_predecessors,
    )
}

#[test]
fn compiler_owns_transient_outbound_http_effect_contract_and_stable_routes() {
    let compiled = compile_source_path_to_machine_plan_for_role(
        std::path::Path::new("examples/outbound_http_effect.bn"),
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
    )
    .unwrap();
    let empty = BTreeSet::new();
    let unsupported = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| {
            !boon_plan::cpu_plan_executor_supports_whole_plan_op(
                &compiled.plan.row_expressions,
                &compiled.plan.storage_layout.scalar_slots,
                op,
                &empty,
            )
        })
        .map(|op| (op.id, op.kind.clone()))
        .collect::<Vec<_>>();
    assert!(
        compiled.plan.capability_summary.cpu_plan_executor_complete,
        "outbound HTTP fixture has unsupported ops: {unsupported:#?}"
    );
    assert!(compiled.plan.persistence.effect_outbox.is_empty());
    assert!(
        compiled
            .plan
            .persistence
            .memory
            .iter()
            .all(|memory| memory.semantic_path != "store.response")
    );
    let [contract] = compiled.plan.effects.as_slice() else {
        panic!("expected one outbound HTTP contract");
    };
    assert_eq!(contract.host_operation, "Http/request");
    assert_eq!(contract.replay, EffectReplay::ReadOnly);
    assert_eq!(contract.barrier, EffectBarrier::None);
    let schema = contract.schema.as_ref().unwrap();
    assert!(matches!(
        &schema.intent_type,
        DataTypePlan::Record { fields, open: false }
            if fields.iter().any(|field| {
                field.name == "headers"
                    && matches!(field.data_type, DataTypePlan::List { .. })
            })
    ));
    let invocation = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if effect.effect_id == contract.effect_id => Some(effect),
            _ => None,
        })
        .expect("typed outbound invocation");
    let EffectResultRoute::Target { target, policy } = &invocation.result;
    assert!(matches!(target, ValueRef::State(_)));
    assert_eq!(*policy, EffectResultPolicy::ReturnValue);
    let verification = verify_plan(&compiled.plan).unwrap();
    assert_eq!(
        verification.status,
        "pass",
        "failed plan checks: {:#?}",
        verification
            .checks
            .iter()
            .filter(|check| !check.pass)
            .collect::<Vec<_>>()
    );
}

#[test]
fn compiler_diagnostic_columns_match_editor_grapheme_positions() {
    let source = "first\ne\u{301}🙂value";
    let byte = source.find("value").unwrap();
    assert_eq!(grapheme_column(source, 2, byte), Some(3));
    assert_eq!(grapheme_column(source, 2, byte + 1), Some(4));
}

#[test]
fn root_value_comparison_lowers_both_typed_operands() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "root-value-comparison.bn",
        r#"
store: [
    change: SOURCE
    requested:
        0 |> HOLD requested {
            change |> THEN { requested + 1 }
        }
    settled:
        0 |> HOLD settled {
            change |> THEN { settled }
        }
    pending:
        requested == settled |> Bool/not()
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let pending = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.pending")
        .expect("pending field");
    let field = pending
        .id
        .strip_prefix("field:")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let expression = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| (op.output == Some(ValueRef::Field(FieldId(field)))).then_some(&op.kind))
        .and_then(|kind| match kind {
            PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } => Some(expression),
            _ => None,
        })
        .expect("typed pending expression");
    let PlanDerivedExpression::RowExpression { expression: root } = expression else {
        panic!("unexpected root comparison expression: {expression:#?}");
    };
    let PlanRowExpressionNode::BuiltinCall {
        function,
        input: Some(input),
        args,
    } = row_node(&compiled.plan.row_expressions, *root)
    else {
        panic!("unexpected root comparison expression: {expression:#?}");
    };
    assert_eq!(*function, PlanRowBuiltin::BoolNot);
    assert!(args.is_empty());
    let PlanRowExpressionNode::NumberInfix { op, left, right } =
        row_node(&compiled.plan.row_expressions, *input)
    else {
        panic!("Bool/not input lost its typed comparison: {expression:#?}");
    };
    assert_eq!(*op, PlanInfixOp::Equal);
    assert!(matches!(
        row_node(&compiled.plan.row_expressions, *left),
        PlanRowExpressionNode::Field {
            input: ValueRef::State(_)
        }
    ));
    assert!(matches!(
        row_node(&compiled.plan.row_expressions, *right),
        PlanRowExpressionNode::Field {
            input: ValueRef::State(_)
        }
    ));
    let verification = verify_plan(&compiled.plan).unwrap();
    assert_eq!(verification.error_count, 0, "{:#?}", verification.checks);
}

#[test]
fn timer_interval_lowers_once_as_a_scheduled_source_route() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "timer-interval.bn",
        r#"
store: [
    tick: Duration[milliseconds: 250] |> Timer/interval()
    count: 0 |> HOLD count {
        tick |> THEN { count + 1 }
    }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(compiled.plan.source_routes.len(), 1);
    assert_eq!(compiled.plan.source_routes[0].path, "store.tick");
    assert_eq!(compiled.plan.source_routes[0].interval_ms, Some(250));
    assert!(
        compiled
            .plan
            .debug_map
            .derived_values
            .iter()
            .all(|field| field.label != "store.tick"),
        "scheduled source must not also lower as a derived field"
    );
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
}

#[test]
fn source_payload_text_to_number_lowers_as_a_typed_conversion() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "source-text-to-number.bn",
        r#"
store: [
    input: SOURCE
    value:
        0 |> HOLD value {
            input.amount |> THEN {
                input.amount |> Text/to_number()
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let route = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.input")
        .expect("typed input source route");
    assert!(route.payload_schema.typed_fields.iter().any(|descriptor| {
        matches!(
            (&descriptor.field, &descriptor.data_type),
            (
                boon_plan::SourcePayloadField::Named(name),
                boon_plan::DataTypePlan::Text
            ) if name == "amount"
        )
    }));

    let update = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| {
            let PlanOpKind::StateUpdate {
                value: Some(value), ..
            } = &op.kind
            else {
                return false;
            };
            matches!(
                row_node(&compiled.plan.row_expressions, *value),
                PlanRowExpressionNode::TextToNumber { .. }
            )
        })
        .expect("TextToNumber update op");
    let PlanOpKind::StateUpdate {
        value: Some(value), ..
    } = &update.kind
    else {
        unreachable!();
    };
    let PlanRowExpressionNode::TextToNumber { input } =
        row_node(&compiled.plan.row_expressions, *value)
    else {
        unreachable!();
    };
    assert!(matches!(
        row_node(&compiled.plan.row_expressions, *input),
        PlanRowExpressionNode::Field {
            input: ValueRef::SourcePayload {
                field: boon_plan::SourcePayloadField::Named(name),
                ..
            },
        } if name == "amount"
    ));
    let Some(ValueRef::State(output)) = update.output else {
        panic!("TextToNumber update must target scalar state");
    };
    assert_eq!(
        compiled
            .plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id == output)
            .map(|slot| &slot.value_type),
        Some(&boon_plan::PlanValueType::Number)
    );
    let verification = verify_plan(&compiled.plan).unwrap();
    assert!(
        verification.checks.iter().all(|check| check.pass),
        "verification failures: {:?}",
        verification
            .checks
            .iter()
            .filter(|check| !check.pass)
            .collect::<Vec<_>>()
    );
}

#[test]
fn nested_source_payload_reused_by_text_input_keeps_its_exact_payload_read() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "nested-text-input-source-payload.bn",
        r#"
store: [
    sources: [station_input: [events: [change: SOURCE]]]
    selected:
        TEXT { initial } |> HOLD selected {
            sources.station_input.events.change.text
        }
    watch:
        TEXT { initial } |> HOLD watch {
            sources.station_input.events.change.text
        }
]
document: Document/new(
    root: Element/text_input(
        element: [events: store.sources.station_input.events]
        style: []
        label: TEXT { Station }
        text: store.selected
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let updates = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                value: Some(value), ..
            } => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        updates.iter().all(|update| matches!(
            row_node(&compiled.plan.row_expressions, **update),
            PlanRowExpressionNode::Field {
                input: ValueRef::SourcePayload {
                    field: boon_plan::SourcePayloadField::Text,
                    ..
                }
            }
        )),
        "nested source payload resolved through the UI record instead: {updates:#?}; reads={:#?}; bindings={:#?}",
        compiled.ir.scope_index.reads,
        compiled.ir.scope_index.bindings,
    );
}

#[test]
fn connected_fixture_repeated_nested_source_payloads_are_exact() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "persistence-fjordpulse-fixture.bn",
        include_str!("../../../examples/persistence_fjordpulse_fixture.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let station_source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.sources.station_input.events.change")
        .map(|route| route.source_id)
        .expect("station source");
    let updates = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                trigger: ValueRef::Source(source),
                value: Some(value),
                ..
            } if *source == station_source => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(updates.len(), 2);
    assert!(
        updates.iter().all(|update| matches!(
            row_node(&compiled.plan.row_expressions, **update),
            PlanRowExpressionNode::Field {
                input: ValueRef::SourcePayload {
                    field: boon_plan::SourcePayloadField::Text,
                    ..
                }
            }
        )),
        "station payload reads are not exact: {updates:#?}",
    );
}

#[test]
fn todo_v2_nested_mapped_rows_keep_typed_document_projections() {
    let path = example_path("examples/migrations/todo/v2.bn");
    let compiled =
        compile_source_path_to_machine_plan(&path, TargetProfile::SoftwareDefault).unwrap();
    let tasks = compiled
        .ir
        .lists
        .iter()
        .find(|list| list.name == "store.tasks")
        .expect("tasks list");
    let materialization = compiled
        .ir
        .materializations
        .iter()
        .find(|materialization| materialization.target_list_id == Some(tasks.id))
        .expect("tasks materialization");
    let source_list = materialization
        .source_list_id
        .expect("DRAIN source list identity");
    assert_eq!(
        materialization.source_scope_id,
        compiled
            .ir
            .lists
            .iter()
            .find(|list| list.id == source_list)
            .and_then(|list| list.row_scope_id),
        "DRAIN must preserve the exact source row scope"
    );
    let expression = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression:
                    Some(PlanDerivedExpression::MaterializeList {
                        target_list,
                        expression,
                        ..
                    }),
                ..
            } if *target_list == boon_plan::ListId(tasks.id.as_usize()) => Some(expression),
            _ => None,
        })
        .expect("tasks materialization expression");
    let PlanDerivedExpression::RowExpression { expression } = expression.as_ref() else {
        panic!("tasks materialization must use a row expression: {expression:#?}");
    };
    let PlanRowExpressionNode::ContextualCollection { captures, .. } =
        row_node(&compiled.plan.row_expressions, *expression)
    else {
        panic!("tasks materialization must use a contextual collection: {expression:#?}");
    };
    assert!(
        captures.is_empty(),
        "DRAIN authority transfer must not depend on transient row captures: {captures:#?}"
    );
    let verification = boon_plan::verify_plan(&compiled.plan).unwrap();
    assert_eq!(
        verification.status,
        "pass",
        "todo V2 plan is not ready: {verification:#?}; tasks slot={:#?}",
        compiled
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == boon_plan::ListId(tasks.id.as_usize()))
    );
    let tasks_slot = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == boon_plan::ListId(tasks.id.as_usize()))
        .expect("tasks slot");
    let authority_fields = tasks_slot
        .row_fields
        .iter()
        .filter(|field| field.role.is_authority())
        .map(|field| field.field_id)
        .collect::<BTreeSet<_>>();
    let state_defaults = compiled
        .plan
        .storage_layout
        .scalar_slots
        .iter()
        .filter(|slot| {
            slot.scope_id
                == tasks
                    .row_scope_id
                    .map(|scope| boon_plan::ScopeId(scope.as_usize()))
        })
        .collect::<Vec<_>>();
    assert!(!state_defaults.is_empty());
    assert!(
        state_defaults.iter().all(|slot| {
            let boon_plan::ScalarInitializerPlan::Expression { expression } = &slot.initializer
            else {
                return false;
            };
            matches!(
                row_node(&compiled.plan.row_expressions, *expression),
                PlanRowExpressionNode::Field {
                    input: ValueRef::Field(field),
                } if authority_fields.contains(field)
            )
        }),
        "DRAIN-backed state defaults must read durable target authority: {state_defaults:#?}"
    );
}
use boon_plan::{
    DataTypePlan, DocumentExprId, DocumentExprOp, DocumentMaterializationSource, DocumentRead,
    DocumentTextSegment, DocumentValueClass, EffectBarrier, EffectReplay, EffectResultPolicy,
    EffectResultRoute, FieldId, ListId, MemoryId, MemoryKind, MigrationExpressionPlan,
    MigrationPredecessorBinding, MigrationTransferKindPlan, MigrationTransformPlan,
    OutputContractKind, OutputDemandPolicy, OutputValueRef, PLAN_MAJOR_VERSION, PlanConstantValue,
    PlanContextualOperationKind, PlanDerivedExpression, PlanLocalId, PlanOpKind,
    PlanRowExpressionArena, PlanRowExpressionId, PlanRowExpressionNode, PlanRowSelectPattern,
    PlanStaticOwnerId, RootOutputDemand, ValueRef, plan_binary, plan_sha256, verify_plan,
};

fn row_node(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
) -> &PlanRowExpressionNode {
    arena.node(expression).unwrap_or_else(|error| {
        panic!(
            "invalid row expression id {} for arena length {}: {error}",
            expression.0,
            arena.len()
        )
    })
}

fn derived_row_expression_ids(expression: &PlanDerivedExpression) -> Vec<PlanRowExpressionId> {
    let mut roots = Vec::new();
    let mut stack = vec![expression];
    while let Some(expression) = stack.pop() {
        match expression {
            PlanDerivedExpression::MaterializeList { expression, .. }
            | PlanDerivedExpression::BoolNotExpression { input: expression } => {
                stack.push(expression);
            }
            PlanDerivedExpression::BoolAnd { left, right } => {
                stack.push(right);
                stack.push(left);
            }
            PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
                roots.push(*default);
                roots.extend(arms.iter().map(|arm| arm.value));
            }
            PlanDerivedExpression::RowExpression { expression }
            | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
                roots.push(*expression);
            }
            PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
            | PlanDerivedExpression::BoolNot { .. }
            | PlanDerivedExpression::NumberCompareConst { .. }
            | PlanDerivedExpression::ValueCompare { .. } => {}
        }
    }
    roots
}

fn visit_derived_row_nodes(
    arena: &PlanRowExpressionArena,
    expression: &PlanDerivedExpression,
    visitor: &mut impl FnMut(PlanRowExpressionId, &PlanRowExpressionNode),
) {
    for root in derived_row_expression_ids(expression) {
        arena
            .visit(root, visitor)
            .unwrap_or_else(|error| panic!("invalid derived row expression {}: {error}", root.0));
    }
}

fn expect_contextual_collection(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    expected_operation: PlanContextualOperationKind,
    context: &str,
) -> (
    PlanStaticOwnerId,
    PlanLocalId,
    PlanRowExpressionId,
    PlanRowExpressionId,
) {
    let node = row_node(arena, expression);
    let PlanRowExpressionNode::ContextualCollection {
        owner,
        operation,
        source,
        row_local,
        body,
        ..
    } = node
    else {
        panic!("{context} must lower as a typed contextual collection: {node:#?}");
    };
    assert_eq!(
        *operation, expected_operation,
        "{context} changed its contextual operation"
    );
    (*owner, *row_local, *source, *body)
}

fn expect_contextual_map(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    context: &str,
) -> (
    PlanStaticOwnerId,
    PlanLocalId,
    PlanRowExpressionId,
    PlanRowExpressionId,
) {
    expect_contextual_collection(arena, expression, PlanContextualOperationKind::Map, context)
}

fn expect_contextual_filter(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    context: &str,
) -> (
    PlanStaticOwnerId,
    PlanLocalId,
    PlanRowExpressionId,
    PlanRowExpressionId,
) {
    expect_contextual_collection(
        arena,
        expression,
        PlanContextualOperationKind::Filter,
        context,
    )
}

fn assert_contextual_local_projection(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    expected_owner: PlanStaticOwnerId,
    expected_local: PlanLocalId,
    expected_projection: &[&str],
    context: &str,
) {
    let node = row_node(arena, expression);
    match node {
        PlanRowExpressionNode::Local {
            owner,
            local,
            projection,
        } => {
            assert_eq!(*owner, expected_owner, "{context} changed static owner");
            assert_eq!(*local, expected_local, "{context} changed local identity");
            assert_eq!(
                projection,
                &expected_projection
                    .iter()
                    .map(|field| (*field).to_owned())
                    .collect::<Vec<_>>(),
                "{context} changed its projected row fields"
            );
        }
        PlanRowExpressionNode::ListRowField { row, .. }
            if expected_projection.len() == 1
                && matches!(
                    row_node(arena, *row),
                    PlanRowExpressionNode::LocalRow { owner, local }
                        if *owner == expected_owner && *local == expected_local
                ) => {}
        _ => panic!("{context} must read the typed contextual row: {node:#?}"),
    }
}

fn example_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn expression_has_typed_list_source(
    document: &boon_plan::DocumentPlan,
    expression: DocumentExprId,
) -> bool {
    match &document.expressions[expression.0].op {
        DocumentExprOp::Read {
            read:
                DocumentRead::List { .. }
                | DocumentRead::Field { .. }
                | DocumentRead::Row { field: Some(_), .. },
        } => true,
        DocumentExprOp::Builtin {
            input: Some(input), ..
        } => expression_has_typed_list_source(document, *input),
        _ => false,
    }
}

fn expression_reads_field(
    document: &boon_plan::DocumentPlan,
    expression: DocumentExprId,
    expected: boon_plan::FieldId,
) -> bool {
    match &document.expressions[expression.0].op {
        DocumentExprOp::Read {
            read: DocumentRead::Field { field },
        } => *field == expected,
        DocumentExprOp::Project { input, .. } => expression_reads_field(document, *input, expected),
        _ => false,
    }
}

fn compile_migration_fixture_chain(
    fixture: &str,
    final_version: u64,
    identity: ApplicationIdentity,
) {
    let mut predecessor = None;
    for version in 1..=final_version {
        let relative_path = format!("examples/migrations/{fixture}/v{version}.bn");
        let source = fs::read_to_string(example_path(&relative_path)).unwrap();
        let bindings = predecessor.as_slice();
        let compiled = compile_fixture_runtime_source_text_with_persistence_catalog(
            &relative_path,
            &source,
            TargetProfile::SoftwareDefault,
            identity.clone(),
            version,
            bindings,
        )
        .unwrap_or_else(|error| panic!("{relative_path} did not compile: {error}"));
        let verification = verify_plan(&compiled.plan).unwrap();
        assert_eq!(
            verification.status,
            "pass",
            "{relative_path} emitted an invalid MachinePlan: {:?}; unresolved_ops={:?}",
            verification
                .checks
                .iter()
                .filter(|check| !check.pass)
                .collect::<Vec<_>>(),
            compiled
                .plan
                .regions
                .iter()
                .flat_map(|region| &region.ops)
                .filter(|op| op.unresolved_executable_ref_count > 0)
                .map(|op| (op.id, &op.output, op.unresolved_executable_ref_count))
                .collect::<Vec<_>>()
        );
        predecessor = Some(MigrationPredecessorBinding::from_machine_plan(
            &compiled.plan,
        ));
    }
}

#[test]
fn compiler_emits_machine_plan_v4_as_its_only_output() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "examples/bytes_length_plan_ops.bn",
        include_str!("../../../examples/bytes_length_plan_ops.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(compiled.plan.version.major, PLAN_MAJOR_VERSION);
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
    assert!(compiled.profile.expression_count > 0);
}

#[test]
fn pure_function_wrapped_hold_initializer_is_materialized_as_a_typed_constant() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "function-initializer.bn",
        r#"
FUNCTION starter_text() {
    decoy: "not the function result"
    "first line\nsecond line"
}

store: [
    value:
        starter_text() |> HOLD value {
            LATEST {}
        }
]

scene: Scene/Element/text(
    element: []
    style: [width: Fill, height: 24]
    text: store.value
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let slot = compiled
        .plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| !slot.indexed)
        .unwrap();
    let boon_plan::ScalarInitializerPlan::Constant { constant_id } = &slot.initializer else {
        panic!("multiline text state must have one constant initializer");
    };
    let constant = &compiled.plan.constants[constant_id.0].value;
    assert_eq!(
        constant,
        &boon_plan::PlanConstantValue::Text {
            value: "first line\nsecond line".to_owned(),
        }
    );
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn compiler_lowers_typed_output_roots_into_the_generic_registry() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "counter-output-root.bn",
        include_str!("../../../examples/counter.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled.plan.document.as_ref().unwrap();
    assert_eq!(compiled.plan.program_role, ProgramRole::Client);

    assert_eq!(compiled.plan.outputs.len(), 1);
    assert_eq!(compiled.plan.outputs[0].name, "document");
    assert_eq!(
        compiled.plan.outputs[0].contract,
        OutputContractKind::Document
    );
    assert_eq!(
        compiled.plan.outputs[0].demand,
        OutputDemandPolicy::HostDemanded
    );
    assert_eq!(
        compiled.plan.outputs[0].value,
        OutputValueRef::RetainedVisual {
            expression: document.root.expression
        }
    );
    assert!(
        verify_plan(&compiled.plan)
            .unwrap()
            .checks
            .iter()
            .any(|check| check.id == "output-roots-typed-canonical-and-resolved" && check.pass)
    );
}

#[test]
fn compiler_lowers_closed_nonvisual_outputs_without_a_document_plan() {
    let compiled = compile_source_text_to_machine_plan_for_role(
        "server-outputs.bn",
        include_str!("../../../examples/server_outputs.bn"),
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
    )
    .unwrap();

    assert!(compiled.plan.document.is_none());
    assert_eq!(compiled.plan.program_role, ProgramRole::Server);
    assert_eq!(
        compiled
            .plan
            .outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>(),
        ["api_response", "pending_priorities"]
    );
    let response = compiled.plan.output_root("api_response").unwrap();
    assert!(matches!(
        &response.contract,
        OutputContractKind::HostValue {
            data_type: DataTypePlan::Record { open: false, .. }
        }
    ));
    assert!(matches!(
        &response.value,
        OutputValueRef::RuntimeValue {
            value: ValueRef::Field(_),
            ..
        }
    ));
    let jobs = compiled.plan.output_root("pending_priorities").unwrap();
    assert!(matches!(
        &jobs.contract,
        OutputContractKind::HostValue {
            data_type: DataTypePlan::List { .. }
        }
    ));
    assert!(matches!(
        &jobs.value,
        OutputValueRef::RuntimeValue {
            value: ValueRef::Field(_),
            ..
        }
    ));
    let [
        HostPortPlan::HttpServer {
            request_source,
            disconnect_source,
            response_output,
        },
    ] = compiled.plan.host_ports.as_slice()
    else {
        panic!("server fixture must lower one typed HTTP host port");
    };
    assert_eq!(disconnect_source, &None);
    let request_route = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.source_id == *request_source)
        .unwrap();
    assert_eq!(request_route.path, "store.request_received");
    assert!(
        request_route
            .payload_schema
            .typed_fields
            .iter()
            .any(|field| {
                field.field == SourcePayloadField::Named("path_segments".to_owned())
                    && matches!(
                        &field.data_type,
                        DataTypePlan::List { item } if item.as_ref() == &DataTypePlan::Text
                    )
            })
    );
    assert!(
        request_route
            .payload_schema
            .typed_fields
            .iter()
            .any(|field| {
                field.field == SourcePayloadField::Named("query".to_owned())
                    && matches!(
                        &field.data_type,
                        DataTypePlan::List { item }
                            if matches!(item.as_ref(), DataTypePlan::Record { open: false, .. })
                    )
            })
    );
    assert_eq!(*response_output, response.id);
    let verification = verify_plan(&compiled.plan).unwrap();
    let failures = verification
        .checks
        .iter()
        .filter(|check| !check.pass)
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "non-visual output plan must be closed and executable: {failures:?}"
    );
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "host-ports-typed-and-resolved" && check.pass)
    );
}

#[test]
fn compiler_executes_recursive_http_payload_list_get() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "server-http-echo.bn",
        include_str!("../../../examples/server_http_echo.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let empty = BTreeSet::new();
    let unsupported = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| {
            !boon_plan::cpu_plan_executor_supports_whole_plan_op(
                &compiled.plan.row_expressions,
                &compiled.plan.storage_layout.scalar_slots,
                op,
                &empty,
            )
        })
        .map(|op| (op.id, op.kind.clone()))
        .collect::<Vec<_>>();
    assert!(
        compiled.plan.capability_summary.cpu_plan_executor_complete,
        "recursive HTTP payload plan has unsupported ops: {unsupported:#?}"
    );
}

#[test]
fn compiler_preserves_multiline_list_arguments_in_source_event_transforms() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "http-query-list-pipeline.bn",
        r#"
store: [
    request: SOURCE
    joined:
        request.method |> THEN {
            request.query
                |> List/filter(item, if: item.name == TEXT { q })
                |> List/map(item, new: item.value)
                |> Text/join(separator: Text/empty())
        }
]

"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let transform = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::SourceEventTransform { arms, .. }),
                ..
            } => arms.first().map(|arm| &arm.value),
            _ => None,
        })
        .expect("source event transform");
    let transform = row_node(&compiled.plan.row_expressions, *transform);
    let PlanRowExpressionNode::BuiltinCall {
        function,
        input: Some(mapped),
        args: joined_args,
    } = transform
    else {
        panic!("terminal join call was not retained: {transform:#?}");
    };
    assert_eq!(*function, PlanRowBuiltin::TextJoin);
    assert_eq!(
        joined_args
            .iter()
            .map(|arg| arg.name.as_str())
            .collect::<Vec<_>>(),
        ["separator"]
    );
    let (map_owner, map_local, filtered, mapped_value) = expect_contextual_map(
        &compiled.plan.row_expressions,
        *mapped,
        "HTTP query value projection",
    );
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        mapped_value,
        map_owner,
        map_local,
        &["value"],
        "HTTP query value projection",
    );
    let (owner, row_local, _source, predicate) = expect_contextual_filter(
        &compiled.plan.row_expressions,
        filtered,
        "HTTP query filter",
    );
    let predicate_node = row_node(&compiled.plan.row_expressions, predicate);
    let PlanRowExpressionNode::NumberInfix { op, left, .. } = predicate_node else {
        panic!("HTTP query filter must retain its typed equality: {predicate:#?}");
    };
    assert_eq!(*op, PlanInfixOp::Equal);
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        *left,
        owner,
        row_local,
        &["name"],
        "HTTP query parameter name",
    );
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
}

#[test]
fn source_event_transform_ignores_skip_when_inferring_its_value_type() {
    compile_fixture_source_text_to_machine_plan(
        "source-event-skip-type.bn",
        r#"
store: [
    input: SOURCE
    result:
        input.key |> WHEN {
            Enter => TEXT { saved }
            __ => SKIP
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("SKIP is absence and must not make a Text event transform an Enum");
}

#[test]
fn fjordpulse_server_host_boundary_is_cpu_executable() {
    let compiled = compile_source_path_to_machine_plan_for_role(
        std::path::Path::new("examples/fjordpulse/Server/RUN.bn"),
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
    )
    .unwrap();
    let empty = BTreeSet::new();
    let unsupported = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| {
            !boon_plan::cpu_plan_executor_supports_whole_plan_op(
                &compiled.plan.row_expressions,
                &compiled.plan.storage_layout.scalar_slots,
                op,
                &empty,
            )
        })
        .map(|op| {
            let output_label = match &op.output {
                Some(ValueRef::Field(field)) => compiled
                    .plan
                    .debug_map
                    .fields
                    .iter()
                    .find(|entry| entry.id == format!("field:{}", field.0))
                    .map(|entry| entry.label.clone()),
                _ => None,
            };
            (op.id, op.output.clone(), output_label, op.kind.clone())
        })
        .collect::<Vec<_>>();
    assert!(
        compiled.plan.capability_summary.cpu_plan_executor_complete,
        "FjordPulse server has unsupported ops: {unsupported:#?}"
    );
    let station_list = compiled
        .plan
        .debug_map
        .list_slots
        .iter()
        .find(|entry| entry.label.ends_with("store.stations"))
        .and_then(|entry| entry.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("station catalog list identity");
    let stations = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == station_list)
        .expect("station catalog list");
    assert_eq!(stations.initial_rows.len(), 5);
    assert!(stations.initial_rows.iter().all(|row| {
        ["id", "kind", "latitude", "longitude", "modes", "name"]
            .into_iter()
            .all(|name| row.fields.iter().any(|field| field.name == name))
    }));
    assert_eq!(compiled.plan.list_indexes.len(), 1);
    assert_eq!(compiled.plan.list_indexes[0].source_list, station_list);
    assert_eq!(compiled.plan.list_indexes[0].keys.len(), 1);
    let access = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::MaterializeList { expression, .. }),
                ..
            } => match expression.as_ref() {
                PlanDerivedExpression::RowExpression { expression } => {
                    match row_node(&compiled.plan.row_expressions, *expression) {
                        PlanRowExpressionNode::ListAccess { access } => Some(access.as_ref()),
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        })
        .expect("inline bounded station access");
    assert!(matches!(
        access.selection,
        boon_plan::PlanListAccessSelection::TextPrefix { .. }
    ));
}

#[test]
fn compiler_lowers_decimal_numbers_as_canonical_executable_number_constants() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "real-output.bn",
        r#"
store: [
    latitude: 59.91
]

outputs: [
    latitude: store.latitude
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
    assert!(compiled.plan.constants.iter().any(|constant| {
        matches!(
            constant.value,
            boon_plan::PlanConstantValue::Number { value }
                if (value.get() - 59.91).abs() < f64::EPSILON
        )
    }));
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn omitted_order_direction_erases_to_an_ascending_typed_index() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "default-order-direction.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { Beta }]
        [name: TEXT { Alpha }]
    }
    result:
        items
        |> List/sort_by(item, key: item.name)
        |> List/take(count: 2)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("omitted order direction must have an executable default");

    let [index] = compiled.plan.list_indexes.as_slice() else {
        panic!(
            "default ordering must lower to one typed index: {:#?}",
            compiled.plan.list_indexes
        );
    };
    assert_eq!(index.keys.len(), 1);
    assert_eq!(
        index.keys[0].direction,
        boon_plan::PlanOrderDirection::Ascending
    );
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn dynamic_order_direction_erases_to_prebuilt_static_index_branches() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "dynamic-order-direction.bn",
        r#"
store: [
    reverse: SOURCE
    direction:
        Ascending |> HOLD direction {
            reverse |> THEN { Descending }
        }
    items: LIST {
        [name: TEXT { Beta }]
        [name: TEXT { Alpha }]
    }
    result:
        items
        |> List/sort_by(item, key: item.name, direction: direction)
        |> List/take(count: 2)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("dynamic order direction must lower to prebuilt physical branches");

    assert_eq!(compiled.plan.list_indexes.len(), 2);
    assert_eq!(
        compiled
            .plan
            .list_indexes
            .iter()
            .filter(|index| {
                index.keys.as_slice().first().map(|key| key.direction)
                    == Some(boon_plan::PlanOrderDirection::Ascending)
            })
            .count(),
        1
    );
    assert_eq!(
        compiled
            .plan
            .list_indexes
            .iter()
            .filter(|index| {
                index.keys.as_slice().first().map(|key| key.direction)
                    == Some(boon_plan::PlanOrderDirection::Descending)
            })
            .count(),
        1
    );

    let mut directional_selects = 0;
    for op in compiled.plan.regions.iter().flat_map(|region| &region.ops) {
        let PlanOpKind::DerivedValue {
            expression: Some(expression),
            ..
        } = &op.kind
        else {
            continue;
        };
        visit_derived_row_nodes(
            &compiled.plan.row_expressions,
            expression,
            &mut |_, node| {
                if let PlanRowExpressionNode::Select { arms, .. } = node
                    && arms.len() == 2
                    && arms.iter().all(|arm| {
                        matches!(
                            row_node(&compiled.plan.row_expressions, arm.value),
                            PlanRowExpressionNode::ListAccess { .. }
                        )
                    })
                {
                    directional_selects += 1;
                }
            },
        );
    }
    assert_eq!(directional_selects, 1);
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn independent_dynamic_order_directions_prebuild_every_direction_vector() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "dynamic-order-vector.bn",
        r#"
store: [
    reverse_primary: SOURCE
    reverse_secondary: SOURCE
    primary_direction:
        Ascending |> HOLD primary_direction {
            reverse_primary |> THEN { Descending }
        }
    secondary_direction:
        Descending |> HOLD secondary_direction {
            reverse_secondary |> THEN { Ascending }
        }
    items: LIST {
        [name: TEXT { Beta }, rank: 2]
        [name: TEXT { Alpha }, rank: 1]
    }
    result:
        items
        |> List/sort_by(item, key: item.name, direction: primary_direction)
        |> List/then_by(item, key: item.rank, direction: secondary_direction)
        |> List/take(count: 2)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("two independent directions must lower to four prebuilt indexes");

    assert_eq!(compiled.plan.list_indexes.len(), 4);
    let vectors = compiled
        .plan
        .list_indexes
        .iter()
        .map(|index| {
            let [primary, secondary] = index.keys.as_slice() else {
                panic!("dynamic compound index must have two keys: {index:#?}");
            };
            (
                primary.direction == boon_plan::PlanOrderDirection::Descending,
                secondary.direction == boon_plan::PlanOrderDirection::Descending,
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        vectors,
        BTreeSet::from([(false, false), (false, true), (true, false), (true, true)])
    );
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn named_intermediate_preserves_authoritative_order_chain_into_access_planning() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "named-order-chain.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { Beta }, rank: 2]
        [name: TEXT { Alpha }, rank: 1]
        [name: TEXT { Alpha }, rank: 3]
    }
    primary:
        items |> List/sort_by(item, key: item.name)
    ordered:
        primary
        |> List/then_by(item, key: item.rank, direction: Descending)
        |> List/take(count: 3)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("checked order provenance must survive a named intermediate");

    let [index] = compiled.plan.list_indexes.as_slice() else {
        panic!(
            "named compound ordering must own one typed index: {:#?}",
            compiled.plan.list_indexes
        );
    };
    assert_eq!(index.keys.len(), 2);
    assert_eq!(
        index.keys[0].direction,
        boon_plan::PlanOrderDirection::Ascending
    );
    assert_eq!(
        index.keys[1].direction,
        boon_plan::PlanOrderDirection::Descending
    );
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn bounded_list_access_rejects_unseekable_filtered_take() {
    for (label, predicate) in [
        (
            "or-prefix",
            r#"Bool/or(
                left: item.name |> Text/starts_with(prefix: store.query)
                right: item.active
            )"#,
        ),
        (
            "contains",
            "item.name |> Text/contains(needle: store.query)",
        ),
    ] {
        let source = format!(
            r#"
store: [
    query: TEXT {{ a }}
    items: LIST {{
        [name: TEXT {{ Alpha }}, active: True]
        [name: TEXT {{ Beta }}, active: False]
    }}
    result:
        items
        |> List/filter(item, if: {predicate})
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/take(count: 1)
]
document: Document/new(root: Element/label(element: [], label: TEXT {{ static }}))
"#
        );
        let error = compile_fixture_source_text_to_machine_plan(
            &format!("{label}.bn"),
            &source,
            TargetProfile::SoftwareDefault,
        )
        .expect_err("filtered take without a bounded access path must fail closed");
        assert!(
            error.to_string().contains(
                "typed List/take has no compiler-proven bounded source-order or keyed access path"
            ),
            "unexpected {label} diagnostic: {error}"
        );
    }
}

#[test]
fn repeated_filters_lower_to_one_conjunctive_typed_access() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "repeated-filter-access.bn",
        r#"
store: [
    selected_city: TEXT { Oslo }
    prefix: TEXT { A }
    items: LIST {
        [city: TEXT { Oslo }, name: TEXT { Alpha }]
        [city: TEXT { Bergen }, name: TEXT { Alna }]
        [city: TEXT { Oslo }, name: TEXT { Beta }]
    }
    result:
        items
        |> List/filter(item, if: item.city == selected_city)
        |> List/filter(item, if: item.name |> Text/starts_with(prefix: prefix))
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/take(count: 10)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(compiled.plan.list_indexes.len(), 1);
    assert_eq!(compiled.plan.list_indexes[0].keys.len(), 2);
    let access = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::MaterializeList { expression, .. }),
                ..
            } => match expression.as_ref() {
                PlanDerivedExpression::RowExpression { expression } => {
                    match row_node(&compiled.plan.row_expressions, *expression) {
                        PlanRowExpressionNode::ListAccess { access } => Some(access.as_ref()),
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        })
        .expect("repeated filters must lower to typed access");
    assert_eq!(access.filters.len(), 2);
    assert!(matches!(
        &access.selection,
        boon_plan::PlanListAccessSelection::TextPrefix { leading, .. } if leading.len() == 1
    ));
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn typed_token_membership_expands_one_bounded_index_key_per_list_item() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "typed-token-membership.bn",
        r#"
store: [
    selected_token: TEXT { rail }
    items: LIST {
        [name: TEXT { Alpha }, rank: 2, tokens: LIST { TEXT { rail }, TEXT { oslo } }]
        [name: TEXT { Beta }, rank: 1, tokens: LIST { TEXT { bus } }]
        [name: TEXT { Gamma }, rank: 3, tokens: LIST {}]
    }
    result:
        items
        |> List/filter(item, if:
            item.tokens
            |> List/any(item, if: item == selected_token)
        )
        |> List/sort_by(item, key: item.rank, direction: Ascending)
        |> List/take(count: 10)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("typed token membership must lower to a bounded expanded index");

    let [index] = compiled.plan.list_indexes.as_slice() else {
        panic!(
            "token membership must own one typed index: {:#?}",
            compiled.plan.list_indexes
        );
    };
    let [tokens, rank] = index.keys.as_slice() else {
        panic!("token index must precede semantic ordering: {index:#?}");
    };
    assert_eq!(
        tokens.multiplicity,
        boon_plan::PlanListIndexKeyMultiplicity::ListItems {
            max_items: boon_plan::MAX_TYPED_LIST_EXPANDED_KEYS_PER_ROW,
        }
    );
    assert_eq!(
        rank.multiplicity,
        boon_plan::PlanListIndexKeyMultiplicity::One
    );
    let access = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::MaterializeList { expression, .. }),
                ..
            } => match expression.as_ref() {
                PlanDerivedExpression::RowExpression { expression } => {
                    match row_node(&compiled.plan.row_expressions, *expression) {
                        PlanRowExpressionNode::ListAccess { access } => Some(access.as_ref()),
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        })
        .expect("token membership must lower to typed bounded access");
    assert!(matches!(
        &access.selection,
        PlanListAccessSelection::KeyPrefix { values } if values.len() == 1
    ));
    assert_eq!(access.semantic_order.len(), 1);
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn transparent_user_wrappers_lower_to_the_same_typed_list_access_plan() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "wrapped-typed-list-access.bn",
        r#"
FUNCTION matching(list, entry: OUT, predicate) {
    list |> List/filter(item: entry, if: predicate)
}

FUNCTION ordered(list, entry: OUT, key) {
    list |> List/sort_by(item: entry, key: key, direction: Ascending)
}

FUNCTION secondary(list, entry: OUT, key) {
    list |> List/then_by(item: entry, key: key, direction: Descending)
}

FUNCTION limited(list, count) {
    list |> List/take(count: count)
}

FUNCTION paged(list, size, after) {
    list |> List/page(size: size, after: after)
}

store: [
    prefix: TEXT { A }
    items: LIST {
        [name: TEXT { Alpha }, rank: 1]
        [name: TEXT { Alpha }, rank: 2]
        [name: TEXT { Beta }, rank: 3]
    }
    matches:
        items
        |> matching(
            entry
            predicate: entry.name |> Text/starts_with(prefix: prefix)
        )
        |> ordered(entry, key: entry.name)
        |> secondary(entry, key: entry.rank)
        |> limited(count: 2)
    page:
        items
        |> matching(
            entry
            predicate: entry.name |> Text/starts_with(prefix: prefix)
        )
        |> ordered(entry, key: entry.name)
        |> secondary(entry, key: entry.rank)
        |> paged(size: 2, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("transparent wrappers must preserve typed list access lowering");

    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
    assert_eq!(compiled.plan.list_indexes.len(), 1);
    let mut access_selections = Vec::new();
    for op in compiled.plan.regions.iter().flat_map(|region| &region.ops) {
        let PlanOpKind::DerivedValue {
            expression: Some(expression),
            ..
        } = &op.kind
        else {
            continue;
        };
        visit_derived_row_nodes(
            &compiled.plan.row_expressions,
            expression,
            &mut |_, node| match node {
                PlanRowExpressionNode::ListAccess { access } => {
                    access_selections.push(access.selection.clone());
                }
                PlanRowExpressionNode::ListPage { page } => {
                    access_selections.push(page.access.selection.clone());
                }
                _ => {}
            },
        );
    }
    assert_eq!(access_selections.len(), 2);
    assert!(access_selections.iter().all(|selection| matches!(
        selection,
        PlanListAccessSelection::TextPrefix { leading, .. } if leading.is_empty()
    )));
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn indexed_hold_used_as_a_mapped_row_field_has_one_exact_storage_binding() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "mapped-indexed-hold-field.bn",
        r#"
store: [
    replace: SOURCE
    rows:
        LIST { [name: TEXT { initial }] }
        |> List/map(item, new: [
            name:
                item.name |> HOLD name {
                    replace.text |> THEN { replace.text }
                }
        ])
    ordered:
        rows
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/take(count: 1)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("a nested indexed HOLD is row authority, not a duplicate list declaration");

    let unsupported = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| {
            !boon_plan::cpu_plan_executor_supports_whole_plan_op(
                &compiled.plan.row_expressions,
                &compiled.plan.storage_layout.scalar_slots,
                op,
                &BTreeSet::new(),
            )
        })
        .map(|op| (op.id, op.output.clone(), op.kind.clone()))
        .collect::<Vec<_>>();
    assert!(
        compiled.plan.capability_summary.cpu_plan_executor_complete,
        "mapped indexed HOLD has unsupported ops: {unsupported:#?}"
    );
    assert_eq!(compiled.plan.list_indexes.len(), 1);
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn terminal_list_page_lowers_in_source_order_and_inside_records() {
    for (label, page_expression) in [
        (
            "root",
            "items |> List/page(size: 2, after: Start)".to_owned(),
        ),
        (
            "record",
            "[result: items |> List/page(size: 2, after: Start)]".to_owned(),
        ),
    ] {
        let compiled = compile_fixture_source_text_to_machine_plan(
            &format!("page-{label}.bn"),
            &format!(
                r#"
store: [
    items: LIST {{
        [name: TEXT {{ Alpha }}]
        [name: TEXT {{ Beta }}]
        [name: TEXT {{ Gamma }}]
    }}
    page: {page_expression}
]
document: Document/new(root: Element/label(element: [], label: TEXT {{ static }}))
"#
            ),
            TargetProfile::SoftwareDefault,
        )
        .unwrap();
        assert_eq!(compiled.plan.list_indexes.len(), 1, "{label}");
        assert!(compiled.plan.list_indexes[0].keys.is_empty(), "{label}");
        let mut pages = Vec::new();
        for op in compiled.plan.regions.iter().flat_map(|region| &region.ops) {
            let PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } = &op.kind
            else {
                continue;
            };
            visit_derived_row_nodes(
                &compiled.plan.row_expressions,
                expression,
                &mut |_, node| {
                    if let PlanRowExpressionNode::ListPage { page } = node {
                        pages.push(page.as_ref().clone());
                    }
                },
            );
        }
        assert_eq!(pages.len(), 1, "{label}");
        assert_ne!(pages[0].view_fingerprint, [0; 32], "{label}");
        assert!(matches!(
            pages[0].access.selection,
            PlanListAccessSelection::OrderedStart
        ));
    }
}

#[test]
fn terminal_list_page_rejects_invalid_literal_sizes() {
    for size in ["0", "1.5", "10001"] {
        let error = compile_fixture_source_text_to_machine_plan(
            "page-invalid-literal-size.bn",
            &format!(
                r#"
store: [
    items: LIST {{ [name: TEXT {{ Alpha }}] }}
    page: items |> List/page(size: {size}, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT {{ static }}))
"#
            ),
            TargetProfile::SoftwareDefault,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("size must be a whole Number between 1 and 10000"),
            "unexpected size {size} error: {error}"
        );
    }
}

#[test]
fn typed_page_fingerprint_uses_semantics_instead_of_plan_local_ids() {
    fn fingerprint(source: &str) -> [u8; 32] {
        let compiled = compile_fixture_source_text_to_machine_plan(
            "page-semantic-fingerprint.bn",
            source,
            TargetProfile::SoftwareDefault,
        )
        .unwrap();
        let mut fingerprints = Vec::new();
        for op in compiled.plan.regions.iter().flat_map(|region| &region.ops) {
            if let PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } = &op.kind
            {
                visit_derived_row_nodes(
                    &compiled.plan.row_expressions,
                    expression,
                    &mut |_, node| {
                        if let PlanRowExpressionNode::ListPage { page } = node {
                            fingerprints.push(page.view_fingerprint);
                        }
                    },
                );
            }
        }
        assert_eq!(fingerprints.len(), 1);
        fingerprints[0]
    }

    let base = r#"
store: [
    items: LIST { [name: TEXT { Alpha }] [name: TEXT { Beta }] }
    page:
        items
        |> List/filter(item, if: item.name |> Text/starts_with(prefix: TEXT { A }))
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#;
    let shifted_ids = r#"
store: [
    unrelated: TEXT { unrelated-constant }
    items: LIST { [name: TEXT { Alpha }] [name: TEXT { Beta }] }
    page:
        items
        |> List/filter(item, if: item.name |> Text/starts_with(prefix: TEXT { A }))
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#;
    let changed_semantics = base.replace("prefix: TEXT { A }", "prefix: TEXT { B }");
    assert_eq!(fingerprint(base), fingerprint(shifted_ids));
    assert_ne!(fingerprint(base), fingerprint(&changed_semantics));
}

#[test]
fn terminal_list_page_fails_closed_without_a_proven_seek_path() {
    let error = compile_fixture_source_text_to_machine_plan(
        "page-unseekable.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { Alpha }, active: True]
        [name: TEXT { Beta }, active: False]
    }
    page:
        items
        |> List/filter(item, if: item.active)
        |> List/page(size: 2, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("no compiler-proven bounded source-order or keyed access path"),
        "unexpected page lowering error: {error}"
    );
}

#[test]
fn compiler_rejects_integer_literals_not_exactly_representable_as_number() {
    let error = compile_fixture_source_text_to_machine_plan(
        "inexact-number.bn",
        r#"
store: [
    value: 9007199254740993
]
outputs: [
    value: store.value
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap_err();
    let error = error.to_string();
    assert!(
        error.contains("cannot be represented exactly as a Boon Number"),
        "unexpected error: {error}"
    );
}

#[test]
fn output_root_identity_ignores_formatting_and_unrelated_declarations() {
    let compact = compile_fixture_source_text_to_machine_plan(
        "stable-output.bn",
        r#"
store: [
    value: 7 |> HOLD value { LATEST {} }
]
outputs: [
    delivery_result: store.value
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let reformatted = compile_fixture_source_text_to_machine_plan(
        "stable-output.bn",
        r#"
-- unrelated formatting and declaration do not define host identity
helper: TEXT { ignored }

store: [
    value:
        7 |> HOLD value {
            LATEST {}
        }
]

outputs: [
    delivery_result: store.value
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(
        compact.plan.output_root("delivery_result").unwrap().id,
        reformatted.plan.output_root("delivery_result").unwrap().id
    );
}

#[test]
fn consequential_io_cannot_hide_in_retained_document_evaluation() {
    let error = compile_fixture_source_text_to_machine_plan(
        "document-log-effect.bn",
        r#"
document: Document/new(
    root: Element/label(
        element: []
        label: TEXT { hidden effect } |> Log/info()
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cannot run during retained document evaluation"),
        "unexpected error: {error}"
    );
}

#[test]
fn compiler_uses_central_host_effect_contracts_for_bounded_file_operations() {
    let read = compile_fixture_source_text_to_machine_plan(
        "bytes-file-read.bn",
        include_str!("../../../examples/bytes_file_read_plan_ops.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert_eq!(read.plan.effects.len(), 1);
    assert_eq!(read.plan.effects[0].host_operation, "File/read_bytes");
    assert_eq!(read.plan.effects[0].replay, EffectReplay::ReadOnly);
    assert_eq!(read.plan.effects[0].barrier, EffectBarrier::None);

    let write = compile_fixture_source_text_to_machine_plan(
        "transactional-file-write.bn",
        include_str!("../../../examples/bytes_file_write_effect.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let contract = write
        .plan
        .effects
        .iter()
        .find(|contract| contract.host_operation == "File/write_bytes")
        .expect("write effect contract");
    assert_eq!(contract.replay, EffectReplay::ProcessScoped);
    assert_eq!(contract.barrier, EffectBarrier::None);
    assert_eq!(
        contract.schema.as_ref().unwrap().intent_constraints,
        vec![boon_plan::EffectIntentConstraintPlan::BytesLengthRange {
            field_path: vec!["bytes".to_owned()],
            min_inclusive: 0,
            max_inclusive: 16 * 1024 * 1024,
        }]
    );
    assert!(
        !write
            .plan
            .persistence
            .effect_outbox
            .iter()
            .any(|schema| schema.effect_id == contract.effect_id),
        "process-scoped writes must not enter the durable outbox"
    );
    let invocation = write
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } => Some(effect),
            _ => None,
        })
        .expect("write effect invocation");
    assert_eq!(invocation.effect_id, contract.effect_id);
    assert_eq!(
        invocation
            .intent_fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["bytes", "file"]
    );
    assert!(
        verify_plan(&write.plan)
            .unwrap()
            .checks
            .iter()
            .all(|check| check.pass),
        "compiled bounded write plan must verify"
    );
}

#[test]
fn numeric_byte_operations_lower_to_dedicated_typed_expressions() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "bytes-numeric-plan-ops.bn",
        include_str!("../../../examples/bytes_numeric_plan_ops.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let plan = format!("{:#?}", compiled.plan);
    for expression in [
        "BytesReadUnsigned",
        "BytesReadSigned",
        "BytesWriteUnsigned",
        "BytesWriteSigned",
    ] {
        assert!(
            plan.contains(expression),
            "compiled plan is missing {expression}"
        );
    }
}

fn typed_passkey_effect_source() -> &'static str {
    include_str!("../../../testdata/typed_passkey_effects.bn")
}

#[test]
fn compiler_lowers_typed_passkey_calls_to_canonical_outbox_and_result_states() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "typed-passkey-effects.bn",
        typed_passkey_effect_source(),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    for operation in [
        "DevelopmentPasskey/register",
        "DevelopmentPasskey/authenticate",
    ] {
        let contract = compiled
            .plan
            .effects
            .iter()
            .find(|contract| contract.host_operation == operation)
            .unwrap();
        assert_eq!(contract.result_policy, EffectResultPolicy::ReturnValue);
        assert_eq!(contract.barrier, EffectBarrier::BeforeAndAfter);
        let schema = compiled
            .plan
            .persistence
            .effect_outbox
            .iter()
            .find(|schema| schema.effect_id == contract.effect_id)
            .unwrap();
        assert!(!schema.invocation_ids.is_empty());
    }
    let registration = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if compiled.plan.effects.iter().any(|contract| {
                contract.effect_id == effect.effect_id
                    && contract.host_operation == "DevelopmentPasskey/register"
            }) =>
            {
                Some(effect)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        registration
            .intent_fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        [
            "account_id",
            "credential_count",
            "simulation",
            "workspace_grant_id",
            "workspace_id"
        ]
    );
    let simulation = registration
        .intent_fields
        .iter()
        .find(|field| field.name == "simulation")
        .unwrap();
    let DataTypePlan::Variant { variants } = &simulation.data_type else {
        panic!("simulation intent must have a variant schema");
    };
    assert_eq!(
        variants
            .iter()
            .map(|variant| variant.tag.as_str())
            .collect::<Vec<_>>(),
        ["Cancel", "Duplicate", "Failure", "Success"]
    );
    let EffectResultRoute::Target { target, policy } = &registration.result;
    assert!(matches!(target, ValueRef::State(_)));
    assert_eq!(*policy, EffectResultPolicy::ReturnValue);
    let persistent_paths = compiled
        .plan
        .persistence
        .memory
        .iter()
        .map(|memory| memory.semantic_path.as_str())
        .collect::<BTreeSet<_>>();
    assert!(persistent_paths.contains("store.registration_result"));
    assert!(persistent_paths.contains("store.authentication_result"));
    let verification = verify_plan(&compiled.plan).unwrap();
    assert!(
        verification.checks.iter().all(|check| check.pass),
        "verification failures: {:?}",
        verification
            .checks
            .iter()
            .filter(|check| !check.pass)
            .collect::<Vec<_>>()
    );
}

#[test]
fn host_effect_list_intent_uses_the_semantic_list_runtime_identity() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "server-effect-list-intent.bn",
        include_str!("../../../examples/server_effect_chain.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let http_effect = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if compiled.plan.effects.iter().any(|contract| {
                contract.effect_id == effect.effect_id && contract.host_operation == "Http/request"
            }) =>
            {
                Some(effect)
            }
            _ => None,
        })
        .expect("HTTP effect invocation");
    let headers = http_effect
        .intent_fields
        .iter()
        .find(|field| field.name == "headers")
        .expect("HTTP headers intent field");
    assert!(
        matches!(
            row_node(&compiled.plan.row_expressions, headers.expression),
            PlanRowExpressionNode::ListRef { .. }
        ),
        "semantic list memory must lower to its runtime ListId, not a root FieldId"
    );
    assert!(
        verify_plan(&compiled.plan)
            .unwrap()
            .checks
            .iter()
            .all(|check| check.pass)
    );
}

#[test]
fn state_triggered_effect_plan_has_no_original_source_input() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "state-triggered-effect-chain.bn",
        include_str!("../../../testdata/state_triggered_effect_chain.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let start = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.start")
        .unwrap()
        .source_id;
    let random = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| {
            matches!(&op.kind,
                PlanOpKind::StateUpdate {
                    effect: Some(effect),
                    ..
                } if compiled.plan.effects.iter().any(|contract|
                    contract.effect_id == effect.effect_id
                        && contract.host_operation == "Random/bytes"))
        })
        .expect("Random/bytes plan op");
    let PlanOpKind::StateUpdate { trigger, .. } = &random.kind else {
        unreachable!();
    };
    assert!(matches!(trigger, ValueRef::State(_)));
    assert!(
        random
            .inputs
            .iter()
            .all(|input| !matches!(input, ValueRef::Source(source) if *source == start)),
        "a state-triggered effect must not retain the original SOURCE input"
    );
    assert!(
        verify_plan(&compiled.plan)
            .unwrap()
            .checks
            .iter()
            .all(|check| check.pass)
    );
}

#[test]
fn host_effect_schema_default_lowers_to_a_typed_plan_constant() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "defaulted-host-effect-intent.bn",
        r#"
store: [
    load: SOURCE
    asset: PackageAsset[url: TEXT { asset://wave.vcd }]
    result:
        NotStarted |> HOLD result {
            load |> THEN {
                File/read_stream(
                    file: asset
                    retain_content: False
                )
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let stream_effect = compiled
        .plan
        .effects
        .iter()
        .find(|effect| effect.host_operation == "File/read_stream")
        .unwrap();
    let invocation = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if effect.effect_id == stream_effect.effect_id => Some(effect),
            _ => None,
        })
        .unwrap();
    let chunk_bytes = invocation
        .intent_fields
        .iter()
        .find(|field| field.name == "chunk_bytes")
        .unwrap();
    let PlanRowExpressionNode::Constant { constant_id } =
        row_node(&compiled.plan.row_expressions, chunk_bytes.expression)
    else {
        panic!("defaulted chunk_bytes must lower to a plan constant");
    };
    let Some(boon_plan::PlanConstantValue::Number { value }) = compiled
        .plan
        .constants
        .iter()
        .find(|constant| constant.id == *constant_id)
        .map(|constant| &constant.value)
    else {
        panic!("defaulted chunk_bytes constant must be a Number");
    };
    assert_eq!(value.to_i64_exact().unwrap(), 64 * 1024);
    assert!(
        verify_plan(&compiled.plan)
            .unwrap()
            .checks
            .iter()
            .all(|check| check.pass)
    );
}

#[test]
fn one_effect_result_owner_keeps_one_identity_across_possible_trigger_sources() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "multi-cause-file-stream.bn",
        r#"
store: [
    load_primary: SOURCE
    load_secondary: SOURCE
    selected_name:
        LATEST {
            TEXT { primary.vcd }
            load_primary |> THEN { TEXT { primary.vcd } }
            load_secondary |> THEN { TEXT { secondary.vcd } }
        }
    selected_asset:
        selected_name |> WHEN {
            TEXT { primary.vcd } => PackageAsset[url: TEXT { asset://primary.vcd }]
            __ => PackageAsset[url: TEXT { asset://secondary.vcd }]
        }
    result:
        NotStarted |> HOLD result {
            selected_name |> THEN {
                File/read_stream(
                    file: selected_asset
                    chunk_bytes: 4096
                    retain_content: True
                )
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let stream = compiled
        .plan
        .effects
        .iter()
        .find(|effect| effect.host_operation == "File/read_stream")
        .unwrap();
    let invocations = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if effect.effect_id == stream.effect_id => Some((op, effect)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [(invocation_op, invocation)] = invocations.as_slice() else {
        panic!("one effect call site must lower once, got {invocations:#?}");
    };
    assert!(matches!(
        &invocation_op.kind,
        PlanOpKind::StateUpdate {
            trigger: ValueRef::State(_),
            ..
        }
    ));
    assert_eq!(invocation.effect_id, stream.effect_id);
    assert!(
        verify_plan(&compiled.plan)
            .unwrap()
            .checks
            .iter()
            .all(|check| check.pass)
    );
}

#[test]
fn multiline_tagged_record_fields_lower_to_executable_row_expressions() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "multiline-tagged-record-field.bn",
        r#"
store: [
    asset:
        PackageAsset[url: TEXT { asset://files/example.vcd }]
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let field = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.asset")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|field| field.parse::<usize>().ok())
        .map(boon_plan::FieldId)
        .expect("asset field");
    let operation = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::Field(field)))
        .expect("asset operation");
    let (
        Some(ValueRef::Field(output)),
        PlanOpKind::DerivedValue {
            expression: Some(PlanDerivedExpression::RowExpression { expression }),
            ..
        },
    ) = (&operation.output, &operation.kind)
    else {
        panic!("asset operation was not executable tagged data: {operation:#?}");
    };
    assert_eq!(*output, field);
    assert!(matches!(
        row_node(&compiled.plan.row_expressions, *expression),
        PlanRowExpressionNode::TaggedObject { tag, .. } if tag == "PackageAsset"
    ));
}

#[test]
fn tagged_host_effect_intent_lowers_as_a_typed_row_expression() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "tagged-host-effect-intent.bn",
        r#"
store: [
    load: SOURCE
    asset: PackageAsset[url: TEXT { asset://wave.vcd }]
    result:
        NotStarted |> HOLD result {
            load |> THEN {
                File/read_stream(
                    file: asset
                    chunk_bytes: 4096
                    retain_content: True
                )
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let asset = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.asset")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|field| field.parse::<usize>().ok())
        .map(FieldId)
        .expect("asset field");
    let expression = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| {
            (op.output == Some(ValueRef::Field(asset)))
                .then_some(&op.kind)
                .and_then(|kind| match kind {
                    PlanOpKind::DerivedValue {
                        expression: Some(PlanDerivedExpression::RowExpression { expression }),
                        ..
                    } => Some(expression),
                    _ => None,
                })
        })
        .expect("asset derived expression");
    let PlanRowExpressionNode::TaggedObject { tag, fields } =
        row_node(&compiled.plan.row_expressions, *expression)
    else {
        panic!("PackageAsset must lower as a generic tagged-object expression");
    };
    assert_eq!(tag, "PackageAsset");
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "url");
    assert!(
        verify_plan(&compiled.plan)
            .unwrap()
            .checks
            .iter()
            .all(|check| check.pass)
    );
}

#[test]
fn multiline_when_arm_constructor_lowers_inline_select_arms() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "multiline-when-arm-constructor.bn",
        r#"
store: [
    toggle: SOURCE
    mode:
        Dark |> HOLD mode {
            toggle |> THEN { Light }
        }
]

document: Document/new(
    root: store.mode |> WHEN {
        Dark => Element/label(
            element: []
            style: [background: [color: store.mode |> WHEN {
                Dark => TEXT { #101820 }
                Light => TEXT { #f4f7fb }
            }]]
            label: Element/text(element: [], style: [], text: TEXT { dark })
        )
        __ => Element/label(element: [], style: [], label: TEXT { light })
    }
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled.plan.document.as_ref().expect("document plan");
    assert!(document.expressions.iter().any(|expression| {
        matches!(&expression.op, DocumentExprOp::Select { arms, .. } if arms.len() == 2)
    }));
    assert!(
        verify_plan(&compiled.plan)
            .unwrap()
            .checks
            .iter()
            .all(|check| check.pass)
    );
}

#[test]
fn effect_invocation_identity_tracks_the_direct_result_state() {
    let original = compile_fixture_source_text_to_machine_plan(
        "typed-passkey-effects.bn",
        typed_passkey_effect_source(),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let rerouted_source =
        typed_passkey_effect_source().replace("registration_result", "registration_result_alt");
    let rerouted = compile_fixture_source_text_to_machine_plan(
        "typed-passkey-effects-rerouted.bn",
        &rerouted_source,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert_ne!(
        original.plan.persistence.schema_hash, rerouted.plan.persistence.schema_hash,
        "changing the direct result state must change durable compatibility"
    );
}

#[test]
fn function_call_match_input_in_hold_update_is_statically_scheduled() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "call-derived-match-input.bn",
        r#"
store: [
    lifecycle: [started: SOURCE]
    workspace_id:
        Text/empty() |> HOLD workspace_id {
            store.lifecycle.started |> THEN {
                Text/is_empty(input: workspace_id) |> WHEN {
                    True => store.lifecycle.started.workspace_id
                    False => workspace_id
                }
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| {
            let PlanOpKind::StateUpdate {
                value: Some(value), ..
            } = &op.kind
            else {
                return false;
            };
            let PlanRowExpressionNode::Select { input, .. } =
                row_node(&compiled.plan.row_expressions, *value)
            else {
                return false;
            };
            matches!(
                row_node(&compiled.plan.row_expressions, *input),
                PlanRowExpressionNode::TextIsEmpty { .. }
            )
        })
        .unwrap();
    let PlanOpKind::StateUpdate {
        value: Some(value), ..
    } = &op.kind
    else {
        unreachable!();
    };
    let mut inputs = Vec::new();
    compiled
        .plan
        .row_expressions
        .visit_value_refs(*value, &mut |input| inputs.push(input.clone()))
        .unwrap();
    assert!(
        inputs
            .iter()
            .any(|input| matches!(input, ValueRef::State(_)))
    );
    assert!(
        inputs.iter().any(|input| {
            matches!(
                input,
                ValueRef::SourcePayload {
                    field: boon_plan::SourcePayloadField::Named(name),
                    ..
                } if name == "workspace_id"
            )
        }),
        "state update lost the exact source payload read: inputs={inputs:#?}; value={value:#?}"
    );
    assert!(
        compiled.plan.capability_summary.cpu_plan_executor_complete,
        "call-derived match op must be CPU-executable: {op:?}; unresolved={:?}",
        compiled.plan.debug_map.unresolved_executable_refs,
    );
    let verification = verify_plan(&compiled.plan).unwrap();
    assert!(
        verification.checks.iter().all(|check| check.pass),
        "verification failures: {:?}",
        verification
            .checks
            .iter()
            .filter(|check| !check.pass)
            .collect::<Vec<_>>()
    );
}

#[test]
fn indexed_list_persistence_covers_every_executor_authority_field() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "todomvc-authority-coverage.bn",
        include_str!("../../../examples/todomvc.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let list_slot = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| {
            compiled.plan.debug_map.list_slots.iter().any(|entry| {
                entry.id == format!("list:{}", slot.list_id.0) && entry.label == "store.todos"
            })
        })
        .expect("todos list slot");
    let list_memory = compiled
        .plan
        .persistence
        .lists
        .iter()
        .find(|memory| memory.runtime_slot == list_slot.id)
        .expect("todos persistence memory");
    let stable_fields = list_memory
        .row_fields
        .iter()
        .filter_map(|field| field.runtime_field_id)
        .collect::<std::collections::BTreeSet<_>>();
    let initial_fields = list_slot
        .initial_rows
        .iter()
        .flat_map(|row| &row.fields)
        .filter_map(|field| field.field_id)
        .collect::<std::collections::BTreeSet<_>>();

    assert!(initial_fields.is_subset(&stable_fields));
    assert!(
        list_memory
            .row_fields
            .iter()
            .any(|field| field.semantic_path == "store.todos.@authority:title")
    );
    assert!(
        list_memory
            .row_fields
            .iter()
            .any(|field| field.semantic_path == "store.todos.@authority:completed")
    );
    assert!(
        verify_plan(&compiled.plan)
            .unwrap()
            .checks
            .iter()
            .any(|check| {
                check.id == "list-authority-fields-have-stable-persistence-leaves" && check.pass
            })
    );
}

fn persistence_ids_by_semantic_path(
    plan: &boon_plan::MachinePlan,
) -> std::collections::BTreeMap<(MemoryKind, String), MemoryId> {
    plan.persistence
        .memory
        .iter()
        .map(|memory| {
            (
                (memory.kind, memory.semantic_path.clone()),
                memory.memory_id,
            )
        })
        .chain(plan.persistence.lists.iter().map(|list| {
            (
                (MemoryKind::List, list.semantic_path.clone()),
                list.memory_id,
            )
        }))
        .collect()
}

#[test]
fn compiler_persistence_metadata_verifies_and_has_no_invented_migrations() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "counter-display-label.bn",
        include_str!("../../../examples/counter.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let verification = verify_plan(&compiled.plan).unwrap();

    assert!(
        verification
            .checks
            .iter()
            .filter(|check| {
                check.id.starts_with("application-")
                    || check.id.starts_with("persistence-")
                    || check.id.starts_with("migration-")
            })
            .all(|check| check.pass),
        "{:#?}",
        verification.checks
    );
    assert!(compiled.plan.persistence.migration_edges.is_empty());
    assert_eq!(
        compiled.plan.application.identity,
        ApplicationIdentity::compiler_default()
    );
}

#[test]
fn persistence_identity_is_stable_across_formatting_and_display_labels() {
    let source = include_str!("../../../examples/counter.bn");
    let formatted = format!("\n\n\n{source}\n\n");
    let first = compile_fixture_source_text_to_machine_plan(
        "first-display-label.bn",
        source,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let second = compile_fixture_source_text_to_machine_plan(
        "renamed-display-label.bn",
        &formatted,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(
        persistence_ids_by_semantic_path(&first.plan),
        persistence_ids_by_semantic_path(&second.plan)
    );
    assert_eq!(
        first.plan.persistence.schema_hash,
        second.plan.persistence.schema_hash
    );
}

#[test]
fn persistence_identity_is_stable_across_state_and_list_sibling_reordering() {
    let first = r#"
store: [
    events: [
        alpha: SOURCE
        beta: SOURCE
    ]
    alpha:
        0 |> HOLD alpha {
            events.alpha |> THEN { alpha + 1 }
        }
    beta:
        0 |> HOLD beta {
            events.beta |> THEN { beta + 1 }
        }
    primary: LIST {
        [label: TEXT { primary }]
    }
    secondary: LIST {
        [label: TEXT { secondary }]
    }
]
"#;
    let reordered = r#"
store: [
    events: [
        beta: SOURCE
        alpha: SOURCE
    ]
    secondary: LIST {
        [label: TEXT { secondary }]
    }
    beta:
        0 |> HOLD beta {
            events.beta |> THEN { beta + 1 }
        }
    primary: LIST {
        [label: TEXT { primary }]
    }
    alpha:
        0 |> HOLD alpha {
            events.alpha |> THEN { alpha + 1 }
        }
]
"#;
    let first = compile_fixture_source_text_to_machine_plan(
        "ordered.bn",
        first,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let reordered = compile_fixture_source_text_to_machine_plan(
        "reordered.bn",
        reordered,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(
        persistence_ids_by_semantic_path(&first.plan),
        persistence_ids_by_semantic_path(&reordered.plan)
    );
    assert_eq!(
        first.plan.persistence.schema_hash,
        reordered.plan.persistence.schema_hash
    );
}

#[test]
fn memory_identity_excludes_defaults_and_recursive_type_fingerprints() {
    let number = r#"
events: SOURCE
value:
    0 |> HOLD value {
        events |> THEN { 1 }
    }
"#;
    let text = r#"
events: SOURCE
value:
    TEXT { zero } |> HOLD value {
        events |> THEN { TEXT { one } }
    }
"#;
    let number = compile_fixture_source_text_to_machine_plan(
        "number-default.bn",
        number,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let text = compile_fixture_source_text_to_machine_plan(
        "text-default.bn",
        text,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let number_memory = &number.plan.persistence.memory[0];
    let text_memory = &text.plan.persistence.memory[0];
    assert_eq!(number_memory.semantic_path, text_memory.semantic_path);
    assert_eq!(number_memory.memory_id, text_memory.memory_id);
    assert_ne!(number_memory.type_fingerprint, text_memory.type_fingerprint);
}

#[test]
fn identity_aware_compiler_api_uses_host_identity_without_changing_memory_ids() {
    let source = include_str!("../../../examples/counter.bn");
    let first_identity = ApplicationIdentity::new("dev.boon.counter", "alice", "test");
    let second_identity = ApplicationIdentity::new("dev.boon.counter", "bob", "test");
    let first = compile_fixture_source_text_to_machine_plan_with_identity(
        "counter-one.bn",
        source,
        TargetProfile::SoftwareDefault,
        first_identity.clone(),
    )
    .unwrap();
    let second = compile_fixture_source_text_to_machine_plan_with_identity(
        "counter-two.bn",
        source,
        TargetProfile::SoftwareDefault,
        second_identity.clone(),
    )
    .unwrap();

    assert_eq!(first.plan.application.identity, first_identity);
    assert_eq!(second.plan.application.identity, second_identity);
    assert_eq!(
        persistence_ids_by_semantic_path(&first.plan),
        persistence_ids_by_semantic_path(&second.plan)
    );
    assert_ne!(
        first.plan.persistence.schema_hash,
        second.plan.persistence.schema_hash
    );
}

#[test]
fn persistence_schema_version_is_an_explicit_compiler_input() {
    let source = include_str!("../../../examples/counter.bn");
    let identity = ApplicationIdentity::new("dev.boon.counter", "migration", "test");
    let v1 = compile_fixture_runtime_source_text_with_persistence_identity(
        "counter-v1.bn",
        source,
        TargetProfile::SoftwareDefault,
        identity.clone(),
        1,
    )
    .unwrap();
    let v2 = compile_fixture_runtime_source_text_with_persistence_identity(
        "counter-v2.bn",
        source,
        TargetProfile::SoftwareDefault,
        identity,
        2,
    )
    .unwrap();

    assert_eq!(v1.plan.persistence.schema_version, 1);
    assert_eq!(v2.plan.persistence.schema_version, 2);
    assert_eq!(
        persistence_ids_by_semantic_path(&v1.plan),
        persistence_ids_by_semantic_path(&v2.plan)
    );
    assert_ne!(
        v1.plan.persistence.schema_hash,
        v2.plan.persistence.schema_hash
    );
}

#[test]
fn compatible_versions_bind_noop_edges_and_inherit_skipped_activation_catalog() {
    let v1_source = "count: 0 |> HOLD count { LATEST {} }";
    let v2_source = "count: 10 |> HOLD count { LATEST {} }";
    let v3_source = "count: 20 |> HOLD count { LATEST {} }";
    let identity = ApplicationIdentity::new("dev.boon.counter", "catalog", "test");
    let v1 = compile_fixture_runtime_source_text_with_persistence_identity(
        "counter-v1.bn",
        v1_source,
        TargetProfile::SoftwareDefault,
        identity.clone(),
        1,
    )
    .unwrap();
    let v1_binding = MigrationPredecessorBinding::from_machine_plan(&v1.plan);
    let v2 = compile_fixture_runtime_source_text_with_persistence_catalog(
        "counter-v2.bn",
        v2_source,
        TargetProfile::SoftwareDefault,
        identity.clone(),
        2,
        std::slice::from_ref(&v1_binding),
    )
    .unwrap();
    let v2_repeat = compile_fixture_runtime_source_text_with_persistence_catalog(
        "counter-v2.bn",
        v2_source,
        TargetProfile::SoftwareDefault,
        identity.clone(),
        2,
        std::slice::from_ref(&v1_binding),
    )
    .unwrap();

    assert_eq!(
        plan_binary(&v2.plan).unwrap(),
        plan_binary(&v2_repeat.plan).unwrap()
    );
    assert_eq!(v2.plan.persistence.migration_recipes.len(), 1);
    assert!(v2.plan.persistence.migration_recipes[0].is_noop());
    assert_eq!(v2.plan.persistence.migration_edges.len(), 1);
    assert_eq!(
        v2.plan.persistence.migration_edges[0].source_schema_hash,
        v1.plan.persistence.schema_hash
    );

    let v2_binding = MigrationPredecessorBinding::from_machine_plan(&v2.plan);
    let v3 = compile_fixture_runtime_source_text_with_persistence_catalog(
        "counter-v3.bn",
        v3_source,
        TargetProfile::SoftwareDefault,
        identity,
        3,
        &[v2_binding],
    )
    .unwrap();

    assert_eq!(v3.plan.persistence.migration_recipes.len(), 1);
    assert!(v3.plan.persistence.migration_recipes[0].is_noop());
    assert_eq!(v3.plan.persistence.migration_edges.len(), 2);
    assert_eq!(
        v3.plan
            .persistence
            .migration_edges
            .iter()
            .map(|edge| (edge.source_schema_version, edge.target_schema_version))
            .collect::<Vec<_>>(),
        vec![(1, 2), (2, 3)]
    );
    assert_eq!(verify_plan(&v3.plan).unwrap().status, "pass");
}

#[test]
fn incompatible_shared_memory_type_requires_drain() {
    let identity = ApplicationIdentity::new("dev.boon.counter", "incompatible", "test");
    let v1 = compile_fixture_runtime_source_text_with_persistence_identity(
        "value-v1.bn",
        "value: 1 |> HOLD value { LATEST {} }",
        TargetProfile::SoftwareDefault,
        identity.clone(),
        1,
    )
    .unwrap();
    let predecessor = MigrationPredecessorBinding::from_machine_plan(&v1.plan);
    let error = compile_fixture_runtime_source_text_with_persistence_catalog(
        "value-v2.bn",
        "value: TEXT { one } |> HOLD value { LATEST {} }",
        TargetProfile::SoftwareDefault,
        identity,
        2,
        &[predecessor],
    )
    .unwrap_err();

    assert!(error.to_string().contains("without DRAIN"), "{error}");
}

#[test]
fn every_versioned_migration_fixture_compiles_as_a_catalog_chain() {
    compile_migration_fixture_chain(
        "counter",
        3,
        ApplicationIdentity::new("dev.boon.counter", "fixture-chain", "test"),
    );
    compile_migration_fixture_chain(
        "todo",
        7,
        ApplicationIdentity::new("dev.boon.todo", "fixture-chain", "test"),
    );
}

#[test]
fn compiler_lowers_when_migration_and_binds_predecessor_without_schema_cycle() {
    let predecessor_source = r#"
completed: False |> HOLD completed { LATEST {} }
"#;
    let source = r#"
completed: False |> HOLD completed { LATEST {} } |> DRAINING
status:
    DRAIN { completed }
    |> WHEN {
        True => Done
        False => Open
    }
    |> HOLD status { LATEST {} }
"#;
    let identity = ApplicationIdentity::new("dev.boon.todo", "migration", "test");
    let predecessor_plan = compile_fixture_runtime_source_text_with_persistence_identity(
        "status-v1.bn",
        predecessor_source,
        TargetProfile::SoftwareDefault,
        identity.clone(),
        1,
    )
    .unwrap();
    let unbound = compile_fixture_runtime_source_text_with_persistence_identity(
        "status-v2.bn",
        source,
        TargetProfile::SoftwareDefault,
        identity.clone(),
        2,
    )
    .unwrap();
    let predecessor = MigrationPredecessorBinding::from_machine_plan(&predecessor_plan.plan);
    let bound = compile_fixture_runtime_source_text_with_persistence_catalog(
        "status-v2.bn",
        source,
        TargetProfile::SoftwareDefault,
        identity,
        2,
        std::slice::from_ref(&predecessor),
    )
    .unwrap();

    assert_eq!(
        unbound.plan.persistence.schema_hash,
        bound.plan.persistence.schema_hash
    );
    assert_eq!(
        unbound.plan.persistence.migration_recipe_hash,
        bound.plan.persistence.migration_recipe_hash
    );
    assert_ne!(
        unbound.plan.persistence.migration_catalog_hash,
        bound.plan.persistence.migration_catalog_hash
    );
    assert_eq!(bound.plan.persistence.migration_recipes.len(), 1);
    assert_eq!(bound.plan.persistence.migration_edges.len(), 1);
    assert_eq!(
        bound.plan.persistence.migration_edges[0].source_schema_hash,
        predecessor.source_schema_hash()
    );
    assert!(
        bound
            .plan
            .persistence
            .memory
            .iter()
            .any(|memory| memory.semantic_path == "status")
    );
    assert!(
        bound
            .plan
            .persistence
            .memory
            .iter()
            .all(|memory| memory.semantic_path != "completed"),
        "DRAINING source authority must not remain in the target schema"
    );

    let transfer = &bound.plan.persistence.migration_recipes[0].transfers[0];
    assert_eq!(transfer.transfer_kind, MigrationTransferKindPlan::Scalar);
    let MigrationTransformPlan::Expression {
        root: MigrationExpressionPlan::Match { arms, .. },
    } = &transfer.transform
    else {
        panic!("WHEN migration must lower to a target-neutral Match: {transfer:#?}");
    };
    assert_eq!(
        arms.iter()
            .map(|arm| arm.pattern.clone())
            .collect::<Vec<_>>(),
        vec![
            boon_plan::PlanRowSelectPattern::Bool { value: false },
            boon_plan::PlanRowSelectPattern::Bool { value: true },
        ]
    );
    assert!(format!("{:?}", bound.plan.regions).find("Drain").is_none());
    assert_eq!(verify_plan(&bound.plan).unwrap().status, "pass");
}

#[test]
fn compiler_lowers_fractional_number_in_migration_expression() {
    let source = r#"
previous: 1 |> HOLD previous { LATEST {} } |> DRAINING
current:
    DRAIN { previous } + 0.5
    |> HOLD current { LATEST {} }
"#;
    let plan = compile_fixture_runtime_source_text_with_persistence_identity(
        "fractional-migration.bn",
        source,
        TargetProfile::SoftwareDefault,
        ApplicationIdentity::new("dev.boon.number", "fractional-migration", "test"),
        2,
    )
    .unwrap()
    .plan;

    let transfer = &plan.persistence.migration_recipes[0].transfers[0];
    let MigrationTransformPlan::Expression {
        root: MigrationExpressionPlan::Infix {
            right, operator, ..
        },
    } = &transfer.transform
    else {
        panic!("fractional migration must lower to an infix expression: {transfer:#?}");
    };
    assert_eq!(operator, "+");
    assert!(matches!(
        right.as_ref(),
        MigrationExpressionPlan::Number { value }
            if *value == "0.5".parse().unwrap()
    ));
}

#[test]
fn migration_recipe_ids_ignore_formatting_sibling_and_record_field_order() {
    let ordered = r#"
left: 1 |> HOLD left { LATEST {} } |> DRAINING
right: 2 |> HOLD right { LATEST {} } |> DRAINING
merged:
    [left: DRAIN { left }, right: DRAIN { right }]
    |> HOLD merged { LATEST {} }
"#;
    let reordered = r#"

right: 2 |> HOLD right { LATEST {} } |> DRAINING

left: 1 |> HOLD left { LATEST {} } |> DRAINING
merged:
    [right: DRAIN { right }, left: DRAIN { left }]
    |> HOLD merged { LATEST {} }

"#;
    let identity = ApplicationIdentity::new("dev.boon.merge", "migration", "test");
    let first = compile_fixture_runtime_source_text_with_persistence_identity(
        "merge-a.bn",
        ordered,
        TargetProfile::SoftwareDefault,
        identity.clone(),
        2,
    )
    .unwrap();
    let second = compile_fixture_runtime_source_text_with_persistence_identity(
        "merge-b.bn",
        reordered,
        TargetProfile::SoftwareDefault,
        identity,
        2,
    )
    .unwrap();

    assert_eq!(
        first.plan.persistence.schema_hash,
        second.plan.persistence.schema_hash
    );
    assert_eq!(
        first.plan.persistence.current_migration_recipe_id,
        second.plan.persistence.current_migration_recipe_id
    );
    let transfer = &first.plan.persistence.migration_recipes[0].transfers[0];
    assert_eq!(
        transfer.inputs.len(),
        2,
        "record merge must retain both DRAIN inputs"
    );
    assert!(matches!(
        transfer.transform,
        MigrationTransformPlan::Expression {
            root: MigrationExpressionPlan::Record { .. }
        }
    ));
}

#[test]
fn compiler_lowers_whole_list_and_indexed_field_migration_recipes() {
    let whole_list = r#"
FUNCTION keep_row(row) {
    [title: TEXT { copied }]
}

todos:
    LIST { [title: TEXT { one }] }
    |> List/map(item, new: keep_row(row: item))
    |> DRAINING

tasks:
    DRAIN { todos }
    |> List/map(item, new: keep_row(row: item))
"#;
    let indexed = r#"
todos:
    LIST { [title: TEXT { one }, text: TEXT { unset }] }
    |> List/map(item, new: new_todo(todo: item))

FUNCTION new_todo(todo) {
    [
        title:
            todo.title |> HOLD title { LATEST {} } |> DRAINING
        text:
            DRAIN { title } |> HOLD text { LATEST {} }
    ]
}
"#;
    let list_plan = compile_fixture_runtime_source_text_with_persistence_identity(
        "list-v2.bn",
        whole_list,
        TargetProfile::SoftwareDefault,
        ApplicationIdentity::new("dev.boon.list", "migration", "test"),
        2,
    )
    .unwrap()
    .plan;
    let indexed_plan = compile_fixture_runtime_source_text_with_persistence_identity(
        "indexed-v2.bn",
        indexed,
        TargetProfile::SoftwareDefault,
        ApplicationIdentity::new("dev.boon.indexed", "migration", "test"),
        2,
    )
    .unwrap()
    .plan;

    let list_transfer = &list_plan.persistence.migration_recipes[0].transfers[0];
    assert_eq!(list_transfer.transfer_kind, MigrationTransferKindPlan::List);
    assert!(list_transfer.indexed_list_owner.is_none());
    assert!(matches!(
        list_transfer.transform,
        MigrationTransformPlan::Identity { .. }
    ));
    let migrated_materializations = list_plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression:
                    Some(PlanDerivedExpression::MaterializeList {
                        target_list,
                        authority_source_list: Some(source_list),
                        ..
                    }),
                ..
            } if target_list != source_list => Some((*target_list, *source_list)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        migrated_materializations,
        vec![(boon_plan::ListId(1), boon_plan::ListId(0))],
        "the migrated target must retain predecessor row identity"
    );
    let indexed_transfer = &indexed_plan.persistence.migration_recipes[0].transfers[0];
    assert_eq!(
        indexed_transfer.transfer_kind,
        MigrationTransferKindPlan::IndexedRowField
    );
    let indexed_owner = indexed_transfer.indexed_list_owner.as_ref().unwrap();
    assert_eq!(
        indexed_owner.memory_id,
        indexed_plan.persistence.lists[0].memory_id
    );
    assert_eq!(
        indexed_owner.memory_id,
        indexed_transfer.destination.memory_id
    );
    assert!(
        indexed_transfer
            .inputs
            .iter()
            .flat_map(|input| &input.leaves)
            .all(|leaf| leaf.memory_id == indexed_owner.memory_id)
    );
    assert!(matches!(
        indexed_transfer.transform,
        MigrationTransformPlan::Identity { .. }
    ));
    let verification = verify_plan(&list_plan).unwrap();
    assert_eq!(
        verification.status, "pass",
        "checks={:?}",
        verification.checks
    );
    let verification = verify_plan(&indexed_plan).unwrap();
    assert_eq!(
        verification.status, "pass",
        "checks={:?}",
        verification.checks
    );
}

#[test]
fn indexed_migrations_reconstruct_untouched_row_defaults() {
    let identity = ApplicationIdentity::new("dev.boon.todo-migration", "migration", "test");
    let compile_stage = |version, path: &str| {
        compile_fixture_runtime_source_text_with_persistence_identity(
            path,
            &fs::read_to_string(example_path(path)).unwrap(),
            TargetProfile::SoftwareDefault,
            identity.clone(),
            version,
        )
        .unwrap()
        .plan
    };
    let v5 = compile_stage(5, "examples/migrations/todo/v5.bn");
    let v6 = compile_stage(6, "examples/migrations/todo/v6.bn");
    let initial_expression = |plan: &MachinePlan, path: &str| {
        let memory = plan
            .persistence
            .memory
            .iter()
            .find(|memory| memory.semantic_path == path)
            .unwrap_or_else(|| {
                panic!(
                    "missing persistence memory `{path}`; available={:?}",
                    plan.persistence
                        .memory
                        .iter()
                        .map(|memory| memory.semantic_path.as_str())
                        .collect::<Vec<_>>()
                )
            });
        plan.storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.id == memory.runtime_slot)
            .and_then(|slot| match &slot.initializer {
                boon_plan::ScalarInitializerPlan::Expression { expression } => Some(*expression),
                boon_plan::ScalarInitializerPlan::Constant { .. } => None,
            })
            .unwrap_or_else(|| panic!("missing row default expression for `{path}`"))
    };

    let v5_text = initial_expression(&v5, "store.tasks.text");
    assert!(
        matches!(
            row_node(&v5.row_expressions, v5_text),
            PlanRowExpressionNode::Field { .. }
        ),
        "V5 text initializer must read the exact predecessor row field: {v5_text:#?}"
    );
    let v6_status = initial_expression(&v6, "store.tasks.status");
    let PlanRowExpressionNode::Select { input, arms } = row_node(&v6.row_expressions, v6_status)
    else {
        panic!("pure indexed migration must compile to a sparse Select default: {v6_status:#?}");
    };
    assert!(matches!(
        row_node(&v6.row_expressions, *input),
        PlanRowExpressionNode::Field { .. }
    ));
    assert_eq!(arms.len(), 2);
    assert!(arms.iter().any(|arm| matches!(
        arm.pattern,
        boon_plan::PlanRowSelectPattern::Bool { value: false }
    )));
    assert!(arms.iter().any(|arm| matches!(
        arm.pattern,
        boon_plan::PlanRowSelectPattern::Bool { value: true }
    )));
}

#[test]
fn compiled_v4_binary_and_hash_are_deterministic() {
    let source = include_str!("../../../examples/counter.bn");
    let first = compile_fixture_source_text_to_machine_plan(
        "counter.bn",
        source,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let second = compile_fixture_source_text_to_machine_plan(
        "counter.bn",
        source,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(
        plan_binary(&first.plan).unwrap(),
        plan_binary(&second.plan).unwrap()
    );
    assert_eq!(
        plan_sha256(&first.plan).unwrap(),
        plan_sha256(&second.plan).unwrap()
    );
}

#[test]
fn anonymous_line_based_state_is_a_compile_diagnostic() {
    let error = compile_fixture_source_text_to_machine_plan(
        "anonymous-state.bn",
        r#"
0 |> HOLD {
    1
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap_err();

    assert!(
        error.to_string().contains("anonymous line-based state"),
        "{error}"
    );
}

#[test]
fn compiler_root_demand_is_sorted_and_unique() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "examples/counter.bn",
        include_str!("../../../examples/counter.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let RootOutputDemand::Selected(field_ids) = compiled.plan.demand.root_derived_outputs else {
        panic!("compiler must encode observed roots as selected demand");
    };

    assert!(field_ids.windows(2).all(|ids| ids[0] < ids[1]));
}

#[test]
fn compiler_root_demand_includes_derived_document_control_flow() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "derived-document-control-flow.bn",
        r#"
store: [
    show_program:
        0 == 1 |> Bool/not()
]

scene:
    store.show_program |> WHEN {
        True => Scene/Element/text(
            element: []
            style: [width: Fill]
            text: TEXT { visible }
        )
        False => NoElement
    }
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let show_program = compiled
        .ir
        .semantic_index
        .fields
        .iter()
        .find(|field| field.path == "store.show_program")
        .map(|field| boon_plan::FieldId(field.id.0))
        .expect("store.show_program semantic field");
    let RootOutputDemand::Selected(fields) = compiled.plan.demand.root_derived_outputs else {
        panic!("document demand must remain sparse");
    };

    assert!(
        fields.contains(&show_program),
        "derived fields used only as output control flow must remain demand-current"
    );
}

#[test]
fn compiler_preserves_empty_selected_demand() {
    let compiled = compile_source_path_to_machine_plan(
        Path::new("../../examples/bytes_length_plan_ops.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(
        compiled.plan.demand.root_derived_outputs,
        RootOutputDemand::Selected(Vec::new())
    );
}

#[test]
fn scoped_list_event_projection_has_a_typed_source_transform() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "scoped-event-projection.bn",
        r#"
store: [
    clear: SOURCE
    active_label:
        TEXT { First } |> HOLD active_label {
            clear |> THEN { TEXT { none } }
        }
    rows:
        LIST {
            [label: TEXT { First }]
        }
        |> List/map(item, new: new_row(item: item))
    visible_rows:
        rows
        |> List/filter(item, if: item.label == active_label)
    row_selected:
        visible_rows
        |> List/map(item, new:
            item.controls.select.event.press |> THEN { item.label }
        )
        |> List/latest()
    selected:
        TEXT { none } |> HOLD selected {
            row_selected
        }
]

FUNCTION new_row(item) {
    [
        controls: [select: SOURCE]
        label: item.label
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let field = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.row_selected")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|field| field.parse::<usize>().ok())
        .map(boon_plan::FieldId)
        .expect("row_selected field");
    let op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::Field(field)))
        .expect("row_selected plan op");

    let PlanOpKind::DerivedValue {
        expression: Some(PlanDerivedExpression::SourceEventTransform { default, arms, .. }),
        ..
    } = &op.kind
    else {
        panic!("row projection must lower to a source-event transform");
    };
    let PlanRowExpressionNode::Constant { constant_id } =
        row_node(&compiled.plan.row_expressions, *default)
    else {
        panic!("event-only list projection must use a typed scalar default");
    };
    assert_eq!(
        compiled.plan.constants[constant_id.0].value,
        boon_plan::PlanConstantValue::Text {
            value: String::new()
        }
    );
    let clear_source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.clear")
        .map(|route| route.source_id)
        .expect("clear source route");
    assert!(
        arms.iter()
            .all(|arm| arm.trigger != ValueRef::Source(clear_source)),
        "a source that only changes list membership must not become a row-event arm: {op:#?}"
    );
    let verification = verify_plan(&compiled.plan).unwrap();
    assert_eq!(
        verification.status,
        "pass",
        "invalid list-event projection plan: {:?}\n{op:#?}",
        verification
            .checks
            .iter()
            .filter(|check| !check.pass)
            .collect::<Vec<_>>()
    );
}

#[test]
fn match_arm_payload_dependencies_do_not_create_untyped_source_arms() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "match-arm-sampled-payload.bn",
        r#"
store: [
    elements: [ready: SOURCE, fire: SOURCE, payload: SOURCE]
    payload_value:
        TEXT { initial } |> HOLD payload_value {
            elements.payload.text
        }
    fingerprint:
        TEXT { request }
        |> Text/concat(with: payload_value, separator: ":")
    request:
        LATEST {
            elements.ready.event.press |> WHEN {
                True => fingerprint
                False => SKIP
            }
            elements.fire.event.press |> THEN { fingerprint }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let field = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.request")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|field| field.parse::<usize>().ok())
        .map(boon_plan::FieldId)
        .expect("request field");
    let op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::Field(field)))
        .expect("request plan op");
    let PlanOpKind::DerivedValue {
        expression: Some(PlanDerivedExpression::SourceEventTransform { arms, .. }),
        ..
    } = &op.kind
    else {
        panic!("request must lower to a typed source-event transform: {op:#?}");
    };
    assert_eq!(arms.len(), 2);

    let source_id = |path: &str| {
        compiled
            .plan
            .source_routes
            .iter()
            .find(|route| route.path == path)
            .map(|route| route.source_id)
            .unwrap_or_else(|| panic!("missing source route `{path}`"))
    };
    let arm_sources = arms
        .iter()
        .map(|arm| match &arm.trigger {
            ValueRef::Source(source) => *source,
            trigger => panic!("request arm has non-source trigger {trigger:?}"),
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        arm_sources,
        BTreeSet::from([
            source_id("store.elements.fire"),
            source_id("store.elements.ready"),
        ])
    );
    assert!(
        !op.inputs
            .contains(&ValueRef::Source(source_id("store.elements.payload"))),
        "sampled payload updates must not invoke the request transform"
    );
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn field_equality_host_effect_guard_is_typed_and_executable() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "field-equality-host-effect-guard.bn",
        r#"
store: [
    start: SOURCE
    replace_request: SOURCE
    request_fingerprint:
        TEXT { current } |> HOLD request_fingerprint {
            replace_request.text
        }
    response_fingerprint: TEXT { current }
    clock_result:
        ClockNotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    random_result:
        RandomNotRequested |> HOLD random_result {
            clock_result |> WHEN {
                WallClockRead => request_fingerprint == response_fingerprint |> WHEN {
                    True => Random/bytes(byte_count: 16)
                    False => SKIP
                }
                __ => SKIP
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let guarded_effect = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if compiled.plan.effects.iter().any(|contract| {
                contract.effect_id == effect.effect_id && contract.host_operation == "Random/bytes"
            }) =>
            {
                Some((op, effect))
            }
            _ => None,
        })
        .expect("typed field-equality host-effect guard");
    let (guarded_op, guarded_effect) = guarded_effect;
    let PlanRowExpressionNode::Select { arms, .. } =
        row_node(&compiled.plan.row_expressions, guarded_effect.gate)
    else {
        panic!("clock-result gate must remain an exact selector");
    };
    assert!(arms.iter().any(|arm| {
        let PlanRowExpressionNode::Select { input, .. } =
            row_node(&compiled.plan.row_expressions, arm.value)
        else {
            return false;
        };
        matches!(
            row_node(&compiled.plan.row_expressions, *input),
            PlanRowExpressionNode::NumberInfix { op, .. } if *op == PlanInfixOp::Equal
        )
    }));
    assert_eq!(guarded_op.unresolved_executable_ref_count, 0);
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn one_effect_result_lane_accepts_trigger_specialized_gates_and_intents() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "trigger-specialized-effect-lane.bn",
        r#"
store: [
    start: SOURCE
    move: SOURCE
    clock_result:
        NotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    random_result:
        NotRequested |> HOLD random_result {
            LATEST {
                clock_result |> WHEN {
                    WallClockRead => Random/bytes(byte_count: 4)
                    __ => SKIP
                }
                move |> THEN {
                    clock_result |> WHEN {
                        WallClockRead => Random/bytes(byte_count: 8)
                        __ => SKIP
                    }
                }
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let invocations = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if compiled.plan.effects.iter().any(|contract| {
                contract.effect_id == effect.effect_id && contract.host_operation == "Random/bytes"
            }) =>
            {
                Some((op, effect))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [first, second] = invocations.as_slice() else {
        panic!("expected two specialized random operations, got {invocations:#?}");
    };
    assert_eq!(first.1.invocation_id, second.1.invocation_id);
    assert_eq!(first.1.owner, second.1.owner);
    assert_ne!(first.0.inputs, second.0.inputs);
    assert_ne!(first.1.intent_fields, second.1.intent_fields);
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn scalar_list_nonempty_host_effect_guard_is_typed_and_executable() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "scalar-list-nonempty-host-effect-guard.bn",
        r#"
store: [
    start: SOURCE
    signal_ids: LIST { TEXT { top.clk } }
    random_result:
        RandomNotRequested |> HOLD random_result {
            start |> THEN {
                signal_ids |> List/is_not_empty() |> WHEN {
                    True => Random/bytes(byte_count: 1)
                    False => SKIP
                }
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let guarded_effect = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if matches!(
                row_node(&compiled.plan.row_expressions, effect.gate),
                PlanRowExpressionNode::Select { input, .. }
                    if matches!(
                        row_node(&compiled.plan.row_expressions, *input),
                        PlanRowExpressionNode::BuiltinCall { function, .. }
                            if *function == PlanRowBuiltin::ListIsNotEmpty
                    )
            ) =>
            {
                Some(op)
            }
            _ => None,
        })
        .expect("typed scalar-list nonempty host-effect guard");
    assert_eq!(guarded_effect.unresolved_executable_ref_count, 0);
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn nested_row_helper_aliases_resolve_to_the_canonical_keyed_row() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "nested-row-helper-alias.bn",
        r#"
store: [
    reset: SOURCE
    seed_rows: LIST {
        [initial: TEXT { first }]
    }
    rows:
        seed_rows
        |> List/map(item, new:
            wrap_row(row: seed_record(seed: item))
        )
]

FUNCTION seed_record(seed) {
    [initial: seed.initial]
}

FUNCTION wrap_row(row) {
    stateful_row(seed: row)
}

FUNCTION stateful_row(seed) {
    [
        initial: seed.initial
        value:
            seed.initial |> HOLD value {
                store.reset |> THEN { seed.initial }
            }
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let value_state = compiled
        .plan
        .debug_map
        .state_slots
        .iter()
        .find(|entry| entry.label.ends_with(".value"))
        .and_then(|entry| entry.id.strip_prefix("state:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(boon_plan::StateId)
        .expect("nested value state");
    let value_slot = compiled
        .plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == value_state)
        .expect("nested value storage slot");
    assert!(value_slot.indexed, "nested row state must be indexed");
    assert!(
        !value_slot.owner.static_owner.is_root() && !value_slot.owner.ancestors.is_empty(),
        "nested row state lost exact owner ancestry: {value_slot:#?}"
    );
    assert_eq!(compiled.plan.commit_plan.unresolved_state_update_count, 0);
    assert_eq!(
        compiled
            .plan
            .capability_summary
            .unresolved_executable_ref_count,
        0
    );
}

#[test]
fn constructor_formal_does_not_bind_to_an_unrelated_row_alias() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "constructor-formal-row-owner.bn",
        r#"
store: [
    unrelated_seed: LIST {
        [label: TEXT { wrong }]
    }
    unrelated_rows:
        unrelated_seed
        |> List/map(item, new: unrelated_row(signal: item))
    source_rows: LIST {
        [signal_id: TEXT { right }, name: TEXT { expected }]
    }
    catalog:
        source_rows
        |> List/map(item, new:
            catalog_row(signal: catalog_record(signal_row: item))
        )
]

FUNCTION unrelated_row(signal) {
    [
        select: SOURCE
        label: signal.label
    ]
}

FUNCTION catalog_record(signal_row) {
    [
        key: signal_row.signal_id
        label: signal_row.name
    ]
}

FUNCTION catalog_row(signal) {
    [
        select: SOURCE
        key: signal.key
        label: signal.label
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let catalog = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.catalog")
        .and_then(|derived| derived.materialized_list_id)
        .map(|list| boon_plan::ListId(list.0))
        .expect("catalog materialized ListId");
    let catalog_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::List(catalog)))
        .expect("catalog materialization operation");
    let PlanOpKind::DerivedValue {
        expression:
            Some(PlanDerivedExpression::MaterializeList {
                target_list,
                fields: materialized_fields,
                expression: materialized_expression,
                ..
            }),
        ..
    } = &catalog_op.kind
    else {
        panic!("catalog must have one authoritative materializer: {catalog_op:#?}");
    };
    assert_eq!(*target_list, catalog);
    let label = *materialized_fields
        .get("label")
        .expect("catalog materialized label field");
    assert!(
        compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .all(|op| op.output != Some(ValueRef::Field(label))),
        "a materialized row field must not have a second indexed writer"
    );
    let PlanDerivedExpression::RowExpression { expression } = materialized_expression.as_ref()
    else {
        panic!("catalog materializer must retain the exact list map");
    };
    let (owner, row_local, source, body) = expect_contextual_map(
        &compiled.plan.row_expressions,
        *expression,
        "catalog materializer",
    );
    let source_rows = compiled
        .ir
        .lists
        .iter()
        .find(|list| list.name.ends_with("source_rows"))
        .expect("source rows list");
    assert!(
        matches!(
            row_node(&compiled.plan.row_expressions, source),
            PlanRowExpressionNode::ListRef { list_id }
                if *list_id == boon_plan::ListId(source_rows.id.0)
        ),
        "catalog map must retain the exact source list"
    );
    let PlanRowExpressionNode::Object { fields } = row_node(&compiled.plan.row_expressions, body)
    else {
        panic!("catalog map must produce a record");
    };
    let label_projection = &fields
        .iter()
        .find(|field| field.name == "label")
        .expect("materialized catalog label")
        .value;
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        *label_projection,
        owner,
        row_local,
        &["name"],
        "catalog constructor label",
    );
    verify_plan(&compiled.plan).expect("constructor formal plan must verify");
}

#[test]
fn row_preserving_list_filters_keep_exact_mapped_field_identity() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "filtered-mapped-row-identity.bn",
        r#"
store: [
    selected_file: TEXT { first.vcd }
    rows:
        LIST {
            [file: TEXT { first.vcd }]
            [file: TEXT { second.vcd }]
        }
        |> List/map(item, new: mapped_row(input: item))
    selected:
        rows
        |> List/filter(item, if: item.file == selected_file)
        |> List/map(item, new: copied_row(input: item))
    continued:
        selected_file |> WHEN {
            TEXT { first.vcd } =>
                selected
                |> List/map(item, new:
                    copied_row(
                        input: item
                    )
                )
            __ => LIST {}
        }
]

FUNCTION mapped_row(input) {
    [file: input.file, select: SOURCE]
}

FUNCTION copied_row(input) {
    [file: input.file]
}

document: Document/new(
    root: Element/label(element: [], label: TEXT { row identity })
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let selected = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.selected")
        .and_then(|derived| derived.materialized_list_id)
        .map(|list| boon_plan::ListId(list.0))
        .expect("selected materialized ListId");
    let selected_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::List(selected)))
        .expect("selected operation");
    let PlanOpKind::DerivedValue {
        expression: Some(expression),
        ..
    } = &selected_op.kind
    else {
        panic!("selected operation lost its typed expression: {selected_op:#?}");
    };
    let expression = match expression {
        PlanDerivedExpression::MaterializeList {
            target_list,
            expression,
            ..
        } => {
            assert_eq!(*target_list, selected);
            expression.as_ref()
        }
        expression => expression,
    };
    let PlanDerivedExpression::RowExpression { expression } = expression else {
        panic!("selected list must lower its filtered map");
    };
    let (owner, row_local, source, body) =
        expect_contextual_map(&compiled.plan.row_expressions, *expression, "selected list");
    let (filter_owner, filter_local, filter_source, predicate) =
        expect_contextual_filter(&compiled.plan.row_expressions, source, "selected source");
    let filter_source_node = row_node(&compiled.plan.row_expressions, filter_source);
    let PlanRowExpressionNode::ListRef {
        list_id: filter_list_id,
    } = filter_source_node
    else {
        panic!("selected filter must retain its typed list source: {filter_source:#?}");
    };
    let PlanRowExpressionNode::NumberInfix { op, left, .. } =
        row_node(&compiled.plan.row_expressions, predicate)
    else {
        panic!("selected filter must retain its typed equality: {predicate:#?}");
    };
    assert_eq!(*op, PlanInfixOp::Equal);
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        *left,
        filter_owner,
        filter_local,
        &["file"],
        "selected filter file",
    );
    let PlanRowExpressionNode::Object { fields } = row_node(&compiled.plan.row_expressions, body)
    else {
        panic!("selected map must produce a record");
    };
    let file_projection = &fields
        .iter()
        .find(|field| field.name == "file")
        .expect("selected file field")
        .value;
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        *file_projection,
        owner,
        row_local,
        &["file"],
        "selected file field",
    );
    assert!(
        compiled
            .plan
            .storage_layout
            .list_slots
            .iter()
            .any(|slot| slot.list_id == *filter_list_id)
    );

    let continued = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.continued")
        .and_then(|derived| derived.materialized_list_id)
        .map(|list| boon_plan::ListId(list.0))
        .expect("continued materialized ListId");
    let continued_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::List(continued)))
        .expect("continued operation");
    let PlanOpKind::DerivedValue {
        expression: Some(expression),
        ..
    } = &continued_op.kind
    else {
        panic!("continued operation lost its typed expression: {continued_op:#?}");
    };
    let expression = match expression {
        PlanDerivedExpression::MaterializeList {
            target_list,
            expression,
            ..
        } => {
            assert_eq!(*target_list, continued);
            expression.as_ref()
        }
        expression => expression,
    };
    let PlanDerivedExpression::RowExpression { expression } = expression else {
        panic!("continued list must lower to a select expression");
    };
    let PlanRowExpressionNode::Select { arms, .. } =
        row_node(&compiled.plan.row_expressions, *expression)
    else {
        panic!("continued list must lower to a select expression");
    };
    let continued_map = &arms
        .iter()
        .find(|arm| {
            matches!(
                row_node(&compiled.plan.row_expressions, arm.value),
                PlanRowExpressionNode::ContextualCollection {
                    operation: PlanContextualOperationKind::Map,
                    ..
                }
            )
        })
        .unwrap_or_else(|| panic!("continued select lost its mapped arm: {arms:#?}"))
        .value;
    assert!(
        arms.iter().any(|arm| matches!(
            row_node(&compiled.plan.row_expressions, arm.value),
            PlanRowExpressionNode::ListLiteral { items } if items.is_empty()
        )),
        "continued select lost its empty fallback arm: {arms:#?}"
    );
    let (continued_owner, continued_local, continued_source, continued_body) =
        expect_contextual_map(
            &compiled.plan.row_expressions,
            *continued_map,
            "continued mapped select arm",
        );
    assert!(
        matches!(
            row_node(&compiled.plan.row_expressions, continued_source),
            PlanRowExpressionNode::ListRef { list_id } if *list_id == selected
        ),
        "continued map must retain the selected list as its exact source"
    );
    let PlanRowExpressionNode::Object { fields } =
        row_node(&compiled.plan.row_expressions, continued_body)
    else {
        panic!("continued map must produce a record: {continued_body:#?}");
    };
    let continued_file = &fields
        .iter()
        .find(|field| field.name == "file")
        .expect("continued file field")
        .value;
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        *continued_file,
        continued_owner,
        continued_local,
        &["file"],
        "continued file field",
    );
}

#[test]
fn root_latest_memory_uses_the_branch_owned_by_each_source() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "source-event-branch-ownership.bn",
        r#"
store: [
    sources: [
        cycle: SOURCE
        reset: SOURCE
    ]
    format:
        LATEST {
            Hexadecimal
            sources.cycle.event.press |> THEN {
                format |> WHEN {
                    Hexadecimal => Binary
                    __ => Hexadecimal
                }
            }
            sources.reset.event.press |> THEN { Hexadecimal }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let state = compiled
        .plan
        .debug_map
        .state_slots
        .iter()
        .find(|field| field.label == "store.format")
        .and_then(|field| field.id.strip_prefix("state:"))
        .and_then(|field| field.parse::<usize>().ok())
        .map(boon_plan::StateId)
        .expect("format state");
    let reset = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.sources.reset")
        .expect("reset source");
    let reset_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| {
            op.output == Some(ValueRef::State(state))
                && op.inputs.contains(&ValueRef::Source(reset.source_id))
        })
        .expect("reset update operation");
    let PlanOpKind::StateUpdate {
        value: Some(value), ..
    } = &reset_op.kind
    else {
        panic!("format reset must lower as a constant state update: {reset_op:#?}");
    };
    let PlanRowExpressionNode::Constant { constant_id } =
        row_node(&compiled.plan.row_expressions, *value)
    else {
        panic!("format reset must lower as a constant state update: {reset_op:#?}");
    };
    let constant = compiled
        .plan
        .constants
        .iter()
        .find(|constant| constant.id == *constant_id)
        .expect("reset constant");
    assert_eq!(
        constant.value,
        boon_plan::PlanConstantValue::Enum {
            value: "Hexadecimal".to_owned()
        }
    );
}

#[test]
fn derived_list_input_wins_over_same_named_list_memory() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "derived-list-ownership.bn",
        r#"
store: [
    sources: [events: SOURCE]
    value: 0 |> HOLD value {
        sources.events |> THEN { value + 1 }
    }
    items: LIST {
        [id: TEXT { a }]
        [id: TEXT { b }]
    }
    selected:
        True |> WHEN {
            True => items |> List/filter(item, if: item.id == TEXT { a })
            False => items
        }
    mapped:
        selected
        |> List/map(item, new: [label: item.id])
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let mapped_list = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.mapped")
        .and_then(|derived| derived.materialized_list_id)
        .map(|list| boon_plan::ListId(list.0))
        .expect("mapped materialized ListId");
    let mapped_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::List(mapped_list)))
        .expect("mapped operation");
    let PlanOpKind::DerivedValue {
        expression:
            Some(PlanDerivedExpression::MaterializeList {
                target_list,
                fields,
                expression,
                ..
            }),
        ..
    } = &mapped_op.kind
    else {
        panic!("mapped must lower as a list map: {mapped_op:#?}");
    };
    assert_eq!(*target_list, mapped_list);
    assert!(fields.contains_key("label"));
    let PlanDerivedExpression::RowExpression { expression } = expression.as_ref() else {
        panic!("mapped list lost its contextual row expression: {expression:#?}");
    };
    let (owner, row_local, source, body) =
        expect_contextual_map(&compiled.plan.row_expressions, *expression, "mapped field");
    let selected_list = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.selected")
        .and_then(|derived| derived.materialized_list_id)
        .map(|list| boon_plan::ListId(list.0))
        .expect("selected materialized ListId");
    assert!(
        matches!(
            row_node(&compiled.plan.row_expressions, source),
            PlanRowExpressionNode::ListRef { list_id } if *list_id == selected_list
        ),
        "mapped field must retain the exact derived-list source"
    );
    let PlanRowExpressionNode::Object { fields } = row_node(&compiled.plan.row_expressions, body)
    else {
        panic!("mapped field must produce a record: {body:#?}");
    };
    let label = &fields
        .iter()
        .find(|field| field.name == "label")
        .expect("mapped label field")
        .value;
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        *label,
        owner,
        row_local,
        &["id"],
        "mapped label field",
    );
}

#[test]
fn filtered_variant_rows_have_one_keyed_root_for_all_consumers() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "filtered-variant-rows.bn",
        r#"
store: [
    load: SOURCE
    asset: PackageAsset[url: TEXT { asset://wave.vcd }]
    file_result:
        NotStarted |> HOLD file_result {
            load |> THEN {
                File/read_stream(file: asset, retain_content: True)
            }
        }
    waveform_result:
        NotStarted |> HOLD waveform_result {
            file_result |> WHEN {
                Finished => file_result.retained |> WHEN {
                    Retained => Wellen/open(content: file_result.retained.content)
                    __ => SKIP
                }
                __ => SKIP
            }
        }
    page:
        NotStarted |> HOLD page {
            waveform_result |> WHEN {
                WaveformOpened => Wellen/hierarchy_page(
                    artifact: waveform_result.artifact
                    request_fingerprint: TEXT { hierarchy:test:0 }
                    offset: 0
                    limit: 32
                )
                __ => SKIP
            }
        }
    start_time:
        waveform_result |> WHEN {
            WaveformOpened => waveform_result.start_time
            __ => 0
        }
    filtered:
        page |> WHEN {
            HierarchyPage =>
                page.rows
                |> List/filter(item, if: item.kind == TEXT { Signal })
            __ => LIST {}
        }
    mapped:
        filtered
        |> List/map(item, new: [label: item.id])
    count: filtered |> List/length()
    mapped_count: mapped |> List/length()
]

document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: TEXT { {store.start_time}:{store.count}:{store.mapped_count} }
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let filtered = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.filtered")
        .expect("filtered derived value");
    assert_eq!(filtered.kind, boon_ir::DerivedValueKind::ListView);
    let filtered_list = filtered
        .materialized_list_id
        .map(|list| boon_plan::ListId(list.0))
        .expect("filtered keyed ListId");
    let producers = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| op.output == Some(ValueRef::List(filtered_list)))
        .collect::<Vec<_>>();
    assert_eq!(producers.len(), 1, "filtered producers: {producers:#?}");
    let PlanOpKind::DerivedValue {
        expression:
            Some(PlanDerivedExpression::MaterializeList {
                target_list,
                authority_source_list,
                fields: materialized_fields,
                value_list_authorities,
                expression,
                ..
            }),
        ..
    } = &producers[0].kind
    else {
        panic!("filtered host-result rows must have one materializer: {producers:#?}");
    };
    assert_eq!(*target_list, filtered_list);
    assert_eq!(*authority_source_list, Some(filtered_list));
    let [value_list_authority] = value_list_authorities.as_slice() else {
        panic!(
            "filtered host-result rows need exactly one canonical value-list authority: \
             {value_list_authorities:#?}"
        );
    };
    assert_eq!(value_list_authority.list_id, filtered_list);
    assert_eq!(&value_list_authority.fields, materialized_fields);
    let mut authority_owner_is_present = false;
    visit_derived_row_nodes(
        &compiled.plan.row_expressions,
        expression,
        &mut |_, node| {
            authority_owner_is_present |= matches!(
                node,
                PlanRowExpressionNode::ContextualCollection { owner, .. }
                    if *owner == value_list_authority.owner
            );
        },
    );
    assert!(
        authority_owner_is_present,
        "value-list authority metadata must identify a contextual owner in the materializer"
    );
    let filtered_slot = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == filtered_list)
        .expect("filtered list storage slot");
    let expected_fields = filtered_slot
        .row_fields
        .iter()
        .filter(|field| field.role.is_value())
        .map(|field| field.name.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        materialized_fields.keys().cloned().collect::<BTreeSet<_>>(),
        expected_fields,
        "a row-preserving filter must retain the complete checked host-result row schema"
    );
    assert!(
        materialized_fields.contains_key("encoding"),
        "the filter lost a valid field that is not referenced by its predicate"
    );
    let consumers = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| op.inputs.contains(&ValueRef::List(filtered_list)))
        .collect::<Vec<_>>();
    assert!(
        consumers.len() >= 2,
        "filtered list must feed map and length consumers: {consumers:#?}"
    );
    assert!(
        verify_plan(&compiled.plan)
            .unwrap()
            .checks
            .iter()
            .all(|check| check.pass)
    );

    let start_time = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.start_time")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .expect("start_time field");
    let expression = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| {
            (op.output == Some(ValueRef::Field(start_time)))
                .then_some(&op.kind)
                .and_then(|kind| match kind {
                    PlanOpKind::DerivedValue {
                        expression: Some(PlanDerivedExpression::RowExpression { expression }),
                        ..
                    } => Some(expression),
                    _ => None,
                })
        })
        .expect("start_time continuous expression");
    let PlanRowExpressionNode::Select { arms, .. } =
        row_node(&compiled.plan.row_expressions, *expression)
    else {
        panic!("start_time is not a typed continuous selector: {expression:#?}");
    };
    let constant_id = arms
        .iter()
        .find_map(|arm| {
            match (
                &arm.pattern,
                row_node(&compiled.plan.row_expressions, arm.value),
            ) {
                (
                    boon_plan::PlanRowSelectPattern::Wildcard,
                    PlanRowExpressionNode::Constant { constant_id },
                ) => Some(constant_id),
                _ => None,
            }
        })
        .expect("start_time wildcard fallback constant");
    assert_eq!(
        compiled
            .ir
            .derived_values
            .iter()
            .find(|value| value.path == "store.start_time")
            .map(|value| &value.kind),
        Some(&boon_ir::DerivedValueKind::Pure)
    );
    let Some(constant) = compiled
        .plan
        .constants
        .iter()
        .find(|constant| constant.id == *constant_id)
    else {
        panic!("start_time fallback references missing constant {constant_id:?}");
    };
    assert!(matches!(
        &constant.value,
        boon_plan::PlanConstantValue::Number { value } if value.get() == 0.0
    ));
}

#[test]
fn nested_filtered_list_does_not_become_its_outer_row_list_authority() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "nested-filtered-list-authority.bn",
        r#"
store: [
    inputs: LIST {
        [
            id: TEXT { one }
            values: LIST {
                [value: 1]
                [value: 2]
            }
        ]
        [
            id: TEXT { two }
            values: LIST {
                [value: 3]
            }
        ]
    }
    rows:
        inputs
        |> List/map(item, new: [
            id: item.id
            visible_values:
                item.values
                |> List/filter(item, if: item.value > 1)
                |> List/map(item, new: [label: item.value])
        ])
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let rows = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.rows")
        .and_then(|derived| derived.materialized_list_id)
        .map(|list| boon_plan::ListId(list.0))
        .expect("outer rows materialized list");
    let materializer = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::List(rows)))
        .expect("outer rows materializer");
    let PlanOpKind::DerivedValue {
        expression:
            Some(PlanDerivedExpression::MaterializeList {
                authority_source_list,
                value_list_authorities,
                ..
            }),
        ..
    } = &materializer.kind
    else {
        panic!("outer rows did not lower to a materialized list: {materializer:#?}");
    };
    assert_eq!(*authority_source_list, None);
    assert!(
        value_list_authorities.is_empty(),
        "nested list operators cannot own the outer row list: {value_list_authorities:#?}"
    );
}

#[test]
fn forwarded_row_resource_members_preserve_source_ownership() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "forwarded-row-resource-members.bn",
        r#"
FUNCTION new_row(input) {
    [
        controls: [remove: SOURCE]
        label: input.label
    ]
}

store: [
    inputs: LIST {
        [label: TEXT { same }]
        [label: TEXT { same }]
    }
    rows:
        inputs
        |> List/map(item, new: new_row(input: item))
    forwarded:
        rows
        |> List/map(item, new: [
            controls: item.controls
            label: item.label
        ])
    consumed:
        forwarded
        |> List/map(item, new: [
            controls: item.controls
            label: item.label
        ])
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let remove = compiled
        .ir
        .sources
        .iter()
        .find(|source| source.path.ends_with(".controls.remove"))
        .expect("row-scoped remove source");
    let forwarded = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.forwarded")
        .and_then(|derived| derived.materialized_list_id)
        .expect("forwarded materialized list");
    let forwarded_locals = compiled
        .ir
        .scope_index
        .locals
        .iter()
        .filter(|local| local.row.is_some_and(|row| row.list == forwarded))
        .collect::<Vec<_>>();
    let forwarded_fields = compiled
        .ir
        .scope_index
        .fields
        .iter()
        .filter(|field| field.row.is_some_and(|row| row.list == forwarded))
        .map(|field| {
            (
                field,
                field.producer.and_then(|producer| {
                    compiled.ir.executable.expressions.get(producer.as_usize())
                }),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        forwarded_locals.iter().any(|local| {
            local.members.iter().any(|member| {
                member.path == ["controls", "remove"]
                    && member.target == boon_ir::ErasedLocalMemberTarget::Source(remove.id)
                    && member.forwarded_from.is_some()
            })
        }),
        "forwarded resource members lost source ownership: locals={forwarded_locals:#?}; \
         fields={forwarded_fields:#?}"
    );
}

#[test]
fn remapped_row_resource_members_replace_filtered_source_ownership() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "remapped-row-resource-members.bn",
        r#"
FUNCTION new_row(input) {
    [
        controls: [remove: SOURCE]
        label: input.label
    ]
}

store: [
    inputs: LIST {
        [label: TEXT { same }]
        [label: TEXT { other }]
    }
    rows:
        inputs
        |> List/map(item, new: new_row(input: item))
    filtered:
        rows
        |> List/filter(item, if: item.label == TEXT { same })
        |> List/map(item, new: new_row(input: item))
    selected:
        filtered
        |> List/map(item, new: item.controls.remove |> THEN { item.label })
        |> List/latest()
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let remove_routes = compiled
        .plan
        .source_routes
        .iter()
        .filter(|route| route.path.ends_with(".controls.remove"))
        .collect::<Vec<_>>();
    assert_eq!(remove_routes.len(), 2, "old and replacement row sources");
    assert_ne!(remove_routes[0].source_id, remove_routes[1].source_id);
}

#[test]
fn inline_typed_host_result_rows_support_contextual_field_reads() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "inline-typed-host-result-rows.bn",
        r#"
store: [
    load: SOURCE
    asset: PackageAsset[url: TEXT { asset://wave.vcd }]
    file_result:
        NotStarted |> HOLD file_result {
            load |> THEN {
                File/read_stream(file: asset, retain_content: True)
            }
        }
    waveform_result:
        NotStarted |> HOLD waveform_result {
            file_result |> WHEN {
                Finished => file_result.retained |> WHEN {
                    Retained => Wellen/open(content: file_result.retained.content)
                    __ => SKIP
                }
                __ => SKIP
            }
        }
    page:
        NotStarted |> HOLD page {
            waveform_result |> WHEN {
                WaveformOpened => Wellen/hierarchy_page(
                    artifact: waveform_result.artifact
                    request_fingerprint: TEXT { hierarchy:test:0 }
                    offset: 0
                    limit: 32
                )
                __ => SKIP
            }
        }
    signal_page:
        NotStarted |> HOLD signal_page {
            page |> WHEN {
                HierarchyPage => Wellen/signal_page(
                    artifact: page.artifact
                    request_fingerprint: TEXT { signal:test:0 }
                    signal_ids: LIST { TEXT { top.clk } }
                    start_time: 0
                    end_time: 100
                    offset: 0
                    max_transitions: 32
                )
                __ => SKIP
            }
        }
    selected:
        signal_page |> WHEN {
            SignalPage =>
                signal_page.signals
                |> List/find(item, if: item.signal_id == TEXT { top.clk })
            __ => NotFound
        }
    hierarchy_selected:
        page |> WHEN {
            HierarchyPage =>
                page.rows
                |> List/find(item, if: item.signal_id == TEXT { top.clk })
            __ => NotFound
        }
    selected_signal_id:
        selected |> WHEN {
            Found[value] => value.signal_id
            NotFound => TEXT { none }
        }
    hierarchy_rows:
        page |> WHEN {
            HierarchyPage => page.rows
            __ => LIST {}
        }
    selected_defaults:
        hierarchy_rows
        |> List/map(item, new:
            select_visible(row: selected_row(signal_row: item))
        )
    selected_visible_items:
        selected_defaults
        |> List/filter(item, if: item.item_kind == VariableRow)
    selected_visible_count: selected_visible_items |> List/length()
    remove_defaults:
        hierarchy_rows
        |> List/map(item, new: remove_default(signal_row: item))
    catalog:
        hierarchy_rows
        |> List/map(item, new: catalog_row(signal_row: item))
]

FUNCTION remove_default(signal_row) {
    [
        signal_id: signal_row.signal_id
        item_kind: VariableRow
        controls: [remove: SOURCE]
    ]
}

FUNCTION catalog_row(signal_row) {
    [
        key: signal_row.signal_id
        selected:
            signal_row.kind == TEXT { Signal } |> HOLD selected {
                LATEST {
                    store.load |> THEN { False }
                    store.remove_defaults
                        |> List/filter(item, if: item.item_kind == VariableRow)
                        |> List/map(item, new:
                            item.controls.remove |> THEN {
                                item.signal_id == signal_row.signal_id |> WHEN {
                                    True => False
                                    False => selected
                                }
                            }
                        )
                        |> List/latest()
                }
            }
    ]
}

FUNCTION selected_row(signal_row) {
    [
        id: signal_row.signal_id
        item_kind: VariableRow
        formatter: Hexadecimal
    ]
}

FUNCTION select_visible(row) {
    row.item_kind |> WHEN {
        VariableRow => stateful_visible(row: row)
        __ => row
    }
}

FUNCTION stateful_visible(row) {
    [
        id: row.id
        item_kind: row.item_kind
        formatter:
            row.formatter |> HOLD formatter {
                store.load |> THEN { formatter }
            }
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
    let captures = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .flat_map(|slot| &slot.row_fields)
        .filter(|field| field.role == PlanListRowFieldRole::Capture)
        .collect::<Vec<_>>();
    let materialization_rows = compiled
        .ir
        .materializations
        .iter()
        .map(|materialization| {
            (
                materialization.id,
                materialization.owner,
                materialization.operation,
                materialization.source_list_id,
                materialization.target_list_id,
            )
        })
        .collect::<Vec<_>>();
    let local_rows = compiled
        .ir
        .scope_index
        .locals
        .iter()
        .map(|local| (local.owner, local.local, local.row, local.captures.len()))
        .collect::<Vec<_>>();
    let indexed_states = compiled
        .ir
        .scope_index
        .bindings
        .iter()
        .filter_map(|binding| match binding.target {
            boon_ir::ErasedBindingTarget::State { row: Some(row), .. } => {
                Some((binding.diagnostic_path.as_str(), binding.static_owner, row))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        captures.len() >= 2,
        "the captured initializer and update values need hidden typed storage: captures={captures:#?}, materializations={materialization_rows:#?}, locals={local_rows:#?}, indexed_states={indexed_states:#?}",
    );
    assert!(
        captures
            .iter()
            .all(|field| field.name.starts_with("@capture/")),
        "capture slots must not use Boon-visible field names: {captures:#?}"
    );
    let durable_fields = compiled
        .plan
        .persistence
        .lists
        .iter()
        .flat_map(|list| &list.row_fields)
        .filter_map(|field| field.runtime_field_id)
        .collect::<BTreeSet<_>>();
    assert!(
        captures
            .iter()
            .all(|capture| !durable_fields.contains(&capture.field_id)),
        "detached closure captures are regenerated runtime storage, not persisted authority"
    );
    assert!(
        compiled
            .ir
            .semantic_index
            .fields
            .iter()
            .all(|field| !field.path.starts_with("@capture/")),
        "capture slots must stay out of the semantic field index"
    );
}

#[test]
fn nested_list_contextual_rows_do_not_reuse_the_parent_row_storage() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "nested-list-contextual-row-storage.bn",
        r#"
store: [
    lanes: LIST {
        [
            id: TEXT { lane-a }
            visible: True
            segments: LIST {
                [signal_id: TEXT { clk }, active: True]
                [signal_id: TEXT { reset }, active: False]
            }
        ]
    }
    visible_lanes:
        lanes
        |> List/filter(item, if: item.visible)
    visible_segments:
        visible_lanes
        |> List/map(item, new:
            item.segments
            |> List/retain(item, if: item.active)
            |> List/map(item, new: item.signal_id)
        )
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn nested_contextual_hold_update_reads_its_ancestor_row_state() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "nested-contextual-hold-update.bn",
        r#"
store: [
    reset: SOURCE
    defaults:
        LIST {
            [id: TEXT { first }]
            [id: TEXT { second }]
        }
        |> List/map(item, new: default_row(row: item))
    rows:
        LIST {
            [id: TEXT { first-row }, selected: False]
            [id: TEXT { second-row }, selected: True]
        }
        |> List/map(item, new: stateful_row(row: item))
]

FUNCTION stateful_row(row) {
    [
        key: row.id
        selected:
            row.selected |> HOLD selected {
                LATEST {
                    store.reset |> THEN { False }
                    store.defaults
                        |> List/filter(item, if: item.kind == VariableRow)
                        |> List/map(item, new:
                            item.controls.remove |> THEN {
                                item.id == row.id |> WHEN {
                                    True => False
                                    False => selected
                                }
                            }
                        )
                        |> List/latest()
                }
            }
    ]
}

FUNCTION default_row(row) {
    [
        id: row.id
        kind: VariableRow
        controls: [remove: SOURCE]
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn nested_list_helper_results_are_specialized_from_the_concrete_row_item() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "nested-list-helper-result-specialization.bn",
        r#"
store: [
    rows: LIST {
        [
            id: TEXT { lane }
            segments: LIST {
                [
                    file: TEXT { wave.vcd }
                    signal_id: TEXT { clk }
                    start_time: 0
                    end_time: 10
                    state: High
                    label: TEXT { 1 }
                    click_time: TEXT { 0 ns }
                ]
            }
        ]
    }
    selected_rows:
        rows
        |> List/map(item, new: selected_row(row: item))
    lanes:
        selected_rows
        |> List/map(item, new: lane_row(row: item))
]

FUNCTION selected_row(row) {
    [
        id: row.id
        segments: row.segments
    ]
}

FUNCTION lane_row(row) {
    [
        id: row.id
        segments: lane_segments(row: row)
    ]
}

FUNCTION lane_segments(row) {
    row.segments
    |> List/retain(item, if: True)
    |> List/map(item, new:
        decorate_segment(segment: project_segment(segment: item))
    )
}

FUNCTION project_segment(segment) {
    [
        file: segment.file
        signal_id: segment.signal_id
        start_time: segment.start_time
        end_time: segment.end_time
        state: segment.state
        raw_label: segment.label
        label: segment.label
        click_time: segment.click_time
        width: 10
        projection_source: RuntimeTimelineMetadata
    ]
}

FUNCTION decorate_segment(segment) {
    [
        file: segment.file
        signal_id: segment.signal_id
        start_time: segment.start_time
        end_time: segment.end_time
        state: segment.state
        raw_label: segment.raw_label
        label: segment.label
        click_time: segment.click_time
        width: segment.width
        projection_source: segment.projection_source
        page_kind: TEXT { waveform_page }
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn derived_list_map_lowers_record_returning_helper() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "derived-list-record-helper.bn",
        r#"
store: [
    mode: Active
    items: LIST {
        [
            id: TEXT { a }
            value: 7
        ]
        [
            id: TEXT { b }
            value: 9
        ]
    }
    mapped:
        items
        |> List/map(item, new: decorate(item: item))
]

FUNCTION decorate(item) {
    [
        label: item.id
        details: [
            value: item.value
            state: store.mode |> WHEN {
                Active => Enabled
                __ => Disabled
            }
        ]
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let mapped = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.mapped")
        .and_then(|derived| derived.materialized_list_id)
        .map(|list| boon_plan::ListId(list.0))
        .expect("mapped materialized ListId");
    let mapped_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::List(mapped)))
        .expect("mapped operation");

    let PlanOpKind::DerivedValue {
        expression:
            Some(PlanDerivedExpression::MaterializeList {
                expression: materialized,
                ..
            }),
        ..
    } = &mapped_op.kind
    else {
        panic!("record-returning helper did not materialize its list: {mapped_op:#?}");
    };
    let PlanDerivedExpression::RowExpression { expression } = materialized.as_ref() else {
        panic!("record-returning helper lost its row expression: {materialized:#?}");
    };
    let (owner, row_local, source, body) = expect_contextual_map(
        &compiled.plan.row_expressions,
        *expression,
        "record-returning helper",
    );
    let items = compiled
        .ir
        .lists
        .iter()
        .find(|list| list.name.ends_with("items"))
        .expect("items list");
    assert!(
        matches!(
            row_node(&compiled.plan.row_expressions, source),
            PlanRowExpressionNode::ListRef { list_id }
                if *list_id == boon_plan::ListId(items.id.0)
        ),
        "record-returning helper must retain the exact items source"
    );
    let PlanRowExpressionNode::Object { fields } = row_node(&compiled.plan.row_expressions, body)
    else {
        panic!("record-returning helper must produce a record: {body:#?}");
    };
    let label = &fields
        .iter()
        .find(|field| field.name == "label")
        .expect("decorated label")
        .value;
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        *label,
        owner,
        row_local,
        &["id"],
        "decorated label",
    );
    let details = fields
        .iter()
        .find(|field| field.name == "details")
        .expect("decorated details")
        .value;
    let PlanRowExpressionNode::Object {
        fields: detail_fields,
    } = row_node(&compiled.plan.row_expressions, details)
    else {
        panic!("decorated details must remain a record: {details:#?}");
    };
    let detail_value = &detail_fields
        .iter()
        .find(|field| field.name == "value")
        .expect("decorated detail value")
        .value;
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        *detail_value,
        owner,
        row_local,
        &["value"],
        "decorated detail value",
    );
}

#[test]
fn derived_list_map_lowers_multiline_helper_pipeline() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "derived-list-pipeline-helper.bn",
        r#"
store: [
    items: LIST {
        [
            id: TEXT { a }
            family: TEXT { kept }
        ]
        [
            id: TEXT { b }
            family: TEXT { skipped }
        ]
    }
    mapped: select_items(items: items)
]

FUNCTION select_items(items) {
    items
        |> List/filter(item, if: item.family == TEXT { kept })
        |> List/map(item, new: [label: item.id])
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let mapped = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.mapped")
        .and_then(|derived| derived.materialized_list_id)
        .map(|list| boon_plan::ListId(list.0))
        .expect("mapped materialized ListId");
    let mapped_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::List(mapped)))
        .expect("mapped operation");

    let PlanOpKind::DerivedValue {
        expression:
            Some(PlanDerivedExpression::MaterializeList {
                target_list,
                fields: materialized_fields,
                expression: materialized,
                ..
            }),
        ..
    } = &mapped_op.kind
    else {
        panic!("multiline helper did not materialize its list: {mapped_op:#?}");
    };
    let mapped_list = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.mapped")
        .and_then(|derived| derived.materialized_list_id)
        .map(|list| boon_plan::ListId(list.0))
        .expect("mapped materialized ListId");
    assert_eq!(*target_list, mapped_list);
    assert!(materialized_fields.contains_key("label"));
    let PlanDerivedExpression::RowExpression { expression } = materialized.as_ref() else {
        panic!("multiline helper lost its row expression: {materialized:#?}");
    };
    let (owner, row_local, source, body) = expect_contextual_map(
        &compiled.plan.row_expressions,
        *expression,
        "multiline helper",
    );
    let (filter_owner, filter_local, filter_source, predicate) = expect_contextual_filter(
        &compiled.plan.row_expressions,
        source,
        "multiline helper filter",
    );
    assert!(
        matches!(
            row_node(&compiled.plan.row_expressions, filter_source),
            PlanRowExpressionNode::ListRef { .. }
        ),
        "multiline helper filter must retain its typed list source: {filter_source:#?}"
    );
    let PlanRowExpressionNode::NumberInfix { op, left, .. } =
        row_node(&compiled.plan.row_expressions, predicate)
    else {
        panic!("multiline helper must retain its typed equality: {predicate:#?}");
    };
    assert_eq!(*op, PlanInfixOp::Equal);
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        *left,
        filter_owner,
        filter_local,
        &["family"],
        "multiline helper family",
    );
    let PlanRowExpressionNode::Object { fields } = row_node(&compiled.plan.row_expressions, body)
    else {
        panic!("multiline helper map must produce a record: {body:#?}");
    };
    let label = &fields
        .iter()
        .find(|field| field.name == "label")
        .expect("multiline helper label")
        .value;
    assert_contextual_local_projection(
        &compiled.plan.row_expressions,
        *label,
        owner,
        row_local,
        &["id"],
        "multiline helper label",
    );
}

#[test]
fn scalar_record_lists_lower_typed_find_and_get_without_list_memory_identity() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "scalar-record-list-lookups.bn",
        r#"
store: [
    request: SOURCE
    found_value:
        request.method |> THEN {
            request.query
            |> List/find(
                item
                if: item.name == TEXT { q }
            )
            |> WHEN {
                Found[value] => value.value
                NotFound => TEXT { missing }
            }
        }
    second_row:
        request.method |> THEN { List/get(list: request.query, index: 1) }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let arm_values = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::SourceEventTransform { arms, .. }),
                ..
            } => Some(arms.iter().map(|arm| &arm.value)),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert!(
        arm_values.iter().any(|value| {
            let PlanRowExpressionNode::Select { input, .. } =
                row_node(&compiled.plan.row_expressions, **value)
            else {
                return false;
            };
            matches!(
                row_node(&compiled.plan.row_expressions, *input),
                PlanRowExpressionNode::ContextualCollection {
                    operation: PlanContextualOperationKind::Find,
                    ..
                }
            )
        }),
        "missing scalar typed List/find: {arm_values:#?}"
    );
    assert!(
        arm_values.iter().any(|value| matches!(
            row_node(&compiled.plan.row_expressions, **value),
            PlanRowExpressionNode::BuiltinCall { function, .. }
                if *function == PlanRowBuiltin::ListGet
        )),
        "missing scalar List/get: {arm_values:#?}"
    );
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
}

#[test]
fn stored_list_find_lowers_a_typed_field_id_indexed_access() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "typed-indexed-find.bn",
        r#"
store: [
    items: LIST {
        [key: TEXT { a }, value: TEXT { A }]
        [key: TEXT { b }, value: TEXT { B }]
    }
    selected:
        items
        |> List/find(item, if: item.key == TEXT { b })
        |> WHEN {
            Found[value] => value.value
            NotFound => TEXT { missing }
        }
]
document: Document/new(
    root: Element/label(element: [], label: store.selected)
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let selected = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.selected")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .unwrap_or_else(|| {
            panic!(
                "selected field id; available fields: {:?}",
                compiled
                    .plan
                    .debug_map
                    .fields
                    .iter()
                    .map(|field| field.label.as_str())
                    .collect::<Vec<_>>()
            )
        });
    let expression = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| {
            (op.output == Some(ValueRef::Field(selected)))
                .then_some(&op.kind)
                .and_then(|kind| match kind {
                    PlanOpKind::DerivedValue {
                        expression: Some(PlanDerivedExpression::RowExpression { expression }),
                        ..
                    } => Some(expression),
                    _ => None,
                })
        })
        .unwrap_or_else(|| {
            panic!(
                "selected typed row expression; selected={selected:?}; ops={:#?}",
                compiled
                    .plan
                    .regions
                    .iter()
                    .flat_map(|region| &region.ops)
                    .filter(|op| op.output == Some(ValueRef::Field(selected)))
                    .collect::<Vec<_>>()
            )
        });
    let PlanRowExpressionNode::Select { input, arms } =
        row_node(&compiled.plan.row_expressions, *expression)
    else {
        panic!("selected find result must lower through WHEN: {expression:#?}");
    };
    let PlanRowExpressionNode::ContextualCollection {
        owner,
        operation: PlanContextualOperationKind::Find,
        source,
        row_local,
        body,
        indexed_access: Some(indexed_access),
        ..
    } = row_node(&compiled.plan.row_expressions, *input)
    else {
        panic!("selected find must carry typed index metadata: {input:#?}");
    };
    let index = compiled
        .plan
        .list_indexes
        .get(indexed_access.index.0)
        .filter(|index| index.id == indexed_access.index)
        .expect("contextual Find typed index");
    let [key] = index.keys.as_slice() else {
        panic!("contextual Find must use one typed key: {index:#?}");
    };
    let PlanListAccessSelection::KeyPrefix { values } = &indexed_access.selection else {
        panic!("contextual Find must use an exact key-prefix seek: {indexed_access:#?}");
    };
    let [expected] = values.as_slice() else {
        panic!("contextual Find must seek one exact value: {values:#?}");
    };
    assert!(matches!(
        row_node(&compiled.plan.row_expressions, *source),
        PlanRowExpressionNode::ListRef { list_id } if *list_id == index.source_list
    ));
    let PlanRowExpressionNode::NumberInfix { op, left, right } =
        row_node(&compiled.plan.row_expressions, *body)
    else {
        panic!("typed find predicate lost equality: {body:#?}");
    };
    assert_eq!(*op, PlanInfixOp::Equal);
    assert_eq!(*right, *expected);
    let PlanRowExpressionNode::ListRowField {
        row: key_row,
        list_id: key_list,
        field: key_field,
    } = row_node(&compiled.plan.row_expressions, key.expression)
    else {
        panic!("typed index key lost its exact row field: {key:#?}");
    };
    assert_eq!(*key_list, index.source_list);
    assert!(matches!(
        row_node(&compiled.plan.row_expressions, *key_row),
        PlanRowExpressionNode::LocalRow {
            owner,
            local,
        } if *owner == key.owner && *local == key.row_local
    ));
    let PlanRowExpressionNode::ListRowField {
        row,
        list_id,
        field,
    } = row_node(&compiled.plan.row_expressions, *left)
    else {
        panic!("typed find predicate lost its exact row field: {left:#?}");
    };
    assert_eq!(*list_id, *key_list);
    assert_eq!(*field, *key_field);
    assert!(matches!(
        row_node(&compiled.plan.row_expressions, *row),
        PlanRowExpressionNode::LocalRow {
            owner: row_owner,
            local,
        } if row_owner == owner && local == row_local
    ));
    let slot = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == index.source_list)
        .expect("stored items list slot");
    let value_field = slot
        .row_fields
        .iter()
        .find(|field| field.name == "value" && field.role.is_value())
        .expect("stored value field")
        .field_id;
    assert!(
        arms.iter().any(|arm| {
            let PlanRowExpressionNode::ListRowField {
                row,
                list_id,
                field,
            } = row_node(&compiled.plan.row_expressions, arm.value)
            else {
                return false;
            };
            if *list_id != index.source_list || *field != value_field {
                return false;
            }
            let PlanRowExpressionNode::ObjectField {
                object,
                field: object_field,
            } = row_node(&compiled.plan.row_expressions, *row)
            else {
                return false;
            };
            object_field == "value"
                && matches!(
                    row_node(&compiled.plan.row_expressions, *object),
                    PlanRowExpressionNode::ContextualCollection {
                        operation: PlanContextualOperationKind::Find,
                        ..
                    }
                )
        }),
        "Found row projection lost its compiler-owned field identity: {arms:#?}"
    );
    assert_eq!(slot.initializer_kind, ListInitializerKind::RecordLiteral);
    assert_eq!(slot.initial_rows.len(), 2);
    assert!(
        compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .all(|op| op.output != Some(ValueRef::List(index.source_list))),
        "a reconstructable literal list must not have a duplicate derived producer"
    );
    assert!(boon_plan::verify_plan(&compiled.plan).is_ok());
}

#[test]
fn cells_scoped_source_routes_publish_complete_structural_owners() {
    let path = example_path("examples/cells.bn");
    let units = compiler_source_units_for_path(&path).unwrap();
    let compiled = compile_runtime_source_units_to_machine_plan_with_persistence_identity(
        "typed-cells-route-indexes.bn",
        &units,
        TargetProfile::SoftwareDefault,
        ApplicationIdentity::new("dev.boon.cells-route-indexes", "test", "local"),
        1,
    )
    .unwrap();
    let evidence = compiled
        .plan
        .source_routes
        .iter()
        .filter(|route| route.scoped)
        .map(|route| {
            (
                route.path.clone(),
                route.scope_id,
                route.owner.static_owner,
                route.owner.ancestors.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert!(!evidence.is_empty());
    assert!(evidence.iter().all(|(_, scope, owner, ancestors)| {
        scope.is_some()
            && !owner.is_root()
            && !ancestors.is_empty()
            && ancestors
                .last()
                .is_some_and(|ancestor| Some(ancestor.scope) == *scope)
    }));
    let verification = boon_plan::verify_plan(&compiled.plan).unwrap();
    assert_eq!(
        verification.status, "pass",
        "Cells structural route evidence: {evidence:#?}\nverification: {verification:#?}"
    );
}

#[test]
fn stored_find_result_preserves_exact_row_provenance() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "stored-typed-find-result.bn",
        r#"
store: [
    items: LIST {
        [key: TEXT { a }, value: TEXT { A }]
        [key: TEXT { b }, value: TEXT { B }]
    }
    found:
        items
        |> List/find(item, if: item.key == TEXT { b })
    selected:
        found |> WHEN {
            Found[value] => value.value
            NotFound => TEXT { missing }
        }
]
document: Document/new(
    root: Element/label(element: [], label: store.selected)
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let found_binding = compiled
        .ir
        .scope_index
        .bindings
        .iter()
        .find(|binding| binding.diagnostic_path == "store.found")
        .expect("stored Find binding");
    let row_value = compiled
        .ir
        .scope_index
        .row_values
        .iter()
        .find(|row| {
            row.expression == found_binding.producer && row.projection == ["value".to_owned()]
        })
        .expect("stored Find Found.value row provenance");
    let items_slot = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == boon_plan::ListId(row_value.row.list.as_usize()))
        .expect("items list slot");
    let value_field = items_slot
        .row_fields
        .iter()
        .find(|field| field.name == "value" && field.role.is_value())
        .expect("items value field")
        .field_id;
    let selected = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.selected")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .expect("store.selected field id");
    let expression = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match (&op.output, &op.kind) {
            (
                Some(ValueRef::Field(field)),
                PlanOpKind::DerivedValue {
                    expression: Some(PlanDerivedExpression::RowExpression { expression }),
                    ..
                },
            ) if *field == selected => Some(expression),
            _ => None,
        })
        .expect("stored Find consumer expression");
    let PlanRowExpressionNode::Select { arms, .. } =
        row_node(&compiled.plan.row_expressions, *expression)
    else {
        panic!("stored Find consumer must lower through WHEN: {expression:#?}");
    };
    assert!(
        arms.iter().any(|arm| {
            let PlanRowExpressionNode::ListRowField {
                row,
                list_id,
                field,
            } = row_node(&compiled.plan.row_expressions, arm.value)
            else {
                return false;
            };
            *list_id == items_slot.list_id
                && *field == value_field
                && matches!(
                    row_node(&compiled.plan.row_expressions, *row),
                    PlanRowExpressionNode::ObjectField { field, .. } if field == "value"
                )
        }),
        "stored Find row projection lost exact identity: {arms:#?}"
    );
    assert!(boon_plan::verify_plan(&compiled.plan).is_ok());
}

#[test]
fn stored_list_get_and_latest_results_preserve_exact_row_provenance() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "stored-list-row-results.bn",
        r#"
store: [
    items: LIST {
        [key: TEXT { a }, value: TEXT { A }]
        [key: TEXT { b }, value: TEXT { B }]
    }
    indexed_row: List/get(list: items, index: 1)
    indexed_value: indexed_row.value
    latest_row: List/latest(list: items)
    latest_value: latest_row.value
]
document: Document/new(
    root: Element/label(element: [], label: store.indexed_value)
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let items_slot = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| {
            compiled.plan.debug_map.list_slots.iter().any(|entry| {
                entry.label == "store.items" && entry.id == format!("list:{}", slot.list_id.0)
            })
        })
        .expect("items list slot");
    let value_field = items_slot
        .row_fields
        .iter()
        .find(|field| field.name == "value" && field.role.is_value())
        .expect("items value field")
        .field_id;
    for path in ["store.indexed_value", "store.latest_value"] {
        let output = compiled
            .plan
            .debug_map
            .fields
            .iter()
            .find(|field| field.label == path)
            .and_then(|field| field.id.strip_prefix("field:"))
            .and_then(|id| id.parse::<usize>().ok())
            .map(FieldId)
            .unwrap_or_else(|| panic!("{path} field id"));
        let expression = compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .find_map(|op| match (&op.output, &op.kind) {
                (
                    Some(ValueRef::Field(field)),
                    PlanOpKind::DerivedValue {
                        expression: Some(PlanDerivedExpression::RowExpression { expression }),
                        ..
                    },
                ) if *field == output => Some(expression),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{path} exact expression"));
        assert!(
            matches!(
                row_node(&compiled.plan.row_expressions, *expression),
                PlanRowExpressionNode::ListRowField {
                    list_id,
                    field,
                    ..
                } if *list_id == items_slot.list_id && *field == value_field
            ),
            "{path} lost exact stored row identity: {expression:#?}"
        );
    }
    assert!(boon_plan::verify_plan(&compiled.plan).is_ok());
}

#[test]
fn stored_row_value_rejects_mixed_list_ownership() {
    let error = compile_fixture_source_text_to_machine_plan(
        "mixed-stored-row-owner.bn",
        r#"
store: [
    choose_left: True
    left: LIST { [value: TEXT { left }] }
    right: LIST { [value: TEXT { right }] }
    selected_row:
        choose_left |> WHILE {
            True => List/get(list: left, index: 0)
            False => List/get(list: right, index: 0)
        }
    selected_value: selected_row.value
]
document: Document/new(
    root: Element/label(element: [], label: store.selected_value)
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("conflicting owners") || error.contains("multiple exact row owners"),
        "mixed keyed row owners must fail explicitly: {error}"
    );
}

#[test]
fn retained_document_can_consume_an_inline_typed_find() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "retained-inline-typed-find.bn",
        r#"
store: [
    items: LIST {
        [key: TEXT { a }, value: TEXT { A }]
        [key: TEXT { b }, value: TEXT { B }]
    }
]

document: Document/new(
    root: Element/label(
        element: []
        label:
            store.items
            |> List/find(item, if: item.key == TEXT { b })
            |> WHEN {
                Found[value] => value.value
                NotFound => TEXT { missing }
            }
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let document = compiled.plan.document.as_ref().unwrap();
    let runtime_find = document
        .expressions
        .iter()
        .find_map(|expression| match &expression.op {
            DocumentExprOp::RuntimeExpression {
                expression: runtime_expression,
                bindings,
            } if matches!(
                row_node(&compiled.plan.row_expressions, *runtime_expression),
                PlanRowExpressionNode::ContextualCollection {
                    operation: PlanContextualOperationKind::Find,
                    indexed_access: Some(_),
                    ..
                }
            ) =>
            {
                Some((*runtime_expression, bindings))
            }
            _ => None,
        })
        .expect("inline Find must use one indexed machine runtime expression");
    assert!(runtime_find.1.is_empty());
    assert!(document.materializations.is_empty());
    assert!(boon_plan::verify_plan(&compiled.plan).is_ok());
}

#[test]
fn document_ids_are_stable_across_identical_compilation() {
    let path = example_path("examples/counter.bn");
    let first = compile_source_path_to_machine_plan(&path, TargetProfile::SoftwareDefault).unwrap();
    let second =
        compile_source_path_to_machine_plan(&path, TargetProfile::SoftwareDefault).unwrap();

    assert_eq!(first.plan.document, second.plan.document);
    assert_eq!(
        plan_sha256(&first.plan).unwrap(),
        plan_sha256(&second.plan).unwrap()
    );
}

#[test]
fn document_record_helper_ignores_nested_conditional_delimiters() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "document-style-helper.bn",
        r#"
store: [mode: Dark]

FUNCTION divider_style() {
    [
        width: 4
        height: Fill
        background: [color: store.mode |> WHEN {
            Dark => TEXT { #25344f }
            Light => TEXT { #c9d7ea }
        }]
        __hover_gloss: 0.02
    ]
}

document: Document/new(
    root: Element/container(
        element: []
        style: divider_style()
        child: Element/label(element: [], style: [], label: TEXT { Divider })
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled.plan.document.as_ref().unwrap();

    assert!(document.expressions.iter().any(|expression| {
        let DocumentExprOp::Record { fields } = &expression.op else {
            return false;
        };
        let names = fields
            .iter()
            .filter_map(|field| field.name)
            .map(|name| document.names[name.0].as_str())
            .collect::<Vec<_>>();
        names == ["width", "height", "background", "__hover_gloss"]
    }));
}

#[test]
fn cross_module_document_call_lowers_typed_global_record_with_exact_demand() {
    let units = [
        CompilerSourceUnit {
            path: "ProfilePage.bn".to_owned(),
            source: r#"
FUNCTION render(profile) {
    Scene/Element/stripe(
        element: []
        direction: Column
        style: [width: Fill]
        items: LIST {
            Scene/Element/text(element: [], style: [width: Fill], text: profile.name)
            Scene/Element/stripe(
                element: []
                direction: Column
                style: [width: Fill]
                items: profile.projects
                    |> List/map(item, new: project_row(project: item))
            )
        }
    )
}

FUNCTION project_row(project) {
    Scene/Element/text(element: [], style: [width: Fill], text: project.title)
}

"#
            .to_owned(),
        },
        CompilerSourceUnit {
            path: "RUN.bn".to_owned(),
            source: r#"
profile: [
    name: TEXT { Your name }
    projects: LIST {
        [title: TEXT { First project }]
    }
]

scene: ProfilePage/render(profile: profile)
"#
            .to_owned(),
        },
    ];

    let compiled =
        compile_source_units_to_machine_plan("RUN.bn", &units, TargetProfile::SoftwareDefault)
            .unwrap();
    let document = compiled.plan.document.as_ref().unwrap();
    assert!(matches!(
        document.expressions[document.root.expression.0].op,
        DocumentExprOp::Constructor { .. }
    ));
    let demanded = match &compiled.plan.demand.root_derived_outputs {
        RootOutputDemand::Selected(fields) => fields.iter().copied().collect::<BTreeSet<_>>(),
        RootOutputDemand::All => panic!("document demand must remain sparse"),
    };
    let name_field = compiled
        .ir
        .semantic_index
        .fields
        .iter()
        .find(|field| field.path == "profile.name")
        .map(|field| boon_plan::FieldId(field.id.0))
        .expect("profile.name field");
    let projects_list = compiled
        .ir
        .lists
        .iter()
        .find(|list| list.name == "projects" || list.name == "profile.projects")
        .map(|list| boon_plan::ListId(list.id.0))
        .expect("profile.projects list");

    assert!(demanded.contains(&name_field));
    assert!(document.expressions.iter().any(|expression| matches!(
        expression.op,
        DocumentExprOp::Read {
            read: DocumentRead::Field { field },
        } if field == name_field
    )));
    assert!(
        document
            .materializations
            .iter()
            .any(|materialization| matches!(
                materialization.source,
                boon_plan::DocumentMaterializationSource::List { list } if list == projects_list
            )),
        "profile.projects did not remain the typed materialization source: {:#?}",
        document.materializations
    );
}

#[test]
fn text_interpolation_is_checked_and_erased_before_document_lowering() {
    let fixtures = [
        (
            "parameter",
            r#"
store: [
    count: 7
]

FUNCTION value_label(value) {
    Element/label(element: [], style: [], label: TEXT { value={value} })
}

document: Document/new(root: value_label(value: store.count))
"#,
        ),
        (
            "passed",
            r#"
store: [count: 7]

FUNCTION passed_label() {
    Element/label(element: [], style: [], label: TEXT { passed={PASSED.store.count} })
}

document: Document/new(root: passed_label(PASS: [store: store]))
"#,
        ),
        (
            "row",
            r#"
store: [
    rows: LIST {
        [name: TEXT { Alpha }]
        [name: TEXT { Beta }]
    }
]

document: Document/new(
    root: Element/stripe(
        element: []
        direction: Column
        style: []
        items: store.rows
            |> List/map(item, new:
                Element/label(element: [], style: [], label: TEXT { row={item.name} })
            )
    )
)
"#,
        ),
    ];

    for (name, source) in fixtures {
        let path = format!("typed-text-interpolation-{name}.bn");
        let parsed = boon_parser::parse_source(&path, source).unwrap();
        let report = boon_typecheck::check(&parsed);
        assert!(
            report.diagnostics.is_empty(),
            "{name} text interpolation did not typecheck: {:#?}",
            report.diagnostics
        );
        let compiled = compile_fixture_source_text_to_machine_plan(
            &path,
            source,
            TargetProfile::SoftwareDefault,
        )
        .unwrap();

        let dynamic_expressions = compiled
            .ir
            .executable
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                boon_ir::ExecutableExpressionKind::TextTemplate { segments } => Some(segments),
                _ => None,
            })
            .flat_map(|segments| segments.iter())
            .filter_map(|segment| match segment {
                boon_ir::ExecutableTextSegment::Static { .. } => None,
                boon_ir::ExecutableTextSegment::Dynamic { value } => Some(*value),
            })
            .collect::<Vec<_>>();
        assert!(!dynamic_expressions.is_empty(), "{name}");
        for expression in dynamic_expressions {
            let kind = &compiled.ir.executable.expressions[expression.as_usize()].kind;
            if matches!(
                kind,
                boon_ir::ExecutableExpressionKind::CanonicalRead { .. }
            ) {
                assert!(compiled.ir.scope_index.reads.iter().any(|read| {
                    read.expression == expression
                        && !matches!(read.target, boon_ir::ErasedReadTarget::ExternalValue { .. })
                }));
            }
            assert!(!matches!(
                kind,
                boon_ir::ExecutableExpressionKind::ExternalRead { .. }
            ));
        }

        let document = compiled.plan.document.as_ref().expect("document plan");
        let dynamic_reads = document
            .expressions
            .iter()
            .filter_map(|expression| match &expression.op {
                DocumentExprOp::TextTemplate { segments } => Some(segments),
                _ => None,
            })
            .flat_map(|segments| segments.iter())
            .filter_map(|segment| match segment {
                DocumentTextSegment::Static { .. } => None,
                DocumentTextSegment::Dynamic { value } => Some(*value),
            })
            .map(|value| &document.expressions[value.0].op)
            .collect::<Vec<_>>();
        assert_eq!(dynamic_reads.len(), 1, "{name}");
        assert!(
            matches!(
                dynamic_reads[0],
                DocumentExprOp::Read {
                    read: DocumentRead::Field { .. }
                        | DocumentRead::Parameter { .. }
                        | DocumentRead::Row { .. }
                } | DocumentExprOp::Project { .. }
                    | DocumentExprOp::Constant { .. }
            ),
            "{name}: {:#?}",
            dynamic_reads[0]
        );
    }

    let implementation = include_str!("document_executable_backend.rs");
    assert!(!implementation.contains("compile_named_path"));
    assert!(!implementation.contains("canonical_root_exists"));
}

#[test]
fn document_rejects_transient_source_payload_reads_instead_of_rendering_null() {
    let error = compile_fixture_source_text_to_machine_plan(
        "document-transient-source-payload.bn",
        r#"
store: [input: SOURCE]

document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: store.input.text
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("retain the event value in HOLD before rendering it"),
        "unexpected document source-payload error: {error}"
    );
}

#[test]
fn retained_document_projects_passed_records_without_compiling_unused_event_fields() {
    let units = [
        CompilerSourceUnit {
            path: "ProjectionView.bn".to_owned(),
            source: r#"
FUNCTION inner_label() {
    Element/label(
        element: []
        style: []
        label: PASSED.store.safe
    )
}

FUNCTION outer_label() {
    inner_label()
}
"#
            .to_owned(),
        },
        CompilerSourceUnit {
            path: "RUN.bn".to_owned(),
            source: r#"
store: [
    input: SOURCE
    safe: TEXT { ready }
    transient: input.text
]

document: Document/new(
    root: ProjectionView/outer_label(PASS: [store: store])
)
"#
            .to_owned(),
        },
    ];
    let compiled =
        compile_source_units_to_machine_plan("RUN.bn", &units, TargetProfile::SoftwareDefault)
            .unwrap();

    let document = compiled.plan.document.as_ref().expect("document plan");
    assert!(
        document.expressions.iter().all(|expression| !matches!(
            &expression.op,
            DocumentExprOp::Read {
                read: DocumentRead::Source { .. }
            }
        )),
        "unused source fields leaked into retained expressions: {:#?}",
        document.expressions
    );
    assert!(boon_plan::verify_plan(&compiled.plan).is_ok());
}

#[test]
fn document_row_alias_arguments_remain_rows_and_selects_follow_dynamic_inputs() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "document-row-argument.bn",
        r#"
store: [
    rows:
        LIST {
            [title: TEXT { First }, kind: First]
            [title: TEXT { Second }, kind: Second]
        }
        |> List/map(item, new: new_row(title: item.title, kind: item.kind))
]

FUNCTION new_row(title, kind) {
    BLOCK {
        row_title: title
        row_kind: kind
        [
            controls: [select: SOURCE]
            selected:
                False |> HOLD selected {
                    controls.select |> THEN { True }
                }
            title: row_title
            kind: row_kind
        ]
    }
}

FUNCTION render_row(row) {
    render_title(row: row)
}

FUNCTION render_title(row) {
    Element/label(
        element: []
        style: merge_style(
            base: [width: 200]
            extra: conditional_style(kind: row.kind)
        )
        label: row.kind |> WHEN {
            First => TEXT { First row }
            Second => TEXT { Second row }
        }
    )
}

FUNCTION merge_style(base, extra) {
    [
        ...base
        ...extra
    ]
}

FUNCTION conditional_style(kind) {
    kind |> WHEN {
        Compact => [height: 20]
        __ => BLOCK {
            height: 40
            [height: height]
        }
    }
}

document: Document/new(
    root: Element/stripe(
        element: []
        direction: Column
        style: []
        items: store.rows
            |> List/map(item, new: render_row(row: item))
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled.plan.document.as_ref().unwrap();

    assert!(document.view_bindings.iter().any(|binding| matches!(
        binding.target,
        boon_plan::DocumentBindingTarget::ScopedField { .. }
    )));

    assert!(document.expressions.iter().any(|expression| {
        let DocumentExprOp::Read {
            read: DocumentRead::Parameter { projection, .. },
        } = &expression.op
        else {
            return false;
        };
        projection
            .iter()
            .map(|name| document.names[name.0].as_str())
            .eq(["kind"])
    }));
    assert!(document.expressions.iter().any(|expression| {
        let DocumentExprOp::Select { arms, .. } = &expression.op else {
            return false;
        };
        arms.iter().any(|arm| {
            matches!(
                document.expressions[arm.output.0].op,
                DocumentExprOp::LocalBlock { .. }
            )
        })
    }));
    assert!(document.expressions.iter().any(|expression| {
        matches!(
            &expression.op,
            DocumentExprOp::Record { fields }
                if fields.len() == 2 && fields.iter().all(|field| field.spread)
        )
    }));
    for expression in &document.expressions {
        let DocumentExprOp::Select { input, .. } = expression.op else {
            continue;
        };
        if document.expressions[input.0].value_class != DocumentValueClass::Static {
            assert_ne!(expression.value_class, DocumentValueClass::Static);
        }
    }
}

#[test]
fn cells_rows_are_typed_visible_range_materializations() {
    let compiled = compile_source_path_to_machine_plan(
        &example_path("examples/cells.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled.plan.document.as_ref().unwrap();
    for state in &compiled.ir.state_cells {
        let executable_state_id = state
            .executable_state_id
            .unwrap_or_else(|| panic!("Cells state `{}` has no executable identity", state.path));
        let executable_state = compiled
            .ir
            .executable
            .states
            .get(executable_state_id.as_usize())
            .filter(|candidate| candidate.id == executable_state_id)
            .unwrap_or_else(|| {
                panic!(
                    "Cells state `{}` has a stale executable identity",
                    state.path
                )
            });
        assert!(
            compiled
                .ir
                .executable
                .expressions
                .get(executable_state.initial.as_usize())
                .is_some_and(|candidate| candidate.id == executable_state.initial),
            "Cells state `{}` has no exact executable initializer",
            state.path
        );
    }
    assert!(
        compiled
            .plan
            .storage_layout
            .scalar_slots
            .iter()
            .all(|slot| slot.value_type != boon_plan::PlanValueType::Unknown)
    );

    let chunk_ops = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::ListProjection {
                projection: PlanListProjection::Chunk { source_list, size },
            } => Some((*source_list, *size, op.output.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(chunk_ops.len(), 1, "chunk ops: {chunk_ops:#?}");
    assert_eq!(chunk_ops[0].1, 26);
    assert!(matches!(chunk_ops[0].2, Some(ValueRef::List(_))));

    assert!(!document.materializations.is_empty());
    assert!(document.expressions.len() < 2_600);
    assert!(document.templates.len() < 2_600);
    assert!(document.materializations.iter().any(|materialization| {
        matches!(
            materialization.source,
            DocumentMaterializationSource::List { .. }
        )
    }));
    assert!(
        document.materializations.iter().any(|materialization| {
            matches!(
                materialization.source,
                DocumentMaterializationSource::Field { .. }
                    | DocumentMaterializationSource::ScopedField { .. }
                    | DocumentMaterializationSource::ParameterField { .. }
                    | DocumentMaterializationSource::Parameter { .. }
            )
        }),
        "materialization sources: {:#?}",
        document
            .materializations
            .iter()
            .map(|materialization| &materialization.source)
            .collect::<Vec<_>>()
    );
    assert!(document.materializations.iter().all(|materialization| {
        match materialization.source {
            DocumentMaterializationSource::List { .. }
            | DocumentMaterializationSource::Field { .. }
            | DocumentMaterializationSource::ScopedField { .. }
            | DocumentMaterializationSource::ParameterField { .. }
            | DocumentMaterializationSource::Parameter { .. } => true,
            DocumentMaterializationSource::Expression { expression } => {
                expression_has_typed_list_source(document, expression)
            }
        }
    }));

    let address_field = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "cells.address")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(boon_plan::FieldId)
        .expect("cells.address field id");
    let cells_slot = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.contains_row_field(address_field))
        .expect("Cells list slot");
    assert_eq!(
        cells_slot.initializer_kind,
        boon_plan::ListInitializerKind::Range
    );
    assert_eq!(
        cells_slot.range,
        Some(boon_plan::PlanRangeInitializer { from: 0, to: 2599 })
    );
    assert_eq!(
        cells_slot
            .row_fields
            .iter()
            .filter(|field| field.role.is_authority())
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["index", "value"]
    );
    let demand_current_fields = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match (&op.kind, op.output.as_ref()) {
            (
                PlanOpKind::DerivedValue {
                    startup_recompute,
                    expression: Some(boon_plan::PlanDerivedExpression::MaterializedRowField { .. }),
                    ..
                },
                Some(ValueRef::Field(field)),
            ) if !startup_recompute
                && cells_slot.contains_row_field(*field)
                && cells_slot
                    .row_fields
                    .iter()
                    .any(|candidate| candidate.field_id == *field && candidate.role.is_value()) =>
            {
                cells_slot
                    .row_fields
                    .iter()
                    .find(|candidate| candidate.field_id == *field)
                    .map(|candidate| candidate.name.as_str())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let sources_field = compiled
        .ir
        .scope_index
        .fields
        .iter()
        .find(|field| {
            field.row.map(|row| boon_plan::ListId(row.list.as_usize())) == Some(cells_slot.list_id)
                && field.name == "sources"
                && field.role.is_value()
        })
        .expect("Cells source-only row field metadata");
    assert!(sources_field.resource_only);
    assert_eq!(
        demand_current_fields,
        BTreeSet::from([
            "address",
            "default_formula",
            "display_text",
            "error",
            "value",
        ])
    );
    assert!(
        !compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .any(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::DerivedValue {
                        expression: Some(boon_plan::PlanDerivedExpression::MaterializeList {
                            target_list,
                            ..
                        }),
                        ..
                    } if *target_list == cells_slot.list_id
                )
            })
    );
    let global_address_reads = document
        .expressions
        .iter()
        .enumerate()
        .filter(|(_, expression)| {
            matches!(
                expression.op,
                DocumentExprOp::Read {
                    read: DocumentRead::Field { field }
                } if field == address_field
            )
        })
        .collect::<Vec<_>>();
    assert!(
        global_address_reads.is_empty(),
        "row-owned address leaked into global reads: {:#?}",
        global_address_reads
            .iter()
            .map(|(index, expression)| (
                index,
                expression,
                compiled
                    .ir
                    .executable
                    .expressions
                    .get(expression.compiler_id)
            ))
            .collect::<Vec<_>>()
    );
    let editing_state = compiled
        .plan
        .debug_map
        .state_slots
        .iter()
        .find(|state| state.label == "cells.editing_text")
        .and_then(|state| state.id.strip_prefix("state:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(boon_plan::StateId)
        .unwrap_or_else(|| {
            panic!(
                "cells.editing_text state id; available states: {:?}",
                compiled
                    .plan
                    .debug_map
                    .state_slots
                    .iter()
                    .map(|state| state.label.as_str())
                    .collect::<Vec<_>>()
            )
        });
    let global_editing_reads = document
        .expressions
        .iter()
        .enumerate()
        .filter(|(_, expression)| {
            matches!(
                expression.op,
                DocumentExprOp::Read {
                    read: DocumentRead::State { state }
                } if state == editing_state
            )
        })
        .collect::<Vec<_>>();
    assert!(
        global_editing_reads.is_empty(),
        "indexed editing state leaked into global document reads: {:#?}",
        global_editing_reads
            .iter()
            .map(|(index, expression)| (
                index,
                expression,
                compiled
                    .ir
                    .executable
                    .expressions
                    .get(expression.compiler_id)
            ))
            .collect::<Vec<_>>()
    );

    let selected_input = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.selected_input")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(boon_plan::FieldId)
        .expect("store.selected_input field id");
    assert!(document.expressions.iter().any(|expression| {
        let DocumentExprOp::Project { field, .. } = &expression.op else {
            return false;
        };
        document.names.get(field.0).map(String::as_str) == Some("editing_text")
            && expression_reads_field(document, expression.id, selected_input)
    }));
    assert!(document.expressions.iter().any(|expression| {
        let DocumentExprOp::Select { input, arms } = &expression.op else {
            return false;
        };
        if !expression_reads_field(document, *input, selected_input) {
            return false;
        }
        arms.iter().any(|arm| {
            matches!(
                arm.pattern,
                boon_plan::DocumentPattern::Tag { tag }
                    if document.names.get(tag.0).map(String::as_str) == Some("Found")
            ) && arm.bindings.len() == 1
                && arm.bindings[0]
                    .projection
                    .iter()
                    .map(|name| document.names[name.0].as_str())
                    .eq(["value"])
        })
    }));
}

#[test]
fn document_backend_contains_no_fixture_branches() {
    let implementation = include_str!("document_plan_backend.rs");
    for fixture in [
        "counter.bn",
        "todomvc.bn",
        "todo_mvc_physical",
        "cells.bn",
        "novywave",
    ] {
        assert!(!implementation.contains(fixture), "found `{fixture}`");
    }
}

#[test]
fn unknown_document_constructor_fails_compilation() {
    let source = r#"
events: SOURCE
value: 0 |> HOLD value { events |> THEN { value } }
items: LIST {}
document: Document/new(root: Unknown/widget())
"#;
    let error = compile_fixture_source_text_to_machine_plan(
        "unknown-document-constructor.bn",
        source,
        TargetProfile::SoftwareDefault,
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("unknown") || message.contains("render") || message.contains("typecheck"),
        "{message}"
    );
}

#[test]
fn compiler_persists_root_latest_but_not_transient_or_derived_fields() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "root-latest-memory.bn",
        r#"
store: [
    pulse: SOURCE
    count:
        LATEST {
            0
            pulse |> THEN { count + 1 }
        }
    transient:
        pulse |> THEN { count + 10 }
    derived: count + 20
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(
        compiled
            .plan
            .persistence
            .memory
            .iter()
            .map(|memory| (memory.semantic_path.as_str(), memory.kind))
            .collect::<Vec<_>>(),
        [("store.count", MemoryKind::Scalar)]
    );
    assert_eq!(compiled.plan.storage_layout.scalar_slots.len(), 1);
    assert!(
        compiled
            .plan
            .debug_map
            .derived_values
            .iter()
            .any(|field| { field.label == "store.transient" })
    );
    assert!(
        compiled
            .plan
            .debug_map
            .derived_values
            .iter()
            .any(|field| { field.label == "store.derived" })
    );
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn compiler_resolves_append_record_fields_from_the_trigger_source_payload() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "append-source-payload-fields.bn",
        r#"
store: [
    completed: SOURCE
    append_token:
        completed |> THEN { completed.digest }
    revisions:
        LIST {}
        |> List/append(item: append_token |> THEN {
            [
                digest: append_token
                compiler: completed.compiler
                target: completed.target
            ]
        })
        |> List/map(item, new: revision_view(revision: item))
]

FUNCTION revision_view(revision) {
    [
        digest: revision.digest
        compiler: revision.compiler
        target: revision.target
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let append_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::ListMutation {
                    mutation: boon_plan::PlanListMutation::Append(_),
                }
            )
        })
        .expect("append op");
    let PlanOpKind::ListMutation {
        mutation: boon_plan::PlanListMutation::Append(append),
    } = &append_op.kind
    else {
        unreachable!();
    };
    assert_eq!(append_op.unresolved_executable_ref_count, 0);
    let mut item_refs = Vec::new();
    compiled
        .plan
        .row_expressions
        .visit_value_refs(append.item, &mut |value| item_refs.push(value.clone()))
        .unwrap();
    for name in ["compiler", "target"] {
        assert!(append.fields.iter().any(|field| field.name == name));
        assert!(item_refs.iter().any(|value| matches!(
            value,
            ValueRef::SourcePayload {
                field: boon_plan::SourcePayloadField::Named(payload_name),
                ..
            } if payload_name == name
        )));
    }
}

fn distributed_compiler_test_program(
    role: ProgramRole,
    source: &str,
) -> DistributedCompilerProgram {
    let role_name = role.as_str();
    DistributedCompilerProgram {
        revision: 1,
        role,
        source_label: format!("distributed-{role_name}-test"),
        units: vec![CompilerSourceUnit {
            path: format!("{role_name}/RUN.bn"),
            source: source.to_owned(),
        }],
        application: ApplicationIdentity::new(
            "dev.boon.distributed-compiler-tests",
            format!("distributed-{role_name}-state"),
            "test.local",
        ),
        schema_version: 1,
        migration_predecessors: Vec::new(),
    }
}

fn compile_distributed_compiler_test_bundle(
    client: &str,
    session: &str,
    server: &str,
) -> CompilerResult<CompiledDistributedMachinePlans> {
    compile_distributed_runtime_source_programs(
        &[
            distributed_compiler_test_program(ProgramRole::Client, client),
            distributed_compiler_test_program(ProgramRole::Session, session),
            distributed_compiler_test_program(ProgramRole::Server, server),
        ],
        TargetProfile::SoftwareDefault,
    )
}

fn assert_distributed_endpoints_are_independently_routable(
    compiled: &CompiledDistributedMachinePlans,
) {
    assert_eq!(
        compiled.graph.wire_schema_hash,
        distributed_graph_schema_hash(&compiled.graph).unwrap()
    );
    for role in [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ] {
        let endpoint = compiled
            .program(role)
            .unwrap()
            .plan
            .distributed_endpoint
            .as_ref()
            .unwrap();
        assert_eq!(endpoint.wire_schema, compiled.graph.wire_schema);
        assert_eq!(endpoint.wire_schema_hash, compiled.graph.wire_schema_hash);
    }
    for edge in &compiled.graph.wire_schema.value_edges {
        let producer = compiled
            .program(edge.producer_role)
            .unwrap()
            .plan
            .distributed_endpoint
            .as_ref()
            .unwrap();
        let consumer = compiled
            .program(edge.consumer_role)
            .unwrap()
            .plan
            .distributed_endpoint
            .as_ref()
            .unwrap();
        assert!(
            producer
                .endpoint
                .value_exports
                .iter()
                .any(|export| export.export_id == edge.export_id)
        );
        assert_eq!(consumer.value_import_route(edge.import_id), Some(edge));
    }
    for edge in &compiled.graph.wire_schema.event_edges {
        let producer = compiled
            .program(edge.producer_role)
            .unwrap()
            .plan
            .distributed_endpoint
            .as_ref()
            .unwrap();
        let consumer = compiled
            .program(edge.consumer_role)
            .unwrap()
            .plan
            .distributed_endpoint
            .as_ref()
            .unwrap();
        assert!(
            producer
                .endpoint
                .event_exports
                .iter()
                .any(|export| export.export_id == edge.export_id)
        );
        assert_eq!(consumer.event_import_route(edge.import_id), Some(edge));
    }
    for edge in &compiled.graph.wire_schema.call_edges {
        let caller = compiled
            .program(edge.caller_role)
            .unwrap()
            .plan
            .distributed_endpoint
            .as_ref()
            .unwrap();
        let callee = compiled
            .program(edge.callee_role)
            .unwrap()
            .plan
            .distributed_endpoint
            .as_ref()
            .unwrap();
        assert_eq!(caller.outbound_call_route(edge.call_site_id), Some(edge));
        assert_eq!(callee.inbound_call_route(edge.call_site_id), Some(edge));
        assert!(callee.endpoint.function_exports.iter().any(|function| {
            function.export_id == edge.function_export_id
                && function.parameters == edge.parameters
                && function.result_type == edge.result_type
        }));
    }
}

const DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT: &str = r#"
document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: TEXT { Distributed compiler test }
    )
)
"#;

#[test]
fn distributed_compiler_links_three_verified_role_plans_without_string_fallbacks() {
    let compiled = compile_distributed_compiler_test_bundle(
        r#"
store: [
    operand: 3
    session_count: Session/store.adjusted_count
    sum: Session/decorate(value: operand + session_count)
]

document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: TEXT { Distributed compiler test }
    )
)
"#,
        r#"
store: [
    server_count: Server/store.count
    adjusted_count: server_count + 1
    server_sum: Server/add(value: adjusted_count)
]

FUNCTION decorate(value) {
    value + 3
}
"#,
        r#"
store: [
    increment: SOURCE
    count:
        40 |> HOLD count {
            increment |> THEN { count + 1 }
        }
]

FUNCTION add(value) {
    value + 2
}
"#,
    )
    .unwrap();

    assert_distributed_endpoints_are_independently_routable(&compiled);

    let graph_id = compiled.graph.graph.graph_id;
    assert_eq!(compiled.graph.endpoints.len(), 3);
    for role in [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ] {
        let plan = &compiled.program(role).expect("compiled role plan").plan;
        let endpoint = plan
            .distributed_endpoint
            .as_ref()
            .expect("distributed endpoint plan");
        assert_eq!(plan.program_role, role);
        assert_eq!(endpoint.endpoint.role, role);
        assert_eq!(endpoint.graph.graph_id, graph_id);
        assert!(plan.debug_map.unresolved_executable_refs.is_empty());
        assert!(
            plan.regions
                .iter()
                .flat_map(|region| &region.ops)
                .all(|op| { op.unresolved_executable_ref_count == 0 })
        );
        let verification = verify_plan(plan).unwrap();
        assert_eq!(
            verification.status,
            "pass",
            "{role:?} verification failures: {:?}",
            verification
                .checks
                .iter()
                .filter(|check| !check.pass)
                .collect::<Vec<_>>()
        );
    }

    let server = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Server)
        .unwrap();
    assert_eq!(server.value_exports.len(), 1);
    assert_eq!(server.function_exports.len(), 1);
    assert_eq!(server.function_exports[0].parameters.len(), 1);

    let session = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    assert_eq!(session.value_imports.len(), 1);
    assert_eq!(session.value_exports.len(), 1);
    assert_eq!(session.function_exports.len(), 1);
    assert_eq!(session.remote_call_sites.len(), 1);
    let session_plan = &compiled.program(ProgramRole::Session).unwrap().plan;
    assert!(session_plan.regions.iter().flat_map(|region| &region.ops).any(|op| {
        op.inputs.iter().any(|input| {
            matches!(input, ValueRef::DistributedImport(id) if *id == session.value_imports[0].import_id)
        })
    }));

    let client = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .unwrap();
    assert_eq!(client.value_imports.len(), 1);
    let client_plan = &compiled.program(ProgramRole::Client).unwrap().plan;
    let client_execution_endpoint = &client_plan
        .distributed_endpoint
        .as_ref()
        .expect("client execution endpoint")
        .endpoint;
    let [call] = client_execution_endpoint.remote_call_sites.as_slice() else {
        panic!(
            "expected one remote call, got {:?}",
            client_execution_endpoint.remote_call_sites
        );
    };
    let [argument] = call.arguments.as_slice() else {
        panic!(
            "expected one remote call argument, got {:?}",
            call.arguments
        );
    };
    assert!(
        matches!(
            row_node(&client_plan.row_expressions, argument.value),
            PlanRowExpressionNode::NumberInfix { .. }
        ),
        "remote argument was not preserved as a compound pure expression: {:?}",
        argument.value
    );
    let client_operation_imports = client_plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .flat_map(|op| &op.inputs)
        .filter_map(|input| match input {
            ValueRef::DistributedImport(import_id) => Some(*import_id),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let expected_client_imports = client
        .value_imports
        .iter()
        .map(|import| import.import_id)
        .chain(call.result.current_import_id())
        .collect::<BTreeSet<_>>();
    assert!(
        expected_client_imports.is_subset(&client_operation_imports),
        "missing executable distributed imports: expected {expected_client_imports:?}, got {client_operation_imports:?}"
    );
}

#[test]
fn distributed_compiler_wire_hash_ignores_bodies_and_local_ids() {
    let baseline = compile_distributed_compiler_test_bundle(
        r#"
store: [
    submit: SOURCE
    current: Session/store.current
    doubled: Session/double(value: current)
]

document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: TEXT { Wire schema }
    )
)
"#,
        r#"
store: [
    submit: Client/store.submit
    count:
        0 |> HOLD count {
            submit |> THEN { count + 1 }
        }
    current: 7
]

FUNCTION double(value) {
    value + 1
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();
    let changed_locals = compile_distributed_compiler_test_bundle(
        r#"
store: [
    local_tick: SOURCE
    submit: SOURCE
    padding: 11
    current: Session/store.current
    doubled: Session/double(value: current)
]

document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: TEXT { Wire schema }
    )
)
"#,
        r#"
store: [
    local_tick: SOURCE
    submit: Client/store.submit
    count:
        0 |> HOLD count {
            submit |> THEN { count + 1 }
        }
    padding: 13
    current: 7
]

FUNCTION double(value) {
    value + 2
}
"#,
        "store: [\n    padding: 17\n    ready: True\n]\n",
    )
    .unwrap();

    assert_distributed_endpoints_are_independently_routable(&baseline);
    assert_distributed_endpoints_are_independently_routable(&changed_locals);
    assert_eq!(baseline.graph.wire_schema, changed_locals.graph.wire_schema);
    assert_eq!(
        distributed_graph_schema_hash(&baseline.graph).unwrap(),
        distributed_graph_schema_hash(&changed_locals.graph).unwrap()
    );

    let baseline_client = baseline
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .unwrap();
    let changed_client = changed_locals
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .unwrap();
    assert_ne!(
        baseline_client.event_exports[0].source_id,
        changed_client.event_exports[0].source_id
    );
    let baseline_session = baseline
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    let changed_session = changed_locals
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    assert_ne!(
        baseline_session.event_imports[0].local_source_id,
        changed_session.event_imports[0].local_source_id
    );
    assert_ne!(
        baseline_session.value_exports[0].value,
        changed_session.value_exports[0].value
    );
    assert_eq!(
        baseline_session.function_exports, changed_session.function_exports,
        "function wire exports must remain signature-only"
    );
    let producer_expression = |compiled: &CompiledDistributedMachinePlans| {
        let plan = &compiled.program(ProgramRole::Session).unwrap().plan;
        let [instance] = plan.producer_function_instances.as_slice() else {
            panic!("expected one producer function instance");
        };
        let ValueRef::Field(result) = instance.result else {
            panic!("producer result must be an ordinary derived field");
        };
        plan.regions
            .iter()
            .flat_map(|region| &region.ops)
            .find_map(|op| {
                if op.output != Some(ValueRef::Field(result)) {
                    return None;
                }
                match &op.kind {
                    PlanOpKind::DerivedValue { expression, .. } => expression.clone(),
                    _ => None,
                }
            })
            .expect("producer result computation")
    };
    assert_ne!(
        producer_expression(&baseline),
        producer_expression(&changed_locals),
        "function implementation must live in the producer machine graph"
    );
}

#[test]
fn distributed_compiler_wire_hash_changes_with_type_and_edge() {
    let client_for = |values: &str| {
        format!(
            "store: [\n{values}\n]\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        )
    };
    let baseline = compile_distributed_compiler_test_bundle(
        &client_for("    primary: Session/store.primary"),
        "store: [\n    primary: 7\n    secondary: 8\n]\n",
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();
    let changed_type = compile_distributed_compiler_test_bundle(
        &client_for("    primary: Session/store.primary"),
        "store: [\n    primary: TEXT { seven }\n    secondary: 8\n]\n",
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();
    let added_edge = compile_distributed_compiler_test_bundle(
        &client_for("    primary: Session/store.primary\n    secondary: Session/store.secondary"),
        "store: [\n    primary: 7\n    secondary: 8\n]\n",
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();

    assert_ne!(baseline.graph.wire_schema, changed_type.graph.wire_schema);
    assert_ne!(
        distributed_graph_schema_hash(&baseline.graph).unwrap(),
        distributed_graph_schema_hash(&changed_type.graph).unwrap()
    );
    assert_ne!(baseline.graph.wire_schema, added_edge.graph.wire_schema);
    assert_ne!(
        distributed_graph_schema_hash(&baseline.graph).unwrap(),
        distributed_graph_schema_hash(&added_edge.graph).unwrap()
    );
}

#[test]
fn distributed_compiler_accepts_every_adjacent_value_and_call_direction() {
    let compiled = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    local_value: 1
    session_value: Session/store.local_value
    session_call: Session/identity(value: local_value)
]

FUNCTION identity(value) {{
    value
}}

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    local_value: 2
    client_value: Client/store.local_value
    client_call: Client/identity(value: local_value)
    server_value: Server/store.local_value
    server_call: Server/identity(value: local_value)
]

FUNCTION identity(value) {
    value
}
"#,
        r#"
store: [
    local_value: 3
    session_value: Session/store.local_value
    session_call: Session/identity(value: local_value)
]

FUNCTION identity(value) {
    value
}
"#,
    )
    .unwrap();

    let expected_routes = BTreeSet::from([
        (ProgramRole::Client, ProgramRole::Session),
        (ProgramRole::Session, ProgramRole::Client),
        (ProgramRole::Session, ProgramRole::Server),
        (ProgramRole::Server, ProgramRole::Session),
    ]);
    let value_routes = compiled
        .graph
        .wire_schema
        .value_edges
        .iter()
        .map(|edge| (edge.consumer_role, edge.producer_role))
        .collect::<BTreeSet<_>>();
    let call_routes = compiled
        .graph
        .wire_schema
        .call_edges
        .iter()
        .map(|edge| (edge.caller_role, edge.callee_role))
        .collect::<BTreeSet<_>>();
    assert_eq!(value_routes, expected_routes);
    assert_eq!(call_routes, expected_routes);
}

#[test]
fn distributed_compiler_rejects_both_direct_role_directions_for_values_and_calls() {
    let client_document =
        |body: &str| format!("{body}\n{DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT}");
    let cases = [
        (
            "Client value read from Server",
            client_document("store: [\n    forbidden: Server/store.value\n]"),
            "store: [\n    ready: True\n]\n".to_owned(),
            "store: [\n    value: 1\n]\n".to_owned(),
            "Client cannot depend directly on Server",
        ),
        (
            "Server value read from Client",
            client_document("store: [\n    value: 1\n]"),
            "store: [\n    ready: True\n]\n".to_owned(),
            "store: [\n    forbidden: Client/store.value\n]\n".to_owned(),
            "Server cannot depend directly on Client",
        ),
        (
            "Client call into Server",
            client_document("store: [\n    forbidden: Server/identity(value: 1)\n]"),
            "store: [\n    ready: True\n]\n".to_owned(),
            "FUNCTION identity(value) { value }\n".to_owned(),
            "Client cannot depend directly on Server",
        ),
        (
            "Server call into Client",
            client_document("FUNCTION identity(value) { value }"),
            "store: [\n    ready: True\n]\n".to_owned(),
            "store: [\n    forbidden: Client/identity(value: 1)\n]\n".to_owned(),
            "Server cannot depend directly on Client",
        ),
    ];

    for (label, client, session, server, expected) in cases {
        let error =
            compile_distributed_compiler_test_bundle(&client, &session, &server).expect_err(label);
        let message = error.to_string();
        assert!(
            message.contains(expected) && message.contains("route the value through Session"),
            "unexpected {label} diagnostic: {message}"
        );
        assert!(
            !message.contains("/call:") && !message.contains("/function:"),
            "{label} diagnostic exposed digest identity: {message}"
        );
    }
}

#[test]
fn distributed_compiler_propagates_obsolete_dotted_role_syntax_diagnostic() {
    let error = compile_distributed_compiler_test_bundle(
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT,
        "store: [\n    forbidden: Client.foo\n]\n",
        "store: [\n    ready: True\n]\n",
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message
            .contains("qualified role values use `Client/value.field`, not `Client.value.field`"),
        "unexpected dotted-role diagnostic: {message}"
    );
}

#[test]
fn distributed_compiler_rejects_role_outputs_as_application_state() {
    let error = compile_distributed_compiler_test_bundle(
        &format!(
            "store: [\n    count: 1\n]\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        "store: [\n    count: Client/outputs.count\n]\n",
        "store: [\n    ready: True\n]\n",
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must use `Client/store.<value>`"),
        "unexpected error: {error}"
    );
}

#[test]
fn distributed_compiler_rejects_session_scoped_server_host_outputs() {
    let error = compile_distributed_compiler_test_bundle(
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT,
        "store: [\n    value: 42\n]\n",
        r#"
store: [
    session_value: Session/store.value
]

outputs: [
    leaked: store.session_value
]
"#,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("host output `leaked` depends on Session-scoped state"),
        "unexpected error: {error}"
    );
}

#[test]
fn distributed_compiler_lowers_reverse_adjacent_edges_without_role_ordering() {
    let compiled = compile_distributed_compiler_test_bundle(
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT,
        r#"
store: [
    seed: 7
]

FUNCTION double(value) {
    value + value
}
"#,
        r#"
store: [
    session_seed: Session/store.seed
    doubled: Session/double(value: session_seed)
]
"#,
    )
    .unwrap();
    let server = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Server)
        .unwrap();
    assert_eq!(server.value_imports.len(), 1);
    assert_eq!(server.remote_call_sites.len(), 1);
    let session = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    assert_eq!(session.value_exports.len(), 1);
    assert_eq!(session.function_exports.len(), 1);
}

#[test]
fn distributed_compiler_scopes_only_server_values_derived_from_session_inputs() {
    let compiled = compile_distributed_compiler_test_bundle(
        r#"
store: [
    increment: SOURCE
]

document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: TEXT { Origin scope }
    )
)
"#,
        r#"
store: [
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
    mirrored: Server/store.mirrored
    shared: Server/store.shared
]
"#,
        r#"
store: [
    session_count: Session/store.count
    mirrored: session_count + 1
    shared: 42
]
"#,
    )
    .unwrap();

    let server = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Server)
        .unwrap();
    let session = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    let mirrored = server
        .value_exports
        .iter()
        .find(|export| export.origin_scoped)
        .expect("Session-derived Server export");
    let shared = server
        .value_exports
        .iter()
        .find(|export| !export.origin_scoped)
        .expect("independent shared Server export");
    assert!(session.value_imports.iter().any(|import| {
        import.source_export_id == mirrored.export_id
            && import.scope == DistributedRouteScopePlan::OriginScoped
            && import.source_origin_scoped
    }));
    assert!(session.value_imports.iter().any(|import| {
        import.source_export_id == shared.export_id
            && import.scope == DistributedRouteScopePlan::SharedSubscription
            && !import.source_origin_scoped
    }));
}

#[test]
fn distributed_compiler_rejects_session_info_captured_by_global_server_state() {
    for intrinsic in ["status", "principal"] {
        let server = format!(
            r#"
store: [
    seed: 1
    saved: Server/store.saved
]
"#,
        );
        let global_state = format!(
            r#"
store: [
    session_seed: Session/store.seed
    saved:
        SessionInfo/{intrinsic}() |> HOLD saved {{
            LATEST {{}}
        }}
]
"#,
        );
        let error = compile_distributed_compiler_test_bundle(
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT,
            &server,
            &global_state,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("outside an active Session scope"),
            "unexpected {intrinsic} error: {error}"
        );
    }
}

#[test]
fn distributed_compiler_accepts_server_session_info_in_origin_scoped_call_branch() {
    for intrinsic in ["status", "principal"] {
        let session = format!(
            r#"
store: [
    info: Server/session_info(seed: 1)
]
"#,
        );
        let server = format!(
            r#"
store: [
    ready: True
]

FUNCTION session_info(seed) {{
    seed |> WHILE {{
        1 => SessionInfo/{intrinsic}()
        __ => SessionInfo/{intrinsic}()
    }}
}}
"#,
        );
        let compiled = compile_distributed_compiler_test_bundle(
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT,
            &session,
            &server,
        )
        .unwrap();

        let session_endpoint = compiled
            .graph
            .endpoints
            .iter()
            .find(|endpoint| endpoint.role == ProgramRole::Session)
            .unwrap();
        let [call] = session_endpoint.remote_call_sites.as_slice() else {
            panic!(
                "expected one Session-origin Server call, got {:#?}",
                session_endpoint.remote_call_sites
            );
        };
        assert_eq!(call.scope, DistributedRouteScopePlan::OriginScoped);
        assert!(compiled.graph.wire_schema.call_edges.iter().any(|edge| {
            edge.call_site_id == call.call_site_id
                && edge.scope == DistributedRouteScopePlan::OriginScoped
                && edge.caller_role == ProgramRole::Session
                && edge.callee_role == ProgramRole::Server
        }));

        let server_plan = &compiled.program(ProgramRole::Server).unwrap().plan;
        assert!(
            server_plan
                .regions
                .iter()
                .flat_map(|region| &region.ops)
                .any(|op| {
                    let PlanOpKind::DerivedValue {
                        expression: Some(expression),
                        ..
                    } = &op.kind
                    else {
                        return false;
                    };
                    let mut found = false;
                    expression
                        .visit_intrinsics(&server_plan.row_expressions, &mut |_| found = true)
                        .unwrap();
                    found
                }),
            "Server plan lost SessionInfo/{intrinsic}()"
        );
    }
}

#[test]
fn distributed_compiler_solves_simultaneous_adjacent_role_interfaces() {
    let compiled = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    client_seed: 3
    session_seed: Session/store.session_seed
]

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    session_seed: 7
    client_seed: Client/store.client_seed
    server_seed: Server/store.server_seed
]
"#,
        r#"
store: [
    server_seed: 11
    session_seed: Session/store.session_seed
]
"#,
    )
    .unwrap();

    let client = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .unwrap();
    let session = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    let server = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Server)
        .unwrap();
    assert_eq!(client.value_imports.len(), 1);
    assert_eq!(client.value_exports.len(), 1);
    assert_eq!(session.value_imports.len(), 2);
    assert_eq!(session.value_exports.len(), 1);
    assert_eq!(server.value_imports.len(), 1);
    assert_eq!(server.value_exports.len(), 1);
}

#[test]
fn distributed_compiler_rejects_unresolved_interface_cycles() {
    let error = compile_distributed_compiler_test_bundle(
        &format!(
            "store: [\n    left: Session/store.right\n]\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        "store: [\n    right: Client/store.left\n]\n",
        "store: [\n    ready: True\n]\n",
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("distributed interface types did not resolve")
            && message.contains("Client/store.left")
            && message.contains("Session/store.right"),
        "unexpected error: {message}"
    );
}

#[test]
fn distributed_compiler_rejects_grounded_combinational_cycles() {
    let error = compile_distributed_compiler_test_bundle(
        &format!(
            "store: [\n    left: Session/store.right + 1\n]\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        "store: [\n    right: Client/store.left + 1\n]\n",
        "store: [\n    ready: True\n]\n",
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("distributed combinational cycle")
            && message.contains("Client/store.left")
            && message.contains("Session/store.right")
            && message.contains("SOURCE, HOLD, or asynchronous effect"),
        "unexpected cycle diagnostic: {message}"
    );
}

#[test]
fn distributed_compiler_rejects_immediate_non_continuous_cycles() {
    let error = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    left:
        Session/store.right |> THEN {{ 1 }}
]

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    right:
        Client/store.left |> THEN { 1 }
]
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("distributed combinational cycle")
            && message.contains("Client/store.left")
            && message.contains("Session/store.right"),
        "unexpected immediate event-flow cycle diagnostic: {message}"
    );
}

#[test]
fn distributed_compiler_rejects_current_call_only_cycles() {
    let error = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    left: Session/from_client(value: 1)
]

FUNCTION from_session(value) {{
    store.left + value
}}

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    right: Client/from_session(value: 1)
]

FUNCTION from_client(value) {
    store.right + value
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("distributed combinational cycle")
            && message.contains("SOURCE, HOLD, or asynchronous effect")
            && message.contains("Client/store.left")
            && message.contains("Session/store.right")
            && message.contains("Client/from_session")
            && message.contains("Session/from_client")
            && !message.contains("/call:")
            && !message.contains("/function:"),
        "unexpected current-call cycle diagnostic: {message}"
    );
}

#[test]
fn distributed_compiler_rejects_mixed_value_and_current_call_cycles() {
    let error = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    left: Session/identity(value: Session/store.right + 1)
]

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    right: Client/store.left + 1
]

FUNCTION identity(value) {
    value
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("distributed combinational cycle")
            && message.contains("Client/store.left")
            && message.contains("Session/store.right"),
        "unexpected mixed distributed cycle diagnostic: {message}"
    );
}

#[test]
fn distributed_compiler_treats_invocation_calls_as_temporal_cycle_boundaries() {
    compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    invoke: SOURCE
    left:
        0 |> HOLD left {{
            invoke |> THEN {{
                Session/identity(value: Session/store.right + 1)
            }}
        }}
]

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    right: Client/store.left + 1
]

FUNCTION identity(value) {
    value
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();
}

#[test]
fn distributed_compiler_accepts_a_cycle_broken_by_source_and_hold() {
    compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    tick: SOURCE
    left:
        0 |> HOLD left {{
            tick |> THEN {{ Session/store.right + 1 }}
        }}
]

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        "store: [\n    right: Client/store.left + 1\n]\n",
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();
}

#[test]
fn distributed_compiler_infers_identity_function_boundary_from_its_call_site() {
    let compiled = compile_distributed_compiler_test_bundle(
        &format!(
            "store: [\n    result: Session/identity(value: 5)\n]\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    ready: True
]

FUNCTION identity(value) {
    value
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();
    let session = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    let [identity] = session.function_exports.as_slice() else {
        panic!("expected one identity export");
    };
    assert_eq!(identity.parameters[0].data_type, DataTypePlan::Number);
    assert_eq!(identity.result_type, DataTypePlan::Number);
}

#[test]
fn distributed_compiler_assigns_current_and_invocation_modes_per_call_site() {
    let compiled = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    invoke: SOURCE
    current: Session/identity(value: 5)
    invoked:
        invoke |> THEN {{ Session/identity(value: 5) }}
]

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    ready: True
]

FUNCTION identity(value) {
    value
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();

    let client = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .unwrap();
    assert_eq!(client.remote_call_sites.len(), 2);
    let current = client
        .remote_call_sites
        .iter()
        .find(|call| call.mode == boon_plan::DistributedCallMode::Current)
        .expect("one current call site");
    assert!(current.invocation_arms.is_empty());
    let invocation = client
        .remote_call_sites
        .iter()
        .find(|call| call.mode == boon_plan::DistributedCallMode::Invocation)
        .expect("one invocation call site");
    assert_eq!(invocation.invocation_arms.len(), 1);
    assert!(matches!(
        invocation.invocation_arms[0].trigger,
        ValueRef::Source(_)
    ));

    let session_plan = &compiled.program(ProgramRole::Session).unwrap().plan;
    assert_eq!(session_plan.producer_function_instances.len(), 2);
    let current_instance = session_plan
        .producer_function_instances
        .iter()
        .find(|instance| instance.mode == boon_plan::DistributedCallMode::Current)
        .expect("one current producer instance");
    assert_eq!(current_instance.call_site_id, current.call_site_id);
    assert!(current_instance.invocation_source.is_none());
    let invocation_instance = session_plan
        .producer_function_instances
        .iter()
        .find(|instance| instance.mode == boon_plan::DistributedCallMode::Invocation)
        .expect("one invocation producer instance");
    assert_eq!(invocation_instance.call_site_id, invocation.call_site_id);
    assert!(matches!(
        invocation_instance.invocation_source,
        Some(source) if invocation_instance.ownership.sources.contains(&source)
    ));
}

#[test]
fn distributed_compiler_keeps_hold_backed_remote_call_current() {
    let compiled = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    increment: SOURCE
]

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
    doubled: Server/double(value: count)
]
"#,
        r#"
store: [ready: True]

FUNCTION double(value) {
    value * 2
}
"#,
    )
    .unwrap();

    let session = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    let [call] = session.remote_call_sites.as_slice() else {
        panic!("expected one Session-to-Server call")
    };
    assert_eq!(call.mode, boon_plan::DistributedCallMode::Current);
    assert!(call.invocation_arms.is_empty());
}

#[test]
fn distributed_compiler_expands_qualified_calls_through_reusable_functions() {
    let compiled = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    first: remote_add(value: 5)
    second: outer(value: 8)
]

FUNCTION remote_add(value) {{
    Session/add(value: value)
}}

FUNCTION outer(value) {{
    remote_add(value: value)
}}

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    ready: True
]

FUNCTION add(value) {
    value + 1
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();

    let client = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .unwrap();
    assert_eq!(client.remote_call_sites.len(), 2);
    assert_ne!(
        client.remote_call_sites[0].call_site_id,
        client.remote_call_sites[1].call_site_id
    );
    assert_ne!(
        client.remote_call_sites[0].result.current_import_id(),
        client.remote_call_sites[1].result.current_import_id()
    );
    let session_plan = &compiled.program(ProgramRole::Session).unwrap().plan;
    let [first, second] = session_plan.producer_function_instances.as_slice() else {
        panic!(
            "expected two producer instances, got {:?}",
            session_plan.producer_function_instances
        );
    };
    assert_eq!(first.function_export_id, second.function_export_id);
    assert_ne!(first.call_site_id, second.call_site_id);
    assert_ne!(first.owner, second.owner);
    assert_ne!(first.result, second.result);
    assert_ne!(first.arguments[0].import_id, second.arguments[0].import_id);
    macro_rules! assert_disjoint_ownership {
        ($field:ident) => {
            assert!(
                first
                    .ownership
                    .$field
                    .iter()
                    .all(|id| !second.ownership.$field.contains(id))
            );
        };
    }
    assert_disjoint_ownership!(static_owners);
    assert_disjoint_ownership!(sources);
    assert_disjoint_ownership!(states);
    assert_disjoint_ownership!(fields);
    assert_disjoint_ownership!(lists);
    assert_disjoint_ownership!(indexes);
    assert_disjoint_ownership!(effects);
    assert_eq!(
        first.ownership.static_owners.first(),
        Some(&first.owner.static_owner)
    );
    assert_eq!(
        second.ownership.static_owners.first(),
        Some(&second.owner.static_owner)
    );
    assert!(
        matches!(first.result, ValueRef::Field(field) if first.ownership.fields.contains(&field))
    );
    assert!(
        matches!(second.result, ValueRef::Field(field) if second.ownership.fields.contains(&field))
    );
}

#[test]
fn distributed_compiler_binds_hold_backed_producer_resources_before_plan_verification() {
    let compiled = compile_distributed_compiler_test_bundle(
        &format!(
            "store: [\n    invoke: SOURCE\n    remembered:\n        invoke |> THEN {{ Session/remember(value: 5) }}\n]\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    ready: True
]

FUNCTION remember(value) {
    value |> HOLD current { LATEST {} }
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();

    let session_plan = &compiled.program(ProgramRole::Session).unwrap().plan;
    let [instance] = session_plan.producer_function_instances.as_slice() else {
        panic!(
            "expected one producer instance, got {:?}",
            session_plan.producer_function_instances
        );
    };
    assert_eq!(instance.mode, boon_plan::DistributedCallMode::Invocation);
    assert!(matches!(
        instance.invocation_source,
        Some(source) if instance.ownership.sources.contains(&source)
    ));
    assert!(!instance.ownership.states.is_empty());
    assert!(
        matches!(instance.result, ValueRef::Field(field) if instance.ownership.fields.contains(&field))
    );
    let verification = verify_plan(session_plan).unwrap();
    assert_eq!(verification.error_count, 0, "{verification:#?}");
    assert!(verification.checks.iter().all(|check| check.pass));
}

#[test]
fn distributed_compiler_rejects_durable_effect_owned_by_process_local_producer() {
    let error = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    register: SOURCE
    registration:
        register |> THEN {{ Session/register() }}
]

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    ready: True
]

FUNCTION register() {
    trigger: SOURCE
    RegistrationNotRequested |> HOLD registration {
        trigger |> THEN {
            DevelopmentPasskey/register(
                workspace_id: TEXT { workspace-1 }
                workspace_grant_id: TEXT { grant-1 }
                account_id: TEXT { account-1 }
                credential_count: 1
                simulation: Success
            )
        }
    }
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(
        message.contains("distributed producer function")
            && message.contains("durable idempotent outbox effect")
            && message.contains("DevelopmentPasskey/register")
            && message.contains("process-local"),
        "unexpected error: {message}"
    );
}

#[test]
fn distributed_compiler_allows_read_only_effect_owned_by_process_local_producer() {
    let compiled = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    refresh: SOURCE
    reading:
        refresh |> THEN {{ Session/read_clock() }}
]

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    ready: True
]

FUNCTION read_clock() {
    trigger: SOURCE
    ClockNotRead |> HOLD reading {
        trigger |> THEN { Clock/wall() }
    }
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();

    let plan = &compiled.program(ProgramRole::Session).unwrap().plan;
    let [instance] = plan.producer_function_instances.as_slice() else {
        panic!("expected one producer instance");
    };
    let [invocation_id] = instance.ownership.effects.as_slice() else {
        panic!("expected one producer-owned effect");
    };
    let invocation = plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if effect.invocation_id == *invocation_id => Some(effect),
            _ => None,
        })
        .expect("owned effect invocation");
    let contract = plan
        .effects
        .iter()
        .find(|contract| contract.effect_id == invocation.effect_id)
        .expect("owned effect contract");
    assert_eq!(contract.host_operation, "Clock/wall");
    assert_eq!(contract.replay, EffectReplay::ReadOnly);
    assert!(plan.persistence.effect_outbox.is_empty());
}

#[test]
fn producer_effect_policy_allows_process_scoped_contract_for_process_local_owner() {
    let write = compile_fixture_source_text_to_machine_plan(
        "process-scoped-producer-policy.bn",
        include_str!("../../../examples/bytes_file_write_effect.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let invocation_id = write
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } => Some(effect.invocation_id),
            _ => None,
        })
        .expect("process-scoped effect invocation");
    let contract = write
        .plan
        .effects
        .iter()
        .find(|contract| contract.host_operation == "File/write_bytes")
        .expect("process-scoped effect contract");
    assert_eq!(contract.replay, EffectReplay::ProcessScoped);
    assert!(write.plan.persistence.effect_outbox.is_empty());

    let distributed = compile_distributed_compiler_test_bundle(
        &format!(
            r#"
store: [
    invoke: SOURCE
    result:
        invoke |> THEN {{ Session/identity(value: 7) }}
]

{}
"#,
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    ready: True
]

FUNCTION identity(value) {
    value
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();
    let plan = &distributed.program(ProgramRole::Session).unwrap().plan;
    let [instance] = plan.producer_function_instances.as_slice() else {
        panic!("expected one producer instance");
    };
    let mut instance = instance.clone();
    instance.ownership.effects = vec![invocation_id];
    super::machine_plan_backend::validate_producer_function_effect_ownership(
        &[instance],
        &write.plan.regions,
        &write.plan.effects,
    )
    .unwrap();
}

#[test]
fn distributed_compiler_lowers_remote_source_as_an_event_lane() {
    let compiled = compile_distributed_compiler_test_bundle(
        &format!(
            "store: [\n    submit: SOURCE\n]\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [
    submit: Client/store.submit
    count:
        0 |> HOLD count {
            submit |> THEN { count + 1 }
        }
]
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap();

    let client = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .unwrap();
    let session = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    assert_eq!(client.event_exports.len(), 1);
    assert!(client.value_exports.is_empty());
    assert_eq!(session.event_imports.len(), 1);
    assert!(session.value_imports.is_empty());
    assert_eq!(
        client.event_exports[0].export_id,
        session.event_imports[0].source_export_id
    );
    assert!(
        compiled
            .program(ProgramRole::Session)
            .unwrap()
            .plan
            .source_routes
            .iter()
            .any(|route| route.source_id == session.event_imports[0].local_source_id)
    );
}

#[test]
fn distributed_compiler_rejects_an_effectful_call_without_an_exact_trigger() {
    let error = compile_distributed_compiler_test_bundle(
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT,
        "result: Server/logged(value: TEXT { one })\noutputs: [\n    ready: True\n]\n",
        r#"
outputs: [
    ready: True
]

FUNCTION logged(value) {
    value |> Log/info()
}
"#,
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("distributed call `Server/logged`")
            && message.contains("no exact SOURCE or state trigger"),
        "unexpected error: {message}"
    );
}

#[test]
fn distributed_compiler_preserves_row_owned_remote_call_inputs() {
    let client = format!(
        r#"
store: [
    items: LIST {{ [value: 1], [value: 2] }}
    rows:
        items
        |> List/map(item, new: decorate(item: item))
]

FUNCTION decorate(item) {{
    [value: Session/add(value: item.value)]
}}

{}
"#,
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
    );
    let compiled = compile_distributed_compiler_test_bundle(
        &client,
        r#"
outputs: [
    ready: True
]

FUNCTION add(value) {
    value + 1
}
"#,
        "outputs: [\n    ready: True\n]\n",
    )
    .unwrap();
    let client_plan = &compiled.program(ProgramRole::Client).unwrap().plan;
    let client = &client_plan
        .distributed_endpoint
        .as_ref()
        .expect("client execution endpoint")
        .endpoint;
    let [call] = client.remote_call_sites.as_slice() else {
        panic!(
            "expected one row-owned call site, got {:#?}",
            client.remote_call_sites
        );
    };
    assert_eq!(call.mode, boon_plan::DistributedCallMode::Current);
    assert_eq!(call.scope, DistributedRouteScopePlan::SessionLocal);
    let [argument] = call.arguments.as_slice() else {
        panic!(
            "expected one row-owned call argument, got {:#?}",
            call.arguments
        );
    };
    let mut row_fields = Vec::new();
    client_plan
        .row_expressions
        .visit_list_fields(argument.value, &mut |list, field| {
            row_fields.push((list, field))
        })
        .unwrap();
    assert_eq!(
        row_fields.len(),
        1,
        "row identity was erased from the remote argument: {:?}",
        argument.value
    );
    let mut inputs = Vec::new();
    client_plan
        .row_expressions
        .visit_inputs(argument.value, &mut |input| inputs.push(input))
        .unwrap();
    assert!(
        inputs
            .iter()
            .any(|input| matches!(input, ValueRef::List(_))),
        "row-owned call argument did not retain its source-list dependency: {:?}",
        argument.value
    );
}
