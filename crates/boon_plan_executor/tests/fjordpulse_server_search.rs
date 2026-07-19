use boon_plan::{
    FieldId, OutputValueRef, ProgramRole, TargetProfile, ValueRef,
    cpu_plan_executor_supports_whole_plan_op,
};
use boon_plan_executor::{
    MachineInstance, SessionOptions, SourceEvent, SourcePayload, Value, ValueTarget,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn number(value: i64) -> Value {
    Value::integer(value).unwrap()
}

#[test]
fn fjordpulse_server_search_uses_the_compiler_owned_prefix_index() {
    let compiled = boon_compiler::compile_source_path_to_machine_plan_for_role(
        Path::new("examples/fjordpulse/Server/RUN.bn"),
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
        "unsupported FjordPulse Server ops: {unsupported:#?}"
    );
    assert_eq!(compiled.plan.query_indexes.len(), 1);
    let station_matches_target = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.station_matches")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .map(ValueTarget::Field)
        .expect("station match projection must resolve to one root field");
    let search_target = compiled
        .plan
        .outputs
        .iter()
        .find(|output| output.name == "search_contract")
        .and_then(|output| match &output.value {
            OutputValueRef::RuntimeValue {
                value: ValueRef::Field(field),
            } => Some(ValueTarget::Field(*field)),
            _ => None,
        })
        .expect("search contract must resolve to one root field");
    let source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.http_request")
        .expect("HTTP request source must exist")
        .source_id;
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let turn = session
        .apply_with_demand(
            SourceEvent {
                sequence: 1,
                source,
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
            &[station_matches_target, search_target],
        )
        .unwrap();

    assert_eq!(turn.metrics.query_full_scan_count, 0);
    assert_eq!(turn.metrics.query_index_range_count, 1);
    assert!(turn.metrics.query_index_key_count <= 2);
    assert_eq!(turn.metrics.query_rows_examined_count, 1);
    assert_eq!(turn.metrics.query_result_count, 1);

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
