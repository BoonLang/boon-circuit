use super::*;
use boon_plan::{
    DistributedRouteScopePlan, HostPortPlan, PlanListProjection, PlanQuerySelection,
    PlanSourceGuard, QueryKeyType, QueryTextNormalization, SourcePayloadField,
    distributed_graph_schema_hash,
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
            list: catalog
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
            list: catalog
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
            list: catalog
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
            list: catalog
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
    let guards = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::HostEffect,
                source_guard: Some(guard),
                ..
            } => Some(guard),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [PlanSourceGuard::All { guards }] = guards.as_slice() else {
        panic!("expected exactly one guarded nested host effect, got {guards:#?}");
    };
    assert_eq!(guards.len(), 2);
    assert!(matches!(
        &guards[0],
        PlanSourceGuard::ValueOneOf {
            input: ValueRef::State(_),
            values,
        } if values == &["Finished".to_owned()]
    ));
    assert!(matches!(
        &guards[1],
        PlanSourceGuard::ValueOneOf {
            input: ValueRef::StateProjection { field_path, .. },
            values,
        } if field_path == &["retained".to_owned()]
            && values == &["Retained".to_owned()]
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
        "unresolved: {:#?}; projections: {:#?}; lists: {:#?}; statements: {:#?}; expressions: {:#?}",
        compiled.plan.debug_map.unresolved_executable_refs,
        compiled.ir.list_projections,
        compiled.ir.lists,
        compiled.parsed.ast.statements,
        compiled.parsed.expressions,
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
    let verification = verify_plan(&compiled.plan).unwrap();
    assert_eq!(verification.status, "pass", "{:#?}", verification.checks);
    assert!(
        compiled.plan.capability_summary.cpu_plan_executor_complete,
        "capabilities: {:#?}; unresolved: {:#?}; unsupported: {:#?}",
        compiled.plan.capability_summary,
        compiled.plan.debug_map.unresolved_executable_refs,
        boon_plan::cpu_plan_executor_unsupported_ops(&compiled.plan),
    );
}

#[test]
fn compiler_owns_generic_compound_query_and_page_contract() {
    let compiled = compile_fixture_source_text_to_machine_plan(
        "generic-compound-query.bn",
        GENERIC_COMPOUND_QUERY_SOURCE,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
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
    assert!(
        matches!(
            expression,
            PlanDerivedExpression::RowExpression {
                expression: PlanRowExpression::BuiltinCall {
                    function,
                    input: Some(input),
                    args,
                },
            } if function == "Bool/not"
                && args.is_empty()
                && matches!(
                    input.as_ref(),
                    PlanRowExpression::NumberInfix { op, left, right }
                        if op == "=="
                            && matches!(left.as_ref(), PlanRowExpression::Field { input: ValueRef::State(_) })
                            && matches!(right.as_ref(), PlanRowExpression::Field { input: ValueRef::State(_) })
                )
        ),
        "unexpected root comparison expression: {expression:#?}"
    );
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
    OutputValueRef, PLAN_MAJOR_VERSION, PlanContextualOperationKind, PlanDerivedExpression,
    PlanExpressionKind, PlanLocalId, PlanOpKind, PlanRowExpression, PlanStaticOwnerId,
    RootOutputDemand, ValueRef, plan_binary, plan_sha256, verify_plan,
};

fn expect_contextual_map<'a>(
    expression: &'a PlanRowExpression,
    context: &str,
) -> (
    PlanStaticOwnerId,
    PlanLocalId,
    &'a PlanRowExpression,
    &'a PlanRowExpression,
) {
    let PlanRowExpression::ContextualCollection {
        owner,
        operation,
        source,
        row_local,
        body,
        ..
    } = expression
    else {
        panic!("{context} must lower as a typed contextual collection: {expression:#?}");
    };
    assert_eq!(
        *operation,
        PlanContextualOperationKind::Map,
        "{context} must retain the exact List/map operation"
    );
    (*owner, *row_local, source.as_ref(), body.as_ref())
}

fn expect_contextual_filter<'a>(
    expression: &'a PlanRowExpression,
    context: &str,
) -> (
    PlanStaticOwnerId,
    PlanLocalId,
    &'a PlanRowExpression,
    &'a PlanRowExpression,
) {
    let PlanRowExpression::ContextualCollection {
        owner,
        operation,
        source,
        row_local,
        body,
        ..
    } = expression
    else {
        panic!("{context} must lower as a typed contextual collection: {expression:#?}");
    };
    assert_eq!(
        *operation,
        PlanContextualOperationKind::Filter,
        "{context} must retain the exact List/filter operation"
    );
    (*owner, *row_local, source.as_ref(), body.as_ref())
}

