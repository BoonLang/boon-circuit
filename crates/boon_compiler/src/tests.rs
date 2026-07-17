use super::*;
use boon_plan::{
    HostPortPlan, PlanListProjection, PlanQuerySelection, QueryKeyType, QueryTextNormalization,
    SourcePayloadField,
};
use std::collections::BTreeSet;

const INDEXED_PREFIX_QUERY_SOURCE: &str = r#"
store: [
    change: SOURCE
    prefix:
        TEXT { al } |> HOLD prefix {
            change.text |> THEN { change.text }
        }
    catalog: LIST {
        [id: TEXT { 1 }, name: TEXT { Alpha }]
        [id: TEXT { 2 }, name: TEXT { Alpine }]
        [id: TEXT { 3 }, name: TEXT { Beta }]
    }
    results:
        List/query_prefix(
            catalog
            field: name
            prefix: prefix
            limit: 20
            normalization: TrimLowercase
        )
]
"#;

const GENERIC_COMPOUND_QUERY_SOURCE: &str = r#"
store: [
    catalog: LIST {
        [id: TEXT { 1 }, city: TEXT { Oslo }, name: TEXT { Alpha }, score: 10]
        [id: TEXT { 2 }, city: TEXT { Oslo }, name: TEXT { Beta }, score: 20]
        [id: TEXT { 3 }, city: TEXT { Bergen }, name: TEXT { Alpha }, score: 30]
    }
    exact_key: [city: TEXT { OSLO }, name: TEXT { alpha }]
    exact_page:
        List/query(
            catalog
            fields: TEXT { city,name }
            normalization: TEXT { TrimLowercase,TrimLowercase }
            select: Exact
            key: exact_key
            limit: 2
            unique: False
            order: Ascending
            residual: None
        )
]
"#;

const GENERIC_NUMBER_AND_TAG_QUERY_SOURCE: &str = r#"
store: [
    catalog: LIST {
        [id: TEXT { 1 }, score: 10, kind: Featured]
        [id: TEXT { 2 }, score: 20, kind: Ordinary]
        [id: TEXT { 3 }, score: 30, kind: Featured]
    }
    lower: 10
    upper: 25
    score_page:
        List/query(
            catalog
            fields: TEXT { score }
            normalization: TEXT { Exact }
            select: Range
            lower: lower
            upper: upper
            lower_inclusive: True
            upper_inclusive: False
            limit: 10
            order: Descending
            residual: None
        )
    kind: Featured
    kind_page:
        List/query(
            catalog
            fields: TEXT { kind }
            normalization: TEXT { Exact }
            select: Exact
            key: kind
            limit: 10
            order: Ascending
            residual: None
        )
]
"#;

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
fn compiler_owns_typed_prefix_query_index_plan() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "indexed-prefix-query.bn",
        INDEXED_PREFIX_QUERY_SOURCE,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    assert_eq!(
        compiled.plan.query_indexes.len(),
        1,
        "unresolved: {:#?}",
        compiled.plan.debug_map.unresolved_executable_refs
    );
    let index = &compiled.plan.query_indexes[0];
    assert_eq!(index.source_semantic_path, "catalog");
    assert_eq!(index.fields[0].semantic_path, "catalog.name");
    assert_eq!(
        index.fields[0].normalization,
        QueryTextNormalization::TrimLowercase
    );
    assert!(
        compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .any(|op| matches!(
                &op.kind,
                PlanOpKind::ListProjection {
                    projection: PlanListProjection::TextPrefix {
                        index: query_index,
                        limit: 20,
                        ..
                    }
                } if query_index == &index.id
            ))
    );
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
}

