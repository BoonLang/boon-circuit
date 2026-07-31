use super::*;
use boon_contract::{CanonicalSourceBundleV1, SourceBundleUnit};
use boon_plan::{
    DistributedRouteScopePlan, HostPortPlan, ListInitializerKind, PlanInfixOp,
    PlanListAccessSelection, PlanListMutation, PlanListRowFieldRole, PlanRowBuiltin,
    SourcePayloadField, distributed_graph_schema_hash,
};
use std::collections::BTreeSet;

#[derive(serde::Deserialize)]
struct SourceBundleGoldenFixture {
    schema: String,
    entrypoint: String,
    units: Vec<SourceBundleGoldenUnit>,
    canonical_entrypoint: String,
    canonical_paths: Vec<String>,
    digest: String,
}

#[derive(serde::Deserialize)]
struct SourceBundleGoldenUnit {
    path: String,
    source: String,
}

#[test]
fn source_compilation_is_bound_to_semantic_and_verification_manifests() {
    let first = compile_fixture_source_text_to_machine_plan(
        "verified-spine.bn",
        "value: 1\n",
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let second = compile_fixture_source_text_to_machine_plan(
        "verified-spine.bn",
        "value: 1\n",
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let semantic = first.ir.semantic_program_digest();
    let verification = first.ir.verification_manifest_digest();
    let source_bundle = CanonicalSourceBundleV1::new(
        "verified-spine.bn",
        [SourceBundleUnit::new("verified-spine.bn", "value: 1\n")],
    )
    .unwrap()
    .digest();
    assert_eq!(first.ir.source_bundle_digest_v1(), source_bundle);
    assert_eq!(second.ir.source_bundle_digest_v1(), source_bundle);
    assert!(semantic.as_bytes().iter().any(|byte| *byte != 0));
    assert!(verification.as_bytes().iter().any(|byte| *byte != 0));
    assert_eq!(second.ir.semantic_program_digest(), semantic);
    assert_eq!(second.ir.verification_manifest_digest(), verification);
}

#[test]
fn dependency_cycle_recovery_lowers_to_an_explicit_private_fault_boundary() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "dependency-cycle-boundary.bn",
        r#"
value:
    Dependency/catch_cycle(
        value: Ready
        on_cycle: CycleError
    )
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let boundary = compiled
        .plan
        .row_expressions
        .iter()
        .find_map(|(_, node)| match node {
            boon_plan::PlanRowExpressionNode::CatchCycle { input, on_cycle } => {
                Some((*input, *on_cycle))
            }
            _ => None,
        })
        .expect("Dependency/catch_cycle must survive as an executor-owned boundary");
    assert_ne!(
        boundary.0, boundary.1,
        "normal and application fallback values must remain distinct"
    );
}

#[test]
fn nested_row_fields_with_repeated_leaf_names_keep_distinct_authorities() {
    let parsed = parse_source(
        "nested-row-authority-fields.bn",
        r#"
store: [
    rows:
        LIST {
            [
                left: [value: 1]
                right: [value: TEXT { distinct }]
            ]
        }
]

document: Document/new(
    root: Element/text(element: [], text: TEXT { ready })
)
"#,
    )
    .unwrap();
    let checked = boon_typecheck::check_program(&parsed);
    assert!(!checked.report.has_errors(), "{:#?}", checked.report);
    let ir = verify_and_lower_checked(checked.program.unwrap(), &[]).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault)
        .expect("nested structural fields are keyed by authority path, not leaf spelling");
    let list = plan
        .storage_layout
        .list_slots
        .first()
        .expect("nested row list slot");
    assert!(
        list.row_fields.iter().any(|field| field.name == "left"),
        "{:#?}",
        list.row_fields
    );
    assert!(
        list.row_fields.iter().any(|field| field.name == "right"),
        "{:#?}",
        list.row_fields
    );
}

#[test]
fn appended_refinement_and_indexed_state_keep_distinct_persistence_leaves() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "indexed-append-authority.bn",
        r#"
store: [
    append: SOURCE
    rows:
        LIST {
            [title: TEXT { first }, completed: False]
            [title: TEXT { second }, completed: True]
        }
        |> List/append(item:
            append |> THEN {
                [title: TEXT { third }, completed: False]
            }
        )
        |> List/map(item, new: stateful_row(row: item))
]

FUNCTION stateful_row(row) {
    [
        sources: [toggle: SOURCE]
        title: row.title
        completed:
            row.completed |> HOLD completed {
                sources.toggle |> THEN { completed |> Bool/not() }
            }
    ]
}

