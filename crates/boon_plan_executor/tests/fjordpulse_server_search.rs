use boon_plan::{
    OutputValueRef, PlanListAccessSelection, ProgramRole, TargetProfile, ValueRef,
    cpu_plan_executor_supports_whole_plan_op,
};
use boon_plan_executor::{
    MachineInstance, SessionOptions, SourceEvent, SourcePayload, Value, ValueTarget,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const SERVER_PATH: &str = "examples/fjordpulse/Server/RUN.bn";
const SERVER_SOURCE: &str = include_str!("../../../examples/fjordpulse/Server/RUN.bn");
const SHARED_PATH: &str = "examples/fjordpulse/Shared/FjordPulseContract.bn";
const SHARED_SOURCE: &str =
    include_str!("../../../examples/fjordpulse/Shared/FjordPulseContract.bn");

fn number(value: i64) -> Value {
    Value::integer(value).unwrap()
}

fn list_field(
    plan: &boon_plan::MachinePlan,
    list_name: &str,
    field_name: &str,
) -> (boon_plan::ListId, boon_plan::FieldId) {
    let list = plan
        .debug_map
        .list_slots
        .iter()
        .find(|entry| entry.label == list_name)
        .and_then(|entry| entry.id.strip_prefix("list:"))
        .and_then(|id| id.parse().ok())
        .map(boon_plan::ListId)
        .unwrap_or_else(|| panic!("missing list debug label `{list_name}`"));
    let field = plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == list)
        .and_then(|slot| {
            slot.row_fields
                .iter()
                .find(|field| field.name == field_name)
        })
        .map(|field| field.field_id)
        .unwrap_or_else(|| panic!("missing `{field_name}` field on `{list_name}`"));
    (list, field)
}

fn station_ids(
    session: &MachineInstance,
    list: boon_plan::ListId,
    id_field: boon_plan::FieldId,
) -> Vec<i64> {
    session
        .list_row_snapshots(list)
        .expect("station rows must remain readable")
        .into_iter()
        .map(|row| {
            let Some(Value::Number(id)) = row.fields.get(&id_field) else {
                panic!("station access item must preserve its numeric id")
            };
            id.to_i64_exact().expect("station id must remain integral")
        })
        .collect()
}

fn station_prefix_ids(prefix: &str, bergen_only: bool) -> Vec<i64> {
    let mut ids = (0_i64..=58_499)
        .filter(|id| !bergen_only || *id >= 58_400)
        .filter(|id| format!("station-{id}").starts_with(prefix))
        .collect::<Vec<_>>();
    ids.sort_by_key(|id| format!("station-{id}"));
    ids.truncate(20);
    ids
}

#[test]
fn fjordpulse_server_search_source_uses_a_typed_normalized_pipeline() {
    for legacy_operator in [
        ["List/", "query", "("].concat(),
        ["List/", "query", "_prefix("].concat(),
    ] {
        assert!(!SERVER_SOURCE.contains(&legacy_operator));
    }

    let pipeline_start = SERVER_SOURCE
        .find("    station_matches:")
        .expect("station_matches declaration");
    let pipeline_end = SERVER_SOURCE[pipeline_start..]
        .find("    departures:")
        .map(|offset| pipeline_start + offset)
        .expect("declaration after station_matches");
    let pipeline = &SERVER_SOURCE[pipeline_start..pipeline_end];

    let filter = pipeline.find("|> List/filter(").expect("typed filter");
    let order = pipeline
        .find("|> List/sort_by(")
        .expect("stable primary order");
    let take = pipeline.find("|> List/take(").expect("bounded result");
    assert!(
        filter < order && order < take,
        "pipeline operator order changed"
    );
    assert!(pipeline.contains("left: normalized_search_query |> Text/is_not_empty()"));
    assert!(pipeline.contains(
        "item.name\n                        |> Text/trim()\n                        |> Text/to_lowercase()\n                        |> Text/starts_with(prefix: normalized_search_query)"
    ));
    assert!(pipeline.contains(
        "key:\n                item.name\n                |> Text/trim()\n                |> Text/to_lowercase()"
    ));
    assert!(pipeline.contains("direction: Ascending"));
    assert!(pipeline.contains("|> List/take(count: 20)"));
    assert!(!pipeline.contains("field:"));
    assert!(!pipeline.contains("normalization:"));
}