#[test]
fn compiler_owns_generic_compound_query_and_page_contract() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "generic-compound-query.bn",
        GENERIC_COMPOUND_QUERY_SOURCE,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert!(
        compiled.ir.typecheck_report.diagnostics.is_empty(),
        "{:#?}",
        compiled.ir.typecheck_report.diagnostics
    );
    let [index] = compiled.plan.query_indexes.as_slice() else {
        panic!(
            "expected one canonical compound index, got {:?}; unresolved={:?}",
            compiled.plan.query_indexes, compiled.plan.debug_map.unresolved_executable_refs
        );
    };
    assert_eq!(index.fields.len(), 2);
    assert!(
        index
            .fields
            .iter()
            .all(|field| field.key_type == QueryKeyType::Text)
    );
    assert!(
        compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .any(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::ListProjection {
                        projection: PlanListProjection::IndexedQuery {
                            selection: PlanQuerySelection::Exact { .. },
                            limit: 2,
                            ..
                        }
                    }
                )
            })
    );
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
}

#[test]
fn compiler_owns_number_range_and_tag_exact_indexes() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "generic-number-tag-query.bn",
        GENERIC_NUMBER_AND_TAG_QUERY_SOURCE,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert!(
        compiled.ir.typecheck_report.diagnostics.is_empty(),
        "{:#?}",
        compiled.ir.typecheck_report.diagnostics
    );
    assert_eq!(
        compiled
            .plan
            .query_indexes
            .iter()
            .flat_map(|index| index.fields.iter().map(|field| field.key_type))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([QueryKeyType::Number, QueryKeyType::Tag])
    );
    assert!(
        compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .any(|op| {
                matches!(
                    &op.kind,
                    PlanOpKind::ListProjection {
                        projection: PlanListProjection::IndexedQuery {
                            selection: PlanQuerySelection::Range { .. },
                            ..
                        }
                    }
                )
            })
    );
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
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
                &compiled.plan.storage_layout.scalar_slots,
                &compiled.plan.storage_layout.list_slots,
                &compiled.plan.constants,
                op,
                &empty,
                &empty,
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
            PlanOpKind::UpdateBranch {
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
    assert!(matches!(
        expression,
        PlanDerivedExpression::BoolNotExpression { input }
            if matches!(
                input.as_ref(),
                PlanDerivedExpression::ValueCompare {
                    left: ValueRef::State(_),
                    op,
                    right: ValueRef::State(_),
                } if op == "=="
            )
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
            matches!(
                op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::TextToNumber,
                    ..
                }
            )
        })
        .expect("TextToNumber update op");
    let PlanOpKind::UpdateBranch {
        ordered_inputs,
        source_payload_field,
        ..
    } = &update.kind
    else {
        unreachable!();
    };
    assert!(matches!(
        source_payload_field,
        Some(boon_plan::SourcePayloadField::Named(name)) if name == "amount"
    ));
    assert!(matches!(
        ordered_inputs.as_slice(),
        [ValueRef::SourcePayload {
            field: boon_plan::SourcePayloadField::Named(name),
            ..
        }] if name == "amount"
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
use boon_plan::{
    DataTypePlan, DocumentExprId, DocumentExprOp, DocumentMaterializationSource, DocumentRead,
    DocumentValueClass, EffectBarrier, EffectReplay, EffectResultPolicy, EffectResultRoute,
    FieldId, MemoryId, MemoryKind, MigrationExpressionPlan, MigrationPredecessorBinding,
    MigrationTransferKindPlan, MigrationTransformPlan, OutputContractKind, OutputDemandPolicy,
    OutputValueRef, PLAN_MAJOR_VERSION, PlanDerivedExpression, PlanExpressionKind, PlanOpKind,
    PlanRowExpression, RootOutputDemand, ValueRef, plan_binary, plan_sha256, verify_plan,
};

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
            "{relative_path} emitted an invalid MachinePlan: {:?}",
            verification
                .checks
                .iter()
                .filter(|check| !check.pass)
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
    assert_eq!(slot.initial_value_kind, boon_plan::InitialValueKind::Text);
    let constant = &compiled.plan.constants[slot.initial_constant_id.unwrap().0].value;
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
            value: ValueRef::Field(_)
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
            value: ValueRef::Field(_)
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
                &compiled.plan.storage_layout.scalar_slots,
                &compiled.plan.storage_layout.list_slots,
                &compiled.plan.constants,
                op,
                &empty,
                &empty,
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
        LATEST {
            request.method |> THEN {
                request.query
                    |> List/filter_field_equal(
                        field: "name"
                        value: TEXT { q }
                    )
                    |> List/join_field(
                        field: "value"
                        separator: Text/empty()
                    )
            }
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
    let PlanRowExpression::BuiltinCall {
        function,
        input: Some(filtered),
        args: joined_args,
    } = transform
    else {
        panic!("terminal join call was not retained: {transform:#?}");
    };
    assert_eq!(function, "List/join_field");
    assert_eq!(
        joined_args
            .iter()
            .filter_map(|arg| arg.name.as_deref())
            .collect::<Vec<_>>(),
        ["field", "separator"]
    );
    let PlanRowExpression::BuiltinCall {
        function,
        args: filter_args,
        ..
    } = filtered.as_ref()
    else {
        panic!("filter call was not retained: {filtered:#?}");
    };
    assert_eq!(function, "List/filter_field_equal");
    assert_eq!(
        filter_args
            .iter()
            .filter_map(|arg| arg.name.as_deref())
            .collect::<Vec<_>>(),
        ["field", "value"]
    );
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
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
                &compiled.plan.storage_layout.scalar_slots,
                &compiled.plan.storage_layout.list_slots,
                &compiled.plan.constants,
                op,
                &empty,
                &empty,
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
    assert_eq!(compiled.plan.query_indexes.len(), 1);
    let stations = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == compiled.plan.query_indexes[0].source_list)
        .expect("station catalog list");
    assert_eq!(stations.initial_rows.len(), 5);
    assert!(stations.initial_rows.iter().all(|row| {
        ["id", "kind", "latitude", "longitude", "modes", "name"]
            .into_iter()
            .all(|name| row.fields.iter().any(|field| field.name == name))
    }));
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
fn compiler_uses_central_host_effect_contracts_and_lowers_transactional_writes() {
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
    assert!(matches!(
        contract.replay,
        EffectReplay::Idempotent {
            key_type: DataTypePlan::Bytes {
                fixed_len: Some(32)
            }
        }
    ));
    assert_eq!(contract.barrier, EffectBarrier::BeforeAndAfter);
    assert!(
        write
            .plan
            .persistence
            .effect_outbox
            .iter()
            .any(|schema| schema.effect_id == contract.effect_id)
    );
    let invocation = write
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::FileWriteBytes,
                effect,
                ..
            } => effect.as_ref(),
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
        ["bytes", "path"]
    );
    assert!(
        verify_plan(&write.plan)
            .unwrap()
            .checks
            .iter()
            .all(|check| check.pass),
        "compiled transactional write plan must verify"
    );
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
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::HostEffect,
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
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::HostEffect,
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
        matches!(headers.input, ValueRef::List(_)),
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
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::HostEffect,
                    effect: Some(effect),
                    ..
                } if compiled.plan.effects.iter().any(|contract|
                    contract.effect_id == effect.effect_id
                        && contract.host_operation == "Random/bytes"))
        })
        .expect("Random/bytes plan op");
    let PlanOpKind::UpdateBranch { trigger, .. } = &random.kind else {
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
    let PlanRowExpression::TaggedObject { tag, fields } = expression else {
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
                Text/is_empty(workspace_id) |> WHEN {
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

    let branch = compiled
        .ir
        .update_branches
        .iter()
        .find(|branch| {
            branch.target == "store.workspace_id" && branch.source == "store.lifecycle.started"
        })
        .unwrap();
    let boon_ir::UpdateExpression::MatchTextIsEmptyConst { input, arms } = &branch.expression
    else {
        panic!("unexpected update expression: {:?}", branch.expression);
    };
    assert_eq!(input, "store.workspace_id");
    assert!(arms.iter().any(|arm| {
        arm.pattern == "True"
            && matches!(
                &arm.output,
                boon_ir::UpdateValueExpression::ReadPath { path }
                    if path == "store.lifecycle.started.workspace_id"
            )
    }));
    assert!(arms.iter().any(|arm| {
        arm.pattern == "False"
            && matches!(
                &arm.output,
                boon_ir::UpdateValueExpression::ReadPath { path }
                    if path == "store.workspace_id"
            )
    }));

    let op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| {
            matches!(
                op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::MatchTextIsEmptyConst,
                    ..
                }
            )
        })
        .unwrap();
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &op.kind else {
        unreachable!();
    };
    assert!(
        ordered_inputs
            .iter()
            .any(|input| matches!(input, ValueRef::State(_)))
    );
    assert!(ordered_inputs.iter().any(|input| {
        matches!(
            input,
            ValueRef::SourcePayload {
                field: boon_plan::SourcePayloadField::Named(name),
                ..
            } if name == "workspace_id"
        )
    }));
    assert!(
        compiled.plan.capability_summary.cpu_plan_executor_complete,
        "call-derived match op must be CPU-executable: {op:?}"
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
                entry.id == format!("list:{}", slot.list_id.0) && entry.label == "todos"
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
            .any(|field| field.semantic_path == "store.todos.$input$title")
    );
    assert!(
        list_memory
            .row_fields
            .iter()
            .any(|field| field.semantic_path == "store.todos.$input$completed")
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
            LATEST { events.alpha |> THEN { alpha + 1 } }
        }
    beta:
        0 |> HOLD beta {
            LATEST { events.beta |> THEN { beta + 1 } }
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
            LATEST { events.beta |> THEN { beta + 1 } }
        }
    primary: LIST {
        [label: TEXT { primary }]
    }
    alpha:
        0 |> HOLD alpha {
            LATEST { events.alpha |> THEN { alpha + 1 } }
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
        LATEST { events |> THEN { 1 } }
    }
"#;
    let text = r#"
events: SOURCE
value:
    TEXT { zero } |> HOLD value {
        LATEST { events |> THEN { TEXT { one } } }
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
            .map(|arm| arm.pattern.as_slice())
            .collect::<Vec<_>>(),
        vec![&["False".to_owned()][..], &["True".to_owned()][..]]
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
    |> List/map(todo, new: keep_row(row: todo))
    |> DRAINING

tasks:
    DRAIN { todos }
    |> List/map(task, new: keep_row(row: task))
"#;
    let indexed = r#"
todos:
    LIST { [title: TEXT { one }, text: TEXT { unset }] }
    |> List/map(todo, new: new_todo(todo: todo))

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
            .unwrap_or_else(|| panic!("missing persistence memory `{path}`"));
        plan.storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.id == memory.runtime_slot)
            .and_then(|slot| slot.initial_row_expression.clone())
            .unwrap_or_else(|| panic!("missing row default expression for `{path}`"))
    };

    assert!(matches!(
        initial_expression(&v5, "task.text"),
        PlanRowExpression::Field { .. }
    ));
    let PlanRowExpression::Select { input, arms } = initial_expression(&v6, "task.status") else {
        panic!("pure indexed migration must compile to a sparse Select default");
    };
    assert!(matches!(input.as_ref(), PlanRowExpression::Field { .. }));
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
    LATEST { 1 }
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
    rows:
        LIST {
            [label: TEXT { First }]
        }
        |> List/map(model_item, new: new_row(item: model_item))
    row_selected:
        rows
        |> List/map(event_item, new: LATEST {
            event_item.controls.select.event.press |> THEN { event_item.label }
        })
        |> List/latest()
    selected:
        TEXT { none } |> HOLD selected {
            LATEST { row_selected }
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
        expression: Some(PlanDerivedExpression::SourceEventTransform { default, .. }),
        ..
    } = &op.kind
    else {
        panic!("row projection must lower to a source-event transform");
    };
    let PlanRowExpression::Constant { constant_id } = default.as_ref() else {
        panic!("event-only list projection must use a typed scalar default");
    };
    assert_eq!(
        compiled.plan.constants[constant_id.0].value,
        boon_plan::PlanConstantValue::Text {
            value: String::new()
        }
    );
    let verification = verify_plan(&compiled.plan).unwrap();
    assert_eq!(
        verification.status,
        "pass",
        "invalid list-event projection plan: {:?}",
        verification
            .checks
            .iter()
            .filter(|check| !check.pass)
            .collect::<Vec<_>>()
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
    let PlanOpKind::UpdateBranch {
        update_constant_id: Some(constant_id),
        ..
    } = &reset_op.kind
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
        LATEST { sources.events |> THEN { value + 1 } }
    }
    items: LIST {
        [id: TEXT { a }]
        [id: TEXT { b }]
    }
    selected:
        True |> WHEN {
            True => items |> List/filter_field_equal(field: "id", value: TEXT { a })
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

    let field_id = |label: &str| {
        compiled
            .plan
            .debug_map
            .fields
            .iter()
            .find(|field| field.label == label)
            .and_then(|field| field.id.strip_prefix("field:"))
            .and_then(|id| id.parse::<usize>().ok())
            .map(boon_plan::FieldId)
            .unwrap_or_else(|| panic!("missing field `{label}`"))
    };
    let selected = field_id("store.selected");
    let mapped = field_id("store.mapped");
    let mapped_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::Field(mapped)))
        .expect("mapped operation");
    let PlanOpKind::DerivedValue {
        expression:
            Some(PlanDerivedExpression::RowExpression {
                expression: PlanRowExpression::ListMap { input, .. },
            }),
        ..
    } = &mapped_op.kind
    else {
        panic!("mapped must lower as a list map: {mapped_op:#?}");
    };

    assert_eq!(
        input.as_ref(),
        &PlanRowExpression::Field {
            input: ValueRef::Field(selected),
        }
    );
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
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.mapped")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(boon_plan::FieldId)
        .expect("mapped field");
    let mapped_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::Field(mapped)))
        .expect("mapped operation");

    assert!(matches!(
        mapped_op.kind,
        PlanOpKind::DerivedValue {
            expression: Some(PlanDerivedExpression::RowExpression {
                expression: PlanRowExpression::ListMap { .. },
            }),
            ..
        }
    ));
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
        |> List/filter_field_equal(field: "family", value: TEXT { kept })
        |> List/map(item, new: [label: item.id])
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();

    let mapped = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.mapped")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(boon_plan::FieldId)
        .expect("mapped field");
    let mapped_op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::Field(mapped)))
        .expect("mapped operation");

    assert!(matches!(
        mapped_op.kind,
        PlanOpKind::DerivedValue {
            expression: Some(PlanDerivedExpression::RowExpression {
                expression: PlanRowExpression::ListMap { .. },
            }),
            ..
        }
    ));
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

    assert!(document.functions.iter().any(|function| {
        let DocumentExprOp::Record { fields } = &document.expressions[function.body.0].op else {
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
                    |> List/map(project, new: project_row(project: project))
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
    let DocumentExprOp::FunctionCall { arguments, .. } =
        &document.expressions[document.root.expression.0].op
    else {
        panic!("scene root is not the ProfilePage call");
    };
    let DocumentExprOp::Record { fields: record } = &document.expressions[arguments[0].value.0].op
    else {
        panic!("ProfilePage call did not receive an explicit typed global record");
    };
    let demanded = match &compiled.plan.demand.root_derived_outputs {
        RootOutputDemand::Selected(fields) => fields.iter().copied().collect::<BTreeSet<_>>(),
        RootOutputDemand::All => panic!("document demand must remain sparse"),
    };
    let mut direct_field_count = 0;
    let mut direct_list_count = 0;
    for record_field in record {
        match document.expressions[record_field.value.0].op {
            DocumentExprOp::Read {
                read: DocumentRead::Field { field },
            } => {
                direct_field_count += 1;
                assert!(demanded.contains(&field));
            }
            DocumentExprOp::Read {
                read: DocumentRead::List { .. },
            } => direct_list_count += 1,
            _ => panic!("global record member did not lower to a direct typed read"),
        }
    }
    assert_eq!(direct_field_count, 1);
    assert_eq!(direct_list_count, 1);
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
        |> List/map(row, new: new_row(title: row.title, kind: row.kind))
]

FUNCTION new_row(title, kind) {
    [
        controls: [select: SOURCE]
        selected:
            False |> HOLD selected {
                LATEST { controls.select |> THEN { True } }
            }
        title: title
        kind: kind
    ]
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
            |> List/map(row, new: render_row(row: row))
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled.plan.document.as_ref().unwrap();

    assert!(document.expressions.iter().any(|expression| {
        let DocumentExprOp::FunctionCall { arguments, .. } = &expression.op else {
            return false;
        };
        arguments.iter().any(|argument| {
            matches!(
                document.expressions[argument.value.0].op,
                DocumentExprOp::Read {
                    read: DocumentRead::Parameter {
                        ref projection,
                        ..
                    }
                } if projection.is_empty()
            )
        })
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

    assert!(!document.materializations.is_empty());
    assert!(document.expressions.len() < 2_600);
    assert!(document.templates.len() < 2_600);
    assert!(document.materializations.iter().any(|materialization| {
        matches!(
            materialization.source,
            DocumentMaterializationSource::List { .. }
        )
    }));
    assert!(document.materializations.iter().any(|materialization| {
        matches!(
            materialization.source,
            DocumentMaterializationSource::Field { .. }
                | DocumentMaterializationSource::ScopedField { .. }
                | DocumentMaterializationSource::ParameterField { .. }
        )
    }));
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
        .find(|field| field.label == "cell.address")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(boon_plan::FieldId)
        .expect("cell.address field id");
    assert!(!document.expressions.iter().any(|expression| {
        matches!(
            expression.op,
            DocumentExprOp::Read {
                read: DocumentRead::Field { field }
            } if field == address_field
        )
    }));
    let editing_state = compiled
        .plan
        .debug_map
        .state_slots
        .iter()
        .find(|state| state.label == "cell.editing_text")
        .and_then(|state| state.id.strip_prefix("state:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(boon_plan::StateId)
        .expect("cell.editing_text state id");
    assert!(!document.expressions.iter().any(|expression| {
        matches!(
            expression.op,
            DocumentExprOp::Read {
                read: DocumentRead::State { state }
            } if state == editing_state
        )
    }));

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
value: 0 |> HOLD value { LATEST { events |> THEN { value } } }
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
        LATEST {
            pulse |> THEN { count + 10 }
        }
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
        LATEST {
            completed |> THEN { completed.digest }
        }
    revisions:
        LIST {}
        |> List/append(item: append_token |> THEN {
            [
                digest: append_token
                compiler: completed.compiler
                target: completed.target
            ]
        })
        |> List/map(revision, new: revision_view(revision: revision))
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
                op.kind,
                PlanOpKind::ListOperation {
                    operation_kind: boon_plan::PlanListOperationKind::Append,
                    ..
                }
            )
        })
        .expect("append op");
    let PlanOpKind::ListOperation {
        append: Some(append),
        ..
    } = &append_op.kind
    else {
        unreachable!();
    };
    assert_eq!(append_op.unresolved_executable_ref_count, 0);
    for name in ["compiler", "target"] {
        let field = append
            .fields
            .iter()
            .find(|field| field.name == name)
            .expect("payload-backed append field");
        assert!(matches!(
            &field.value_ref,
            Some(ValueRef::SourcePayload {
                field: boon_plan::SourcePayloadField::Named(payload_name),
                ..
            }) if payload_name == name
        ));
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
    server_count: Server.store.count
    session_count: Session.outputs.adjusted_count
    sum: Server/add(value: operand + server_count + session_count)
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
    server_count: Server.store.count
    adjusted_count: server_count + 1
]

outputs: [
    adjusted_count: store.adjusted_count
]
"#,
        r#"
store: [
    increment: SOURCE
    count:
        40 |> HOLD count {
            LATEST {
                increment |> THEN { count + 1 }
            }
        }
]

outputs: [
    count: store.count
]

FUNCTION add(value) {
    value + 2
}
"#,
    )
    .unwrap();

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
    assert_eq!(server.pure_function_exports.len(), 1);
    assert_eq!(server.pure_function_exports[0].parameters.len(), 1);

    let session = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .unwrap();
    assert_eq!(session.value_imports.len(), 1);
    assert_eq!(session.value_exports.len(), 1);
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
    assert_eq!(client.value_imports.len(), 2);
    let [call] = client.remote_call_sites.as_slice() else {
        panic!(
            "expected one remote call, got {:?}",
            client.remote_call_sites
        );
    };
    let [argument] = call.arguments.as_slice() else {
        panic!(
            "expected one remote call argument, got {:?}",
            call.arguments
        );
    };
    assert!(
        matches!(argument.value, PlanRowExpression::NumberInfix { .. }),
        "remote argument was not preserved as a compound pure expression: {:?}",
        argument.value
    );
    let client_plan = &compiled.program(ProgramRole::Client).unwrap().plan;
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
        .chain([call.result_import_id])
        .collect::<BTreeSet<_>>();
    assert!(
        expected_client_imports.is_subset(&client_operation_imports),
        "missing executable distributed imports: expected {expected_client_imports:?}, got {client_operation_imports:?}"
    );
}

#[test]
fn distributed_compiler_rejects_dependencies_in_the_wrong_role_direction() {
    let error = compile_distributed_compiler_test_bundle(
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT,
        "outputs: [\n    count: 1\n]\n",
        "outputs: [\n    count: Session.outputs.count\n]\n",
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("Session.outputs.count")
            && (message.contains("direction")
                || message.contains("cannot depend")
                || message.contains("unknown qualified external value")),
        "unexpected error: {message}"
    );
}

#[test]
fn distributed_compiler_rejects_effectful_function_exports() {
    let client = format!(
        "result: Server/logged(value: 1)\n{}",
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
    );
    let error = compile_distributed_compiler_test_bundle(
        &client,
        "outputs: [\n    ready: True\n]\n",
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
        message.contains("Server/logged")
            || message.contains("distributed function `logged`")
            || message.contains("pure"),
        "unexpected error: {message}"
    );
}

#[test]
fn distributed_compiler_rejects_remote_calls_inside_list_rows() {
    let client = format!(
        r#"
store: [
    items: LIST {{ [value: 1] }}
    rows:
        items
        |> List/map(item, new: decorate(item: item))
]

FUNCTION decorate(item) {{
    [value: Server/add(value: item.value)]
}}

{}
"#,
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
    );
    let error = compile_distributed_compiler_test_bundle(
        &client,
        "outputs: [\n    ready: True\n]\n",
        r#"
outputs: [
    ready: True
]

FUNCTION add(value) {
    value + 1
}
"#,
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("non-indexed root value")
            || message.contains("scheduled call-site identity")
            || message.contains("reusable functions")
            || (message.contains("qualified external expression")
                && message.contains("no checked type")),
        "unexpected error: {message}"
    );
}