document: Document/new(
    root: Element/text(element: [], text: TEXT { ready })
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("indexed state and its list-input authority have distinct stable leaves");
    let verification = boon_plan::verify_plan(&compiled.plan).unwrap();
    assert_eq!(verification.status, "pass", "{:#?}", verification.checks);
    let persistent = compiled
        .plan
        .persistence
        .lists
        .iter()
        .find(|list| list.semantic_path == "store.rows")
        .expect("persistent rows authority");
    let paths = persistent
        .row_fields
        .iter()
        .map(|field| field.semantic_path.as_str())
        .collect::<BTreeSet<_>>();
    assert!(paths.contains("store.rows.completed"), "{paths:#?}");
    assert!(
        paths.contains("store.rows.@authority:completed"),
        "{paths:#?}"
    );
    let stable_fields = persistent
        .row_fields
        .iter()
        .filter_map(|field| field.runtime_field_id)
        .collect::<BTreeSet<_>>();
    let append = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::ListMutation {
                mutation: PlanListMutation::Append(append),
            } => Some(append),
            _ => None,
        })
        .expect("append operation");
    assert_eq!(
        append
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["completed", "title"])
    );
    assert!(
        append
            .fields
            .iter()
            .all(|field| stable_fields.contains(&field.field_id))
    );
}

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
                    PlanRowSelectPattern::Tag { name },
                    PlanRowExpressionNode::Select { input, arms },
                ) if name == "Finished" => Some((*input, arms)),
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
                    PlanRowSelectPattern::Tag { name },
                    PlanRowExpressionNode::Constant { constant_id },
                ) if name == "Retained" => Some(*constant_id),
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
        Some(PlanConstantValue::Tag { name }) if name == "True"
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
    let store_last_status = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.last_status")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|field| field.parse::<usize>().ok())
        .map(FieldId)
        .expect("store.last_status field");
    let output_last_status = match &compiled.plan.output_root("last_status").unwrap().value {
        OutputValueRef::RuntimeValue {
            value: ValueRef::Field(field),
            ..
        } => *field,
        value => panic!("unexpected last_status output value: {value:#?}"),
    };
    if output_last_status != store_last_status {
        let expression = compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .find_map(|op| {
                (op.output == Some(ValueRef::Field(output_last_status))).then_some(&op.kind)
            })
            .and_then(|kind| match kind {
                PlanOpKind::DerivedValue {
                    expression: Some(PlanDerivedExpression::RowExpression { expression }),
                    ..
                } => Some(*expression),
                _ => None,
            })
            .expect("last_status output alias expression");
        assert!(
            matches!(
                row_node(&compiled.plan.row_expressions, expression),
                PlanRowExpressionNode::Field {
                    input: ValueRef::Field(field)
                } if *field == store_last_status
            ),
            "output alias bypassed store.last_status: {:#?}",
            row_node(&compiled.plan.row_expressions, expression)
        );
    }
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
fn compiler_lowers_direct_host_results_to_verified_nonpersistent_lanes() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "direct-host-result-plan.bn",
        r#"