#[test]
fn fjordpulse_server_search_source_is_parser_valid_during_generic_operator_cutover() {
    let diagnostics = boon_compiler::diagnose_runtime_source_units(
        SERVER_PATH,
        &[
            boon_compiler::CompilerSourceUnit {
                path: SHARED_PATH.to_owned(),
                source: SHARED_SOURCE.to_owned(),
            },
            boon_compiler::CompilerSourceUnit {
                path: SERVER_PATH.to_owned(),
                source: SERVER_SOURCE.to_owned(),
            },
        ],
    );
    let parser_errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.start.is_none() && diagnostic.end.is_none())
        .collect::<Vec<_>>();
    assert!(
        parser_errors.is_empty(),
        "FjordPulse source has parser errors: {parser_errors:#?}"
    );
    let unknown_operators = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("unknown function"))
        .collect::<Vec<_>>();
    assert!(
        unknown_operators.is_empty(),
        "missing generic operators: {unknown_operators:#?}"
    );
}

#[test]
fn fjordpulse_server_search_executes_the_typed_pipeline() {
    let compiled = boon_compiler::compile_source_path_to_machine_plan_for_role(
        Path::new(SERVER_PATH),
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
    )
    .expect("deterministic FjordPulse Server should compile");
    let empty = BTreeSet::new();
    let unsupported = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| {
            !cpu_plan_executor_supports_whole_plan_op(
                &compiled.plan.row_expressions,
                &compiled.plan.storage_layout.scalar_slots,
                op,
                &empty,
            )
        })
        .map(|op| {
            (
                op.id,
                op.indexed,
                op.unresolved_executable_ref_count,
                op.kind.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        compiled.plan.capability_summary.cpu_plan_executor_complete,
        "unsupported FjordPulse Server ops: {unsupported:#?}"
    );
    assert_eq!(compiled.plan.list_indexes.len(), 1);
    let access = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            boon_plan::PlanOpKind::DerivedValue {
                expression:
                    Some(boon_plan::PlanDerivedExpression::MaterializeList { expression, .. }),
                ..
            } => match expression.as_ref() {
                boon_plan::PlanDerivedExpression::RowExpression { expression } => {
                    match compiled.plan.row_expressions.node(*expression).ok()? {
                        boon_plan::PlanRowExpressionNode::ListAccess { access } => {
                            Some(access.as_ref())
                        }
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        })
        .expect("inline bounded FjordPulse search access");
    assert!(matches!(
        access.selection,
        PlanListAccessSelection::TextPrefix { .. }
    ));
    let output_target = |name: &str| {
        compiled
            .plan
            .outputs
            .iter()
            .find(|output| output.name == name)
            .and_then(|output| match &output.value {
                OutputValueRef::RuntimeValue {
                    value: ValueRef::Field(field),
                    ..
                } => Some(ValueTarget::Field(*field)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name} must resolve to one root field"))
    };
    let search_target = output_target("search_contract");
    let response_target = output_target("http_response");
    let source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.http_request")
        .expect("HTTP request source must exist")
        .source_id;
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    let route = session.source_route_token(source, &[]).unwrap();

    let turn = session
        .apply_with_demand(
            SourceEvent {
                sequence: 1,
                source,
                route,
                target: None,
                payload: SourcePayload {
                    fields: BTreeMap::from([
                        ("method".to_owned(), Value::Text("GET".to_owned())),
                        ("path".to_owned(), Value::Text("/api/search".to_owned())),
                        (
                            "path_segments".to_owned(),
                            Value::List(vec![
                                Value::Text("api".to_owned()),
                                Value::Text("search".to_owned()),
                            ]),
                        ),
                        (
                            "query".to_owned(),
                            Value::List(vec![Value::Record(BTreeMap::from([
                                ("name".to_owned(), Value::Text("q".to_owned())),
                                ("value".to_owned(), Value::Text("ber".to_owned())),
                            ]))]),
                        ),
                    ]),
                    ..SourcePayload::default()
                },
            },
            &[search_target, response_target],
        )
        .unwrap();
    assert!(turn.metrics.access_index_seek_count >= 1);
    assert_eq!(turn.metrics.access_full_scan_count, 0);
    assert_eq!(turn.metrics.access_candidate_count, 1);

    let Value::Record(search) = session.output_value_current("search_contract").unwrap() else {
        panic!("search contract must remain structural")
    };
    assert_eq!(search["ok"], Value::Bool(true));
    assert_eq!(search["query"], Value::Text("ber".to_owned()));
    assert_eq!(search["indexedResultCount"], number(1));

    let Value::Record(response) = session.output_value_current("http_response").unwrap() else {
        panic!("HTTP response must remain structural")
    };
    assert_eq!(response["status"], number(200));
    let Value::Bytes(body) = &response["body"] else {
        panic!("HTTP response body must be Bytes")
    };
    let body = std::str::from_utf8(body).expect("JSON response body must be UTF-8");
    assert!(body.contains("NSR:StopPlace:58366"));
    assert!(body.contains("Bergen stasjon"));
}

#[test]
fn fjordpulse_scale_access_matrix_is_bounded_across_58_500_rows() {
    fn contains_selection(
        selection: &PlanListAccessSelection,
        predicate: fn(&PlanListAccessSelection) -> bool,
    ) -> bool {
        predicate(selection)
            || match selection {
                PlanListAccessSelection::Union { branches }
                | PlanListAccessSelection::Intersection { branches } => branches
                    .iter()
                    .any(|branch| contains_selection(branch, predicate)),
                _ => false,
            }
    }

    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "fjordpulse-scale-access.bn",
        include_str!("../../../testdata/fjordpulse_scale_access.bn"),
        TargetProfile::SoftwareDefault,
    )
    .expect("58,500-row typed station access matrix must compile");
    let selections = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            boon_plan::PlanOpKind::DerivedValue {
                expression:
                    Some(boon_plan::PlanDerivedExpression::MaterializeList { expression, .. }),
                ..
            } => match expression.as_ref() {
                boon_plan::PlanDerivedExpression::RowExpression { expression } => {
                    match compiled.plan.row_expressions.node(*expression).ok()? {
                        boon_plan::PlanRowExpressionNode::ListAccess { access } => {
                            Some(&access.selection)
                        }
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    for (label, predicate) in [
        (
            "exact/compound",
            (|selection| matches!(selection, PlanListAccessSelection::KeyPrefix { .. }))
                as fn(&PlanListAccessSelection) -> bool,
        ),
        (
            "prefix",
            (|selection| matches!(selection, PlanListAccessSelection::TextPrefix { .. }))
                as fn(&PlanListAccessSelection) -> bool,
        ),
        (
            "range",
            (|selection| matches!(selection, PlanListAccessSelection::ComponentRange { .. }))
                as fn(&PlanListAccessSelection) -> bool,
        ),
        (
            "union",
            (|selection| matches!(selection, PlanListAccessSelection::Union { .. }))
                as fn(&PlanListAccessSelection) -> bool,
        ),
        (
            "intersection",
            (|selection| matches!(selection, PlanListAccessSelection::Intersection { .. }))
                as fn(&PlanListAccessSelection) -> bool,
        ),
    ] {
        assert!(
            selections
                .iter()
                .any(|selection| contains_selection(selection, predicate)),
            "58,500-row plan is missing {label} access"
        );
    }

    let index_count = compiled.plan.list_indexes.len() as u64;
    assert!(
        index_count >= 5,
        "access matrix should deduplicate into several indexes"
    );
    let page_source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.page_after_input")
        .expect("page continuation source")
        .source_id;
    let access_cases = [
        (
            "store.exact",
            list_field(&compiled.plan, "store.exact", "id"),
            vec![58_499],
            1,
        ),
        (
            "store.compound_exact",
            list_field(&compiled.plan, "store.compound_exact", "id"),
            vec![58_499],
            1,
        ),
        (
            "store.prefix",
            list_field(&compiled.plan, "store.prefix", "id"),
            station_prefix_ids("station-584", false),
            20,
        ),
        (
            "store.compound_prefix",
            list_field(&compiled.plan, "store.compound_prefix", "id"),
            station_prefix_ids("station-584", true),
            20,
        ),
        (
            "store.range",
            list_field(&compiled.plan, "store.range", "id"),
            (58_440..58_460).collect(),
            20,
        ),
        (
            "store.union",
            list_field(&compiled.plan, "store.union", "id"),
            vec![1, 58_499],
            2,
        ),
        (
            "store.token_union",
            list_field(&compiled.plan, "store.token_union", "id"),
            vec![58_499, 58_498],
            2,
        ),
        (
            "store.token_intersection",
            list_field(&compiled.plan, "store.token_intersection", "id"),
            vec![58_499],
            2,
        ),
        (
            "store.spatial",
            list_field(&compiled.plan, "store.spatial", "id"),
            (58_490..=58_499).collect(),
            100,
        ),
    ];
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default())
        .expect("58,500-row typed station access matrix must initialize");
    let startup = session.startup_metrics();
    assert_eq!(startup.ordered_index_full_rebuild_count, index_count);
    assert!(startup.ordered_index_rebuild_entry_count >= 58_500_u64.saturating_mul(index_count));
    assert!(startup.ordered_index_rebuild_encoded_key_bytes > 0);
    assert!(startup.ordered_index_rebuild_structural_key_bytes > 0);
    assert_eq!(startup.ordered_index_current_count, index_count);
    assert_eq!(startup.ordered_index_resource_limit_failure_count, 0);
    assert!(startup.ordered_index_rebuild_expanded_key_count > 0);
    assert_eq!(
        startup.ordered_index_current_payload_bytes,
        startup.ordered_index_rebuild_payload_bytes
    );

    for (name, (list, id_field), expected_ids, maximum_candidates) in access_cases {
        let (value, metrics) = session
            .list_value_current_with_metrics(list)
            .unwrap_or_else(|error| panic!("{name} failed: {error}"));
        assert!(matches!(value, Value::List(_)));
        assert_eq!(
            station_ids(&session, list, id_field),
            expected_ids,
            "{name} returned wrong rows"
        );
        assert!(metrics.access_index_seek_count >= 1, "{name} did not seek");
        assert_eq!(metrics.access_full_scan_count, 0, "{name} scanned");
        assert!(
            metrics.access_candidate_count <= maximum_candidates,
            "{name} visited {} candidates, limit is {maximum_candidates}",
            metrics.access_candidate_count
        );
        assert_eq!(metrics.ordered_index_full_rebuild_count, 0);
        assert_eq!(metrics.ordered_index_resource_limit_failure_count, 0);
        if matches!(
            name,
            "store.union" | "store.token_union" | "store.token_intersection"
        ) {
            assert!(
                metrics.access_branch_poll_count >= 2,
                "{name} skipped branches"
            );
        }
        if name == "store.spatial" {
            assert!(
                metrics.access_kernel_returned_count > metrics.access_result_count,
                "spatial residual did not prune index candidates"
            );
        }
    }

    let (first_page, first_metrics) = session
        .root_value_current_with_metrics("store.page")
        .expect("first station page must execute");
    let Value::Record(first_page) = first_page else {
        panic!("station page must be structural")
    };
    assert_eq!(first_page["$tag"], Value::Text("Page".to_owned()));
    assert!(matches!(&first_page["items"], Value::List(items) if items.len() == 20));
    let next = first_page["next"].clone();
    assert!(
        matches!(&next, Value::Record(fields) if fields.get("$tag") == Some(&Value::Text("Cursor".to_owned())))
    );
    assert_eq!(first_metrics.access_index_seek_count, 1);
    assert_eq!(first_metrics.access_candidate_count, 21);
    assert_eq!(first_metrics.access_full_scan_count, 0);

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: page_source,
            route: session.source_route_token(page_source, &[]).unwrap(),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([("value".to_owned(), next)]),
                ..SourcePayload::default()
            },
        })
        .expect("page continuation source must apply");
    let (deep_page, deep_metrics) = session
        .root_value_current_with_metrics("store.page")
        .expect("deep station page must execute");
    let Value::Record(deep_page) = deep_page else {
        panic!("deep station page must be structural")
    };
    assert!(matches!(&deep_page["items"], Value::List(items) if items.len() == 20));
    assert_eq!(
        turn.metrics
            .access_cursor_seek_count
            .saturating_add(deep_metrics.access_cursor_seek_count),
        1
    );
    assert_eq!(
        turn.metrics
            .access_full_scan_count
            .saturating_add(deep_metrics.access_full_scan_count),
        0
    );
    assert!(
        turn.metrics
            .access_candidate_count
            .saturating_add(deep_metrics.access_candidate_count)
            <= 21
    );
}