fn assert_contextual_local_projection(
    expression: &PlanRowExpression,
    expected_owner: PlanStaticOwnerId,
    expected_local: PlanLocalId,
    expected_projection: &[&str],
    context: &str,
) {
    let PlanRowExpression::Local {
        owner,
        local,
        projection,
    } = expression
    else {
        panic!("{context} must read the typed contextual local: {expression:#?}");
    };
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
            value: ValueRef::List(_)
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
    let PlanRowExpression::BuiltinCall {
        function,
        input: Some(mapped),
        args: joined_args,
    } = transform
    else {
        panic!("terminal join call was not retained: {transform:#?}");
    };
    assert_eq!(function, "Text/join");
    assert_eq!(
        joined_args
            .iter()
            .filter_map(|arg| arg.name.as_deref())
            .collect::<Vec<_>>(),
        ["separator"]
    );
    let (map_owner, map_local, filtered, mapped_value) =
        expect_contextual_map(mapped, "HTTP query value projection");
    assert_contextual_local_projection(
        mapped_value,
        map_owner,
        map_local,
        &["value"],
        "HTTP query value projection",
    );
    let (owner, row_local, _source, predicate) =
        expect_contextual_filter(filtered, "HTTP query filter");
    let PlanRowExpression::NumberInfix { op, left, .. } = predicate else {
        panic!("HTTP query filter must retain its typed equality: {predicate:#?}");
    };
    assert_eq!(op, "==");
    assert_contextual_local_projection(
        left,
        owner,
        row_local,
        &["name"],
        "HTTP query parameter name",
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
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::HostEffect,
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
            PlanOpKind::UpdateBranch {
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
    let ValueRef::Constant(constant_id) = chunk_bytes.input else {
        panic!("defaulted chunk_bytes must lower to a plan constant");
    };
    let Some(boon_plan::PlanConstantValue::Number { value }) = compiled
        .plan
        .constants
        .iter()
        .find(|constant| constant.id == constant_id)
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
            PlanOpKind::UpdateBranch {
                effect: Some(effect),
                ..
            } if effect.effect_id == stream.effect_id => Some(effect),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        invocations.len() > 1,
        "the regression fixture must lower one call site through multiple possible causes"
    );
    assert_eq!(
        invocations
            .iter()
            .map(|invocation| invocation.invocation_id)
            .collect::<BTreeSet<_>>()
            .len(),
        1,
        "trigger specialization must not split one effect result owner"
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
    assert!(
        matches!(
            (&operation.output, &operation.kind),
            (
                Some(ValueRef::Field(output)),
                PlanOpKind::DerivedValue {
                    expression: Some(PlanDerivedExpression::RowExpression {
                        expression: PlanRowExpression::TaggedObject { tag, .. },
                    }),
                    ..
                }
            ) if *output == field && tag == "PackageAsset"
        ),
        "asset operation was not executable tagged data: {operation:#?}"
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
            .and_then(|slot| slot.initial_expression.clone())
            .unwrap_or_else(|| panic!("missing row default expression for `{path}`"))
    };

    assert!(matches!(
        initial_expression(&v5, "store.tasks.text"),
        PlanRowExpression::Field { .. }
    ));
    let PlanRowExpression::Select { input, arms } = initial_expression(&v6, "store.tasks.status")
    else {
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
    let PlanRowExpression::Constant { constant_id } = default.as_ref() else {
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
            PlanOpKind::UpdateBranch {
                source_guard: Some(PlanSourceGuard::All { guards }),
                effect: Some(_),
                ..
            } if guards
                .iter()
                .any(|guard| matches!(guard, PlanSourceGuard::ValuesEqual { .. })) =>
            {
                Some(op)
            }
            _ => None,
        })
        .expect("typed field-equality host-effect guard");
    assert_eq!(guarded_effect.unresolved_executable_ref_count, 0);
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
            PlanOpKind::UpdateBranch {
                source_guard: Some(PlanSourceGuard::ListIsNotEmpty { expected: true, .. }),
                effect: Some(_),
                ..
            } => Some(op),
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

    assert!(
        compiled
            .plan
            .debug_map
            .state_slots
            .iter()
            .any(|entry| { entry.label.ends_with(".value") && entry.label.contains("item") }),
        "canonical item output lost its keyed nested state: {:?}",
        compiled.plan.debug_map.state_slots
    );
    assert_eq!(compiled.plan.commit_plan.unresolved_update_branch_count, 0);
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
    let (owner, row_local, source, body) =
        expect_contextual_map(expression, "catalog materializer");
    let source_rows = compiled
        .ir
        .lists
        .iter()
        .find(|list| list.name.ends_with("source_rows"))
        .expect("source rows list");
    assert_eq!(
        source,
        &PlanRowExpression::ListRef {
            list_id: boon_plan::ListId(source_rows.id.0),
        },
        "catalog map must retain the exact source list"
    );
    let PlanRowExpression::Object { fields } = body else {
        panic!("catalog map must produce a record");
    };
    let label_projection = &fields
        .iter()
        .find(|field| field.name == "label")
        .expect("materialized catalog label")
        .value;
    assert_contextual_local_projection(
        label_projection,
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
    let (owner, row_local, source, body) = expect_contextual_map(expression, "selected list");
    let (filter_owner, filter_local, filter_source, predicate) =
        expect_contextual_filter(source, "selected source");
    let PlanRowExpression::ListRef {
        list_id: filter_list_id,
    } = filter_source
    else {
        panic!("selected filter must retain its typed list source: {filter_source:#?}");
    };
    let PlanRowExpression::NumberInfix { op, left, .. } = predicate else {
        panic!("selected filter must retain its typed equality: {predicate:#?}");
    };
    assert_eq!(op, "==");
    assert_contextual_local_projection(
        left,
        filter_owner,
        filter_local,
        &["file"],
        "selected filter file",
    );
    let PlanRowExpression::Object { fields } = body else {
        panic!("selected map must produce a record");
    };
    let file_projection = &fields
        .iter()
        .find(|field| field.name == "file")
        .expect("selected file field")
        .value;
    assert_contextual_local_projection(
        file_projection,
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
    let PlanDerivedExpression::RowExpression {
        expression: PlanRowExpression::Select { arms, .. },
    } = expression
    else {
        panic!("continued list must lower to a select expression");
    };
    let continued_map = &arms
        .iter()
        .find(|arm| {
            matches!(
                &arm.value,
                PlanRowExpression::ContextualCollection {
                    operation: PlanContextualOperationKind::Map,
                    ..
                }
            )
        })
        .unwrap_or_else(|| panic!("continued select lost its mapped arm: {arms:#?}"))
        .value;
    assert!(
        arms.iter().any(|arm| matches!(
            &arm.value,
            PlanRowExpression::ListLiteral { items } if items.is_empty()
        )),
        "continued select lost its empty fallback arm: {arms:#?}"
    );
    let (continued_owner, continued_local, continued_source, continued_body) =
        expect_contextual_map(continued_map, "continued mapped select arm");
    assert_eq!(
        continued_source,
        &PlanRowExpression::ListRef { list_id: selected },
        "continued map must retain the selected list as its exact source"
    );
    let PlanRowExpression::Object { fields } = continued_body else {
        panic!("continued map must produce a record: {continued_body:#?}");
    };
    let continued_file = &fields
        .iter()
        .find(|field| field.name == "file")
        .expect("continued file field")
        .value;
    assert_contextual_local_projection(
        continued_file,
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
    let (owner, row_local, source, body) = expect_contextual_map(expression, "mapped field");
    let selected_list = compiled
        .ir
        .derived_values
        .iter()
        .find(|derived| derived.path == "store.selected")
        .and_then(|derived| derived.materialized_list_id)
        .map(|list| boon_plan::ListId(list.0))
        .expect("selected materialized ListId");
    assert_eq!(
        source,
        &PlanRowExpression::ListRef {
            list_id: selected_list,
        },
        "mapped field must retain the exact derived-list source"
    );
    let PlanRowExpression::Object { fields } = body else {
        panic!("mapped field must produce a record: {body:#?}");
    };
    let label = &fields
        .iter()
        .find(|field| field.name == "label")
        .expect("mapped label field")
        .value;
    assert_contextual_local_projection(label, owner, row_local, &["id"], "mapped label field");
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
    let (owner, row_local, source, body) =
        expect_contextual_map(expression, "record-returning helper");
    let items = compiled
        .ir
        .lists
        .iter()
        .find(|list| list.name.ends_with("items"))
        .expect("items list");
    assert_eq!(
        source,
        &PlanRowExpression::ListRef {
            list_id: boon_plan::ListId(items.id.0),
        },
        "record-returning helper must retain the exact items source"
    );
    let PlanRowExpression::Object { fields } = body else {
        panic!("record-returning helper must produce a record: {body:#?}");
    };
    let label = &fields
        .iter()
        .find(|field| field.name == "label")
        .expect("decorated label")
        .value;
    assert_contextual_local_projection(label, owner, row_local, &["id"], "decorated label");
    let PlanRowExpression::Object {
        fields: detail_fields,
    } = &fields
        .iter()
        .find(|field| field.name == "details")
        .expect("decorated details")
        .value
    else {
        panic!(
            "decorated details must remain a record: {:#?}",
            fields
                .iter()
                .find(|field| field.name == "details")
                .expect("decorated details")
                .value
        );
    };
    let detail_value = &detail_fields
        .iter()
        .find(|field| field.name == "value")
        .expect("decorated detail value")
        .value;
    assert_contextual_local_projection(
        detail_value,
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
    let (owner, row_local, source, body) = expect_contextual_map(expression, "multiline helper");
    let (filter_owner, filter_local, filter_source, predicate) =
        expect_contextual_filter(source, "multiline helper filter");
    assert!(
        matches!(filter_source, PlanRowExpression::ListRef { .. }),
        "multiline helper filter must retain its typed list source: {filter_source:#?}"
    );
    let PlanRowExpression::NumberInfix { op, left, .. } = predicate else {
        panic!("multiline helper must retain its typed equality: {predicate:#?}");
    };
    assert_eq!(op, "==");
    assert_contextual_local_projection(
        left,
        filter_owner,
        filter_local,
        &["family"],
        "multiline helper family",
    );
    let PlanRowExpression::Object { fields } = body else {
        panic!("multiline helper map must produce a record: {body:#?}");
    };
    let label = &fields
        .iter()
        .find(|field| field.name == "label")
        .expect("multiline helper label")
        .value;
    assert_contextual_local_projection(label, owner, row_local, &["id"], "multiline helper label");
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
        arm_values.iter().any(|value| matches!(
            value,
            PlanRowExpression::Select { input, .. }
                if matches!(
                    input.as_ref(),
                    PlanRowExpression::ContextualCollection {
                        operation: PlanContextualOperationKind::Find,
                        ..
                    }
                )
        )),
        "missing scalar typed List/find: {arm_values:#?}"
    );
    assert!(
        arm_values.iter().any(|value| matches!(
            value,
            PlanRowExpression::BuiltinCall { function, .. } if function == "List/get"
        )),
        "missing scalar List/get: {arm_values:#?}"
    );
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
}

#[test]
fn stored_list_find_lowers_a_typed_field_id_index_lookup() {
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
    let PlanRowExpression::Select { input, .. } = expression else {
        panic!("selected find result must lower through WHEN: {expression:#?}");
    };
    let PlanRowExpression::ContextualCollection {
        owner,
        operation: PlanContextualOperationKind::Find,
        source,
        row_local,
        body,
        index_lookup: Some(index_lookup),
    } = input.as_ref()
    else {
        panic!("selected find must carry typed index metadata: {input:#?}");
    };
    assert_eq!(
        source.as_ref(),
        &PlanRowExpression::ListRef {
            list_id: index_lookup.list_id,
        }
    );
    let PlanRowExpression::NumberInfix { op, left, .. } = body.as_ref() else {
        panic!("typed find predicate lost equality: {body:#?}");
    };
    assert_eq!(op, "==");
    assert!(matches!(
        left.as_ref(),
        PlanRowExpression::ListRowField {
            row,
            list_id,
            field,
        } if *list_id == index_lookup.list_id
            && *field == index_lookup.field
            && matches!(
                row.as_ref(),
                PlanRowExpression::LocalRow {
                    owner: row_owner,
                    local,
                } if row_owner == owner && local == row_local
            )
    ));
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

    assert!(
        document
            .expressions
            .iter()
            .all(|expression| !matches!(expression.op, DocumentExprOp::FunctionCall { .. })),
        "transparent style helper must be erased before document lowering"
    );
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
    assert!(
        document
            .expressions
            .iter()
            .all(|expression| !matches!(expression.op, DocumentExprOp::FunctionCall { .. })),
        "transparent cross-module calls must be erased before document lowering"
    );
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
    [
        controls: [select: SOURCE]
        selected:
            False |> HOLD selected {
                controls.select |> THEN { True }
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
            |> List/map(item, new: render_row(row: item))
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let document = compiled.plan.document.as_ref().unwrap();

    assert!(
        document
            .expressions
            .iter()
            .all(|expression| !matches!(expression.op, DocumentExprOp::FunctionCall { .. })),
        "transparent row helpers must be erased before document lowering"
    );
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
        .find(|field| field.label == "item.address")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(boon_plan::FieldId)
        .expect("item.address field id");
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
        assert!(
            callee
                .endpoint
                .pure_function_exports
                .iter()
                .any(|function| {
                    function.export_id == edge.function_export_id
                        && function.parameters == edge.parameters
                        && function.result_type == edge.result_type
                })
        );
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
    assert_eq!(session.pure_function_exports.len(), 1);
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
    assert_ne!(
        baseline_session.pure_function_exports[0].body,
        changed_session.pure_function_exports[0].body
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
fn distributed_compiler_rejects_dependencies_in_the_wrong_role_direction() {
    let error = compile_distributed_compiler_test_bundle(
        &format!(
            "server_count: Server/store.count\n{}",
            DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
        ),
        "outputs: [\n    count: 1\n]\n",
        "store: [\n    count: 1\n]\n",
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("Client cannot depend directly on Server")
            && (message.contains("direction")
                || message.contains("cannot depend")
                || message.contains("route the value through Session")),
        "unexpected error: {message}"
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
    assert_eq!(session.pure_function_exports.len(), 1);
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
    let error = compile_distributed_compiler_test_bundle(
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT,
        r#"
store: [
    seed: 1
    saved: Server/store.saved
]
"#,
        r#"
store: [
    session_seed: Session/store.seed
    saved:
        SessionInfo/principal() |> HOLD saved {
            LATEST {}
        }
]
"#,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("outside an active Session scope"),
        "unexpected error: {error}"
    );
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
    let [identity] = session.pure_function_exports.as_slice() else {
        panic!("expected one identity export");
    };
    assert_eq!(identity.parameters[0].data_type, DataTypePlan::Number);
    assert_eq!(identity.result_type, DataTypePlan::Number);
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
fn distributed_compiler_rejects_effectful_function_exports() {
    let error = compile_distributed_compiler_test_bundle(
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT,
        "result: Server/logged(value: 1)\noutputs: [\n    ready: True\n]\n",
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
    [value: Session/add(value: item.value)]
}}

{}
"#,
        DISTRIBUTED_COMPILER_TEST_CLIENT_DOCUMENT
    );
    let error = compile_distributed_compiler_test_bundle(
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