store: [
    start: SOURCE
    result:
        start |> THEN { Clock/wall() }
    observed:
        0 |> HOLD observed {
            result |> WHEN {
                WallClockRead => 1
                __ => SKIP
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let result = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.result")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|field| field.parse::<usize>().ok())
        .map(FieldId)
        .expect("store.result field");
    let effect = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::EffectUpdate { effect, .. }
                if op.output == Some(ValueRef::Field(result)) =>
            {
                Some(effect)
            }
            _ => None,
        })
        .expect("direct effect update");
    let lane = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::RowExpression { expression }),
                materialization: None,
                ..
            } if op.output == Some(ValueRef::Field(result)) => Some(*expression),
            _ => None,
        })
        .expect("direct transient result lane");
    assert!(matches!(
        row_node(&compiled.plan.row_expressions, lane),
        PlanRowExpressionNode::TransientEffectResult { invocation_id }
            if *invocation_id == effect.invocation_id
    ));
    assert!(
        compiled
            .plan
            .persistence
            .memory
            .iter()
            .all(|memory| memory.semantic_path != "store.result")
    );
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
        .map(|op| (op.id, op.kind.clone()))
        .collect::<Vec<_>>();
    assert!(
        compiled.plan.capability_summary.cpu_plan_executor_complete,
        "direct host-result fixture has unsupported ops: {unsupported:#?}"
    );
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
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
        Parsed[value: 0] |> HOLD value {
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
    let PlanRowExpressionNode::TextToNumber { input, radix } =
        row_node(&compiled.plan.row_expressions, *value)
    else {
        unreachable!();
    };
    assert!(radix.is_none());
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
        Some(&boon_plan::PlanValueType::Tag)
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
                expression: Some(expression),
                materialization: Some(boon_plan::PlanListMaterialization { target_list, .. }),
                ..
            } if *target_list == boon_plan::ListId(tasks.id.as_usize()) => Some(expression),
            _ => None,
        })
        .expect("tasks materialization expression");
    let PlanDerivedExpression::RowExpression { expression } = expression else {
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
    DataTypePlan, DocumentExprOp, DocumentRead, DocumentTextSegment, EffectBarrier, EffectReplay,
    EffectResultPolicy, EffectResultRoute, FieldId, ListId, MemoryId, MemoryKind,
    MigrationExpressionPlan, MigrationPredecessorBinding, MigrationTransferKindPlan,
    MigrationTransformPlan, OutputContractKind, OutputDemandPolicy, OutputValueRef,
    PLAN_MAJOR_VERSION, PlanConstantValue, PlanContextualOperationKind, PlanDerivedExpression,
    PlanListMaterialization, PlanLocalId, PlanOpKind, PlanRowExpressionArena, PlanRowExpressionId,
    PlanRowExpressionNode, PlanRowSelectPattern, PlanStaticOwnerId, RootOutputDemand, ValueRef,
    plan_binary, plan_sha256, verify_plan,
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
    let compiled = compile_fixture_source_text_to_machine_plan(
        "source-event-skip-type.bn",
        r#"
store: [
    input: SOURCE
    result:
        input.key |> WHEN {
            TEXT { Enter } => TEXT { saved }
            __ => SKIP
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("SKIP is absence and must not make a Text event transform an Enum");
    assert!(
        compiled
            .plan
            .row_expressions
            .iter()
            .any(|(_, expression)| matches!(expression, PlanRowExpressionNode::Absent))
    );
    assert!(compiled.plan.constants.iter().all(|constant| {
        !matches!(
            &constant.value,
            PlanConstantValue::Tag { name } if name == "SKIP"
        )
    }));
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
                expression: Some(PlanDerivedExpression::RowExpression { expression }),
                materialization: Some(_),
                ..
            } => match row_node(&compiled.plan.row_expressions, *expression) {
                PlanRowExpressionNode::ListAccess { access } => Some(access.as_ref()),
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
fn compiler_preserves_transformed_bits_widths_through_ir_plan_and_json() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "bits-operations-output.bn",
        r#"
store: [
    left: BITS[8] { 16ua3 }
    right: BITS[8] { 16u05 }
    slice: left |> Bits/slice(from: 2, count: 3)
    concatenated: left |> Bits/concat(with: right)
    extended: left |> Bits/sign_extend(width: 12)
    encoded:
        concatenated
        |> Bits/to_bytes(byte_order: BigEndian)
]

outputs: [
    slice: store.slice
    concatenated: store.concatenated
    extended: store.extended
    encoded: store.encoded
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let call_type = |name: &str| {
        compiled
            .ir
            .executable
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                boon_ir::ExecutableExpressionKind::Call {
                    name: call_name, ..
                } if call_name == name => Some(expression.flow_type.ty.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing executable BITS call `{name}`"))
    };
    assert_eq!(
        call_type("Bits/slice"),
        boon_typecheck::Type::Bits { width: 3 }
    );
    assert_eq!(
        call_type("Bits/concat"),
        boon_typecheck::Type::Bits { width: 16 }
    );
    assert_eq!(
        call_type("Bits/sign_extend"),
        boon_typecheck::Type::Bits { width: 12 }
    );
    assert_eq!(
        call_type("Bits/to_bytes"),
        boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Fixed(2))
    );

    for (name, expected) in [
        ("slice", boon_plan::DataTypePlan::Bits { width: 3 }),
        ("concatenated", boon_plan::DataTypePlan::Bits { width: 16 }),
        ("extended", boon_plan::DataTypePlan::Bits { width: 12 }),
        (
            "encoded",
            boon_plan::DataTypePlan::Bytes { fixed_len: Some(2) },
        ),
    ] {
        let output = compiled
            .plan
            .output_root(name)
            .unwrap_or_else(|| panic!("missing output root `{name}`"));
        assert_eq!(
            output.contract,
            boon_plan::OutputContractKind::HostValue {
                data_type: expected
            }
        );
    }

    let plan_builtins = compiled
        .plan
        .row_expressions
        .iter()
        .filter_map(|(_, expression)| match expression {
            boon_plan::PlanRowExpressionNode::BuiltinCall { function, .. } => Some(*function),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert!(plan_builtins.contains(&PlanRowBuiltin::BitsSlice));
    assert!(plan_builtins.contains(&PlanRowBuiltin::BitsConcat));
    assert!(plan_builtins.contains(&PlanRowBuiltin::BitsSignExtend));
    assert!(plan_builtins.contains(&PlanRowBuiltin::BitsToBytes));

    let encoded = serde_json::to_vec(&compiled.plan).unwrap();
    let decoded: boon_plan::MachinePlan = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, compiled.plan);
    assert_eq!(verify_plan(&decoded).unwrap().status, "pass");
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
fn compiler_preserves_arbitrary_precision_integer_literals_exactly() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "exact-large-number.bn",
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
    .unwrap();
    let expected = "9007199254740993"
        .parse::<boon_plan::ExactNumber>()
        .unwrap();
    assert!(compiled.plan.constants.iter().any(|constant| {
        matches!(
            &constant.value,
            boon_plan::PlanConstantValue::Number { value } if value == &expected
        )
    }));
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn retained_document_constants_preserve_the_same_exact_number_domain() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "exact-document-number.bn",
        r#"
document: Document/new(
    root: Element/label(
        element: []
        style: [width: 9007199254740993123456789]
        label: TEXT { exact }
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let expected = "9007199254740993123456789"
        .parse::<boon_plan::ExactNumber>()
        .unwrap();
    let document = compiled.plan.document.as_ref().expect("document plan");
    assert!(document.constants.iter().any(|constant| {
        matches!(
            &constant.value,
            boon_plan::DocumentConstantValue::Number { value } if value == &expected
        )
    }));
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
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
fn tagged_payload_pattern_lowers_one_exact_document_projection() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "tagged-payload-document-pattern.bn",
        r#"
store: [
    selected: Found[value: TEXT { ready }]
]

document: Document/new(
    root: store.selected |> WHEN {
        Found[value] => Element/text(element: [], style: [], text: value)
        __ => Element/text(element: [], style: [], text: TEXT { missing })
    }
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled.plan.document.as_ref().expect("document plan");
    let found = document
        .expressions
        .iter()
        .find_map(|expression| {
            let DocumentExprOp::Select { arms, .. } = &expression.op else {
                return None;
            };
            arms.iter().find(|arm| {
                matches!(
                    arm.pattern,
                    boon_plan::DocumentPattern::Tag { tag }
                        if document.names.get(tag.0).map(String::as_str) == Some("Found")
                )
            })
        })
        .expect("Found select arm");
    assert_eq!(found.bindings.len(), 1);
    assert_eq!(
        found.bindings[0]
            .projection
            .iter()
            .map(|name| document.names[name.0].as_str())
            .collect::<Vec<_>>(),
        ["value"]
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
    let edit_state = list_memory
        .row_fields
        .iter()
        .find(|field| field.semantic_path == "store.todos.edit_text")
        .and_then(|field| field.runtime_field_id)
        .expect("indexed edit state persistence field");
    let edit_authority = list_memory
        .row_fields
        .iter()
        .find(|field| field.semantic_path == "store.todos.@authority:edit_text")
        .and_then(|field| field.runtime_field_id)
        .expect("edit constructor authority persistence field");
    let mut constructor_names = std::collections::BTreeSet::new();

    assert!(initial_fields.is_subset(&stable_fields));
    assert!(
        list_slot
            .row_fields
            .iter()
            .filter(|field| field.role.is_authority())
            .all(|field| constructor_names.insert(field.name.as_str())),
        "constructor authority names must be unique: {:#?}",
        list_slot.row_fields
    );
    assert_ne!(edit_state, edit_authority);
    assert_eq!(
        list_slot
            .row_fields
            .iter()
            .find(|field| field.field_id == edit_state)
            .map(|field| field.role),
        Some(PlanListRowFieldRole::Value)
    );
    assert_eq!(
        list_slot
            .row_fields
            .iter()
            .filter(|field| field.name == "edit_text" && field.role.is_authority())
            .map(|field| field.field_id)
            .collect::<Vec<_>>(),
        vec![edit_authority]
    );
    for name in ["title", "completed", "edit_text"] {
        assert_eq!(
            list_slot
                .row_fields
                .iter()
                .filter(|field| field.name == name && field.role.is_value())
                .count(),
            1,
            "{name} must have one public row value identity"
        );
        assert_eq!(
            list_slot
                .row_fields
                .iter()
                .filter(|field| field.name == name && field.role.is_authority())
                .count(),
            1,
            "{name} must have one constructor authority identity"
        );
    }
    let completed_value = list_slot
        .row_fields
        .iter()
        .find(|field| field.name == "completed" && field.role.is_value())
        .map(|field| field.field_id)
        .expect("completed public value field");
    assert_eq!(
        crate::machine_plan_backend::row_input_field_id_for_list_id(
            &compiled.ir,
            list_slot.list_id,
            "completed",
        ),
        Some(completed_value),
        "row reads must use the current public value rather than constructor authority"
    );
    let debug_id = |entries: &[boon_plan::DebugEntry], label: &str, prefix: &str| {
        entries
            .iter()
            .find(|entry| entry.label == label)
            .and_then(|entry| entry.id.strip_prefix(prefix))
            .and_then(|id| id.parse::<usize>().ok())
            .unwrap_or_else(|| panic!("missing debug identity `{prefix}<id>` for `{label}`"))
    };
    let toggle_source = boon_plan::SourceId(debug_id(
        &compiled.plan.debug_map.source_routes,
        "store.sources.toggle_all_checkbox.events.click",
        "source:",
    ));
    let completed_state = boon_plan::StateId(debug_id(
        &compiled.plan.debug_map.state_slots,
        "store.todos.completed",
        "state:",
    ));
    let all_completed_field = FieldId(debug_id(
        &compiled.plan.debug_map.derived_values,
        "store.all_completed",
        "field:",
    ));
    let store_field = FieldId(debug_id(&compiled.plan.debug_map.fields, "store", "field:"));
    let toggle_update = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                trigger: ValueRef::Source(source),
                value: Some(value),
                ..
            } if *source == toggle_source
                && op.output == Some(ValueRef::State(completed_state)) =>
            {
                Some(*value)
            }
            _ => None,
        })
        .expect("toggle-all completed update");
    let mut toggle_inputs = Vec::new();
    compiled
        .plan
        .row_expressions
        .visit_value_refs(toggle_update, &mut |input| {
            toggle_inputs.push(input.clone())
        })
        .unwrap();
    assert!(
        toggle_inputs.contains(&ValueRef::Field(all_completed_field)),
        "toggle-all update must read the exact derived leaf: {toggle_inputs:#?}"
    );
    assert!(
        !toggle_inputs.contains(&ValueRef::Field(store_field)),
        "toggle-all update must not materialize the structural store root: {toggle_inputs:#?}"
    );
    let selected_filter_state = boon_plan::StateId(debug_id(
        &compiled.plan.debug_map.state_slots,
        "store.selected_filter",
        "state:",
    ));
    let visible_todos_list = boon_plan::ListId(debug_id(
        &compiled.plan.debug_map.list_slots,
        "store.visible_todos",
        "list:",
    ));
    let visible_filter = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| {
            op.output == Some(ValueRef::List(visible_todos_list))
                && matches!(
                    &op.kind,
                    PlanOpKind::DerivedValue {
                        materialization: Some(_),
                        ..
                    }
                )
        })
        .expect("visible-todo filter rematerialization");
    assert!(
        visible_filter
            .inputs
            .contains(&ValueRef::State(selected_filter_state)),
        "the retained view must be current with its root filter state"
    );
    assert!(
        compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .all(|op| {
                op.output != Some(ValueRef::List(visible_todos_list))
                    || !matches!(&op.kind, PlanOpKind::ListMutation { .. })
            }),
        "a cross-list retained view is derived currentness, not list authority mutation"
    );
    let clear_completed_source = boon_plan::SourceId(debug_id(
        &compiled.plan.debug_map.source_routes,
        "store.sources.clear_completed_button.events.press",
        "source:",
    ));
    let authority_remove = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::ListMutation {
                mutation: PlanListMutation::Remove(remove),
            } if op.output == Some(ValueRef::List(list_slot.list_id))
                && remove.trigger == ValueRef::Source(clear_completed_source) =>
            {
                Some(remove)
            }
            _ => None,
        })
        .expect("clear-completed authority removal");
    assert_eq!(
        authority_remove.owner.static_owner,
        boon_plan::PlanStaticOwnerId::ROOT,
        "a root event owns list-authority activation independently of row-local predicates"
    );
    assert!(
        authority_remove.owner.ancestors.is_empty(),
        "a root event must not require a row trigger ancestor"
    );
    assert_ne!(
        authority_remove.local_owner,
        boon_plan::PlanStaticOwnerId::ROOT,
        "the authority predicate must retain its contextual row owner"
    );
    let row_remove_source = boon_plan::SourceId(debug_id(
        &compiled.plan.debug_map.source_routes,
        "store.todos.sources.remove_todo_button.events.press",
        "source:",
    ));
    let row_remove = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::ListMutation {
                mutation: PlanListMutation::Remove(remove),
            } if op.output == Some(ValueRef::List(list_slot.list_id))
                && remove.trigger == ValueRef::Source(row_remove_source) =>
            {
                Some(remove)
            }
            _ => None,
        })
        .expect("row-owned delete authority removal");
    assert_eq!(
        row_remove.owner.ancestors.len(),
        1,
        "a row-owned delete event must retain its exact source-row activation"
    );
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
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([(1, 2), (2, 3)])
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
            boon_plan::PlanRowSelectPattern::Tag {
                name: "False".to_owned(),
            },
            boon_plan::PlanRowSelectPattern::Tag {
                name: "True".to_owned(),
            },
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
                materialization:
                    Some(PlanListMaterialization {
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
        &arm.pattern,
        boon_plan::PlanRowSelectPattern::Tag { name } if name == "False"
    )));
    assert!(arms.iter().any(|arm| matches!(
        &arm.pattern,
        boon_plan::PlanRowSelectPattern::Tag { name } if name == "True"
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
fn compiler_root_demand_includes_lowered_nested_document_field_reads() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "nested-document-field-demand.bn",
        r#"
store: [
    change: SOURCE
    active:
        False |> HOLD active {
            change |> THEN { True }
        }
    visible_row_limit:
        32 |> HOLD visible_row_limit {
            change |> THEN { 64 }
        }
    request_descriptor: [
        visible_row_limit: store.visible_row_limit
    ]
]

FUNCTION descriptor_label() {
    Element/label(
        element: []
        style: []
        label: PASSED.store.request_descriptor.visible_row_limit |> Number/to_text(radix: 10)
    )
}

document: Document/new(
    root: store.active |> WHEN {
        True => descriptor_label(PASS: [store: store])
        False => Element/label(element: [], style: [], label: TEXT { inactive })
    }
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled.plan.document.as_ref().expect("document plan");
    let document_fields = document
        .expressions
        .iter()
        .filter_map(|expression| match &expression.op {
            DocumentExprOp::Read {
                read: DocumentRead::Field { field },
            } => Some(*field),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let RootOutputDemand::Selected(demanded) = &compiled.plan.demand.root_derived_outputs else {
        panic!("document demand must remain sparse");
    };
    let demanded = demanded.iter().copied().collect::<BTreeSet<_>>();
    let nested_descriptor_field = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find_map(|entry| {
            entry
                .label
                .ends_with("request_descriptor.visible_row_limit")
                .then(|| {
                    entry
                        .id
                        .strip_prefix("field:")
                        .and_then(|id| id.parse::<usize>().ok())
                        .map(FieldId)
                })
                .flatten()
        })
        .expect("nested descriptor leaf field");
    assert!(
        document_fields.is_subset(&demanded),
        "lowered document root reads escaped the demand plan: {:?}",
        document_fields.difference(&demanded).collect::<Vec<_>>()
    );
    assert!(
        document_fields.contains(&nested_descriptor_field),
        "fixture did not lower the nested descriptor leaf as an exact document field read"
    );
    assert!(
        compiled.plan.regions.iter().any(|region| {
            region.kind == boon_plan::RegionKind::DerivedEvaluation
                && region.ops.iter().any(|op| {
                    !op.indexed && op.output == Some(ValueRef::Field(nested_descriptor_field))
                })
        }),
        "exact nested descriptor field has no executable root computation"
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
    assert!(
        matches!(
            row_node(&compiled.plan.row_expressions, *default),
            PlanRowExpressionNode::Absent
        ),
        "an event-only list projection must remain privately absent before its first event"
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
            elements.ready.event.press |> THEN { True } |> WHEN {
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
        .find(|op| {
            matches!(
                &op.kind,
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
                )
            )
        })
        .expect("typed scalar-list nonempty host-effect guard");
    assert_eq!(guarded_effect.unresolved_executable_ref_count, 0);
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
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
            Some(PlanDerivedExpression::RowExpression {
                expression: materialized_expression,
            }),
        materialization:
            Some(PlanListMaterialization {
                target_list,
                fields: materialized_fields,
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
    let (owner, row_local, source, body) = expect_contextual_map(
        &compiled.plan.row_expressions,
        *materialized_expression,
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
        materialization: Some(materialization),
        ..
    } = &selected_op.kind
    else {
        panic!("selected operation lost its typed expression: {selected_op:#?}");
    };
    assert_eq!(materialization.target_list, selected);
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
        materialization: Some(materialization),
        ..
    } = &continued_op.kind
    else {
        panic!("continued operation lost its typed expression: {continued_op:#?}");
    };
    assert_eq!(materialization.target_list, continued);
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
fn initial_latest_routes_nested_cursor_values_through_host_effect_lowering() {
    let source = r#"
store: [
    request: SOURCE
    cursor_values:
        LATEST {
            NotStarted
            request |> THEN {
                Wellen/cursor_values(
                    artifact: request.artifact
                    request_fingerprint: request.request_fingerprint
                    cursor_time: request.cursor_time
                    signal_ids: request.signal_ids
                )
            }
        }
]
"#;

    let compiled = compile_fixture_source_text_to_machine_plan(
        "initial-latest-cursor-values.bn",
        source,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let cursor_effects = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } if compiled.plan.effects.iter().any(|contract| {
                contract.effect_id == effect.effect_id
                    && contract.host_operation == "Wellen/cursor_values"
            }) =>
            {
                Some(effect)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(cursor_effects.len(), 1, "{cursor_effects:#?}");
}

#[test]
fn effect_bearing_continuous_latest_branch_is_not_a_state_initializer() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "effect-bearing-continuous-latest-branch.bn",
        r#"
store: [
    enable: SOURCE
    retrigger: SOURCE
    enabled:
        False |> HOLD enabled {
            enable |> THEN { True }
        }
    clock_result:
        LATEST {
            NotRequested
            LATEST {
                enabled |> WHEN {
                    True => Clock/wall()
                    False => SKIP
                }
                retrigger |> THEN { Clock/wall() }
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(compiled.ir.state_cells.len(), 2);
    let clock_effects = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| {
            matches!(
                &op.kind,
                PlanOpKind::StateUpdate {
                    effect: Some(effect),
                    ..
                } if compiled.plan.effects.iter().any(|contract| {
                    contract.effect_id == effect.effect_id
                        && contract.host_operation == "Clock/wall"
                })
            )
        })
        .count();
    assert_eq!(clock_effects, 2);
}

#[test]
fn one_statement_retains_every_nested_initial_latest_state_binding() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "nested-initial-latest-state-bindings.bn",
        r#"
store: [
    inner_event: SOURCE
    value:
        LATEST {
            OuterIdle
            LATEST {
                InnerIdle
                inner_event |> THEN { Done }
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(compiled.ir.state_cells.len(), 2);
    assert_eq!(compiled.plan.storage_layout.scalar_slots.len(), 2);
    assert_eq!(
        compiled
            .ir
            .scope_index
            .bindings
            .iter()
            .filter(|binding| {
                matches!(binding.target, boon_ir::ErasedBindingTarget::State { .. })
            })
            .count(),
        2
    );
}

#[test]
fn detached_indexed_state_uses_the_exact_constructor_alias() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "detached-indexed-state-constructor-alias.bn",
        r#"
store: [
    toggle: SOURCE
    source_rows: LIST {
        [signal_id: TEXT { data_bus }]
    }
    aliases:
        source_rows
        |> List/map(item, new:
            new_alias(signal: alias_row(row: item))
        )
]

FUNCTION alias_row(row) {
    [
        key: row.signal_id
        bridge_key: row.signal_id
        label: row.signal_id
        selected_initial: True
    ]
}

FUNCTION new_alias(signal) {
    [
        key: signal.key
        bridge_key: signal.bridge_key
        label: signal.label
        selected_once:
            signal.selected_initial |> HOLD selected_once {
                store.toggle |> THEN {
                    signal.key == TEXT { data_bus } |> WHEN {
                        True => False
                        False => selected_once
                    }
                }
            }
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("the logical `key` read must disambiguate its shared `signal_id` source");

    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
    let constructor_aliases = compiled
        .ir
        .executable
        .expressions
        .iter()
        .filter_map(|expression| match &expression.kind {
            boon_ir::ExecutableExpressionKind::MaterializationLocal {
                projection,
                constructor_projection,
                ..
            } if projection == &["signal_id".to_owned()] => Some(constructor_projection.join(".")),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert!(
        ["key", "bridge_key", "label"]
            .into_iter()
            .all(|alias| constructor_aliases.contains(alias)),
        "shared source projection lost exact constructor aliases: {constructor_aliases:#?}"
    );
    let aliases = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| {
            compiled.plan.debug_map.list_slots.iter().any(|entry| {
                entry.label == "store.aliases" && entry.id == format!("list:{}", slot.list_id.0)
            })
        })
        .expect("aliases list slot");
    let field = |name: &str| {
        aliases
            .row_fields
            .iter()
            .find(|field| field.name == name && field.role.is_authority())
            .unwrap_or_else(|| panic!("aliases `{name}` authority field"))
            .field_id
    };
    let key = field("key");
    let bridge_key = field("bridge_key");
    let label = field("label");
    let mut update_inputs = BTreeSet::new();
    for value in compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                value: Some(value), ..
            } => Some(*value),
            _ => None,
        })
    {
        compiled
            .plan
            .row_expressions
            .visit_inputs(value, &mut |input| {
                if let ValueRef::Field(field) = input {
                    update_inputs.insert(field);
                }
            })
            .unwrap();
    }
    assert!(
        update_inputs.contains(&key),
        "detached state update did not read the logical `key` field: inputs={update_inputs:#?}; \
         alias fields={:#?}",
        aliases.row_fields
    );
    assert!(
        !update_inputs.contains(&bridge_key) && !update_inputs.contains(&label),
        "detached state update selected a sibling alias of `signal_id`: {update_inputs:#?}"
    );
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
            True => List/get(list: left, position: 1) |> WHEN {
                Found[value] => value
                NotFound => FLUSH { MissingLeftRow }
            }
            False => List/get(list: right, position: 1) |> WHEN {
                Found[value] => value
                NotFound => FLUSH { MissingRightRow }
            }
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
fn retained_document_can_consume_a_function_flush_boundary() {
    let path = example_path("examples/fibonacci.bn");
    let compiled = compile_source_path_to_machine_plan(&path, TargetProfile::SoftwareDefault)
        .expect("compiled retained FLUSH boundary");

    let document = compiled.plan.document.as_ref().expect("retained document");
    assert!(
        document.expressions.iter().any(|expression| {
            matches!(
                expression.op,
                DocumentExprOp::RuntimeExpression {
                    expression: runtime_expression,
                    ..
                } if matches!(
                    row_node(&compiled.plan.row_expressions, runtime_expression),
                    PlanRowExpressionNode::FlushBoundary { .. }
                )
            )
        }),
        "function FLUSH boundary must remain an exact runtime expression"
    );
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
fn shared_source_bundle_digest_v1_golden_compiles_canonical_client_bundle() {
    let fixture: SourceBundleGoldenFixture = serde_json::from_str(include_str!(
        "../../../fixtures/contracts/source_bundle_digest_v1.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema, "boon.source-bundle-golden.v1");

    let canonical = CanonicalSourceBundleV1::new(
        &fixture.entrypoint,
        fixture
            .units
            .iter()
            .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
    )
    .unwrap();
    assert_eq!(canonical.entrypoint(), fixture.canonical_entrypoint);
    assert_eq!(
        canonical
            .units()
            .iter()
            .map(|unit| unit.path())
            .collect::<Vec<_>>(),
        fixture
            .canonical_paths
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert_eq!(canonical.digest().to_string(), fixture.digest);

    let units = canonical
        .units()
        .iter()
        .map(|unit| CompilerSourceUnit {
            path: unit.path().to_owned(),
            source: unit.source().to_owned(),
        })
        .collect::<Vec<_>>();
    let compiled = compile_source_units_to_machine_plan(
        canonical.entrypoint(),
        &units,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled
        .plan
        .document
        .as_ref()
        .expect("golden client bundle must retain a visual document");

    assert_eq!(compiled.plan.program_role, ProgramRole::Client);
    assert_eq!(compiled.plan.outputs.len(), 1);
    assert_eq!(compiled.plan.outputs[0].name, "scene");
    assert_eq!(compiled.plan.outputs[0].contract, OutputContractKind::Scene);
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
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
}

#[test]
fn source_unit_entrypoint_must_name_an_exact_canonical_unit() {
    let units = vec![CompilerSourceUnit {
        path: "app/RUN.bn".to_owned(),
        source: "value: 1\n".to_owned(),
    }];
    let error = compile_source_units_to_machine_plan(
        "diagnostic-label-only",
        &units,
        TargetProfile::SoftwareDefault,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("entrypoint `diagnostic-label-only` is not one of its units"),
        "{error}"
    );
    let diagnostics = diagnose_runtime_source_units("diagnostic-label-only", &units);
    assert_eq!(diagnostics.len(), 1);
    assert!(
        diagnostics[0]
            .message
            .contains("entrypoint `diagnostic-label-only` is not one of its units")
    );
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
fn document_accepts_an_exact_nested_source_handle_without_treating_it_as_payload() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "document-nested-source-handle.bn",
        r#"
store: [
    elements: [
        button: SOURCE
    ]
]

document: Document/new(
    root: Element/label(
        element: [events: store.elements.button]
        style: []
        label: TEXT { Ready }
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let document = compiled.plan.document.as_ref().expect("document plan");
    assert!(document.expressions.iter().any(|expression| matches!(
        &expression.op,
        DocumentExprOp::Read {
            read: DocumentRead::Source { .. }
        }
    )));
    assert!(boon_plan::verify_plan(&compiled.plan).is_ok());
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
fn compiler_resolves_append_fields_from_the_exact_same_named_source_payload() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "append-source-payload-fields.bn",
        r#"
store: [
    elements: [
        draft: [completed: SOURCE]
        publish: [completed: SOURCE]
    ]
    draft_bootstrap:
        elements.draft.completed.bootstrap |> WHEN {
            True => Ready
            False => Pending
        }
    append_token:
        elements.publish.completed
        |> THEN { elements.publish.completed.digest }
    revisions:
        LIST {}
        |> List/append(item: append_token |> THEN {
            [
                digest: append_token
                compiler: elements.publish.completed.compiler
                target: elements.publish.completed.target
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
        source_label: format!("{role_name}/RUN.bn"),
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
        let expression = plan
            .regions
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
            .expect("producer result computation");
        let root = match expression {
            PlanDerivedExpression::RowExpression { expression }
            | PlanDerivedExpression::MaterializedRowField { expression, .. } => expression,
            other => panic!("producer result must retain its row expression: {other:?}"),
        };
        let mut nodes = Vec::new();
        plan.row_expressions
            .visit(root, &mut |_, node| nodes.push(node.clone()))
            .unwrap();
        let constants = nodes
            .iter()
            .filter_map(|node| match node {
                PlanRowExpressionNode::Constant { constant_id } => {
                    Some(plan.constants[constant_id.0].value.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        (expression, nodes, constants)
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
fn distributed_compiler_rejects_pass_across_runtime_islands() {
    let explicit = compile_distributed_compiler_test_bundle(
        &format!(
            "store: [\n    value: Session/read(PASS: [value: 1])\n]\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        "FUNCTION read() { 1 }\n",
        "store: [\n    ready: True\n]\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        explicit.contains("PASS")
            && (explicit.contains("external callable")
                || explicit.contains("across a runtime island")),
        "unexpected explicit distributed PASS diagnostic: {explicit}"
    );

    let required = compile_distributed_compiler_test_bundle(
        &format!(
            "store: [\n    value: Session/read()\n]\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
FUNCTION read() {
    PASSED.value
}
"#,
        "store: [\n    ready: True\n]\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        required
            .contains("distributed function `read` cannot require PASS across a runtime island"),
        "unexpected implicit distributed PASS diagnostic: {required}"
    );
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
        let server = r#"
store: [
    seed: 1
    saved: Server/store.saved
]
"#
        .to_string();
        // `principal` contains the authoritative roles LIST, so the language
        // foundations correctly reject putting that whole value in HOLD
        // before distributed scope validation runs. Keep the scope regression
        // independent of that second violation by retaining only its
        // collection-free authentication tag.
        let saved = if intrinsic == "status" {
            format!(
                "SessionInfo/{intrinsic}() |> HOLD saved {{\n            LATEST {{}}\n        }}"
            )
        } else {
            format!(
                "SessionInfo/{intrinsic}()\n        |> WHEN {{\n            Anonymous => Anonymous\n            Authenticated => Authenticated\n        }}\n        |> HOLD saved {{\n            LATEST {{}}\n        }}"
            )
        };
        let global_state = format!(
            r#"
store: [
    session_seed: Session/store.seed
    saved:
        {saved}
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
        let session = r#"
store: [
    info: Server/session_info(seed: 1)
]
"#
        .to_string();
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
fn distributed_compiler_resolves_transitive_producers_before_role_lowering() {
    let compiled = compile_distributed_compiler_test_bundle(
        &format!(
            "store: [\n    result: Session/outer(value: 5)\n]\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        r#"
store: [ready: True]

FUNCTION outer(value) {
    Server/double(value: value)
}
"#,
        r#"
store: [ready: True]

FUNCTION double(value) {
    value * 2
}
"#,
    )
    .unwrap();

    let client = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .expect("Client endpoint");
    let session = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .expect("Session endpoint");
    assert_eq!(client.remote_call_sites.len(), 1);
    assert_eq!(session.remote_call_sites.len(), 1);
    assert_eq!(
        compiled
            .program(ProgramRole::Session)
            .expect("Session plan")
            .plan
            .producer_function_instances
            .len(),
        1
    );
    assert_eq!(
        compiled
            .program(ProgramRole::Server)
            .expect("Server plan")
            .plan
            .producer_function_instances
            .len(),
        1
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
    let owned_state_slots = session_plan
        .storage_layout
        .scalar_slots
        .iter()
        .filter(|slot| instance.ownership.states.contains(&slot.state_id))
        .collect::<Vec<_>>();
    assert_eq!(owned_state_slots.len(), instance.ownership.states.len());
    assert!(owned_state_slots.iter().all(|slot| {
        slot.lifetime == boon_plan::PlanStateLifetime::Persistent
            && session_plan
                .persistence
                .memory
                .iter()
                .all(|memory| memory.runtime_slot != slot.id)
    }));
    let ValueRef::Field(result_field) = instance.result else {
        panic!("producer result must be a field")
    };
    let result_expression = session_plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                derived_kind: boon_plan::PlanDerivedKind::Pure,
                expression: Some(PlanDerivedExpression::RowExpression { expression }),
                ..
            } if op.output == Some(ValueRef::Field(result_field)) => Some(*expression),
            _ => None,
        })
        .expect("state-backed producer result must be a pure current expression");
    let mut result_inputs = BTreeSet::new();
    session_plan
        .row_expressions
        .visit_value_refs(result_expression, &mut |value| {
            result_inputs.insert(value.clone());
        })
        .unwrap();
    assert!(result_inputs.iter().any(
        |value| matches!(value, ValueRef::State(state) if instance.ownership.states.contains(state))
    ));
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
