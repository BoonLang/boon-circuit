#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

use boon_compiler::{
    CompileRequest, CompiledMachinePlanFromSource, CompilerResult, compile_machine_plan,
};
use boon_plan::{ApplicationIdentity, ProgramRole, TargetProfile};
use boon_plan_executor::{MachineInstance, SessionOptions, SourceEvent, SourcePayload, Value};
use serde::Deserialize;
use std::collections::BTreeMap;

fn compile_test_source(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
    program_role: ProgramRole,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    compile_machine_plan(CompileRequest::source_text(
        source_label,
        source_text,
        target_profile,
        program_role,
        ApplicationIdentity::compiler_default(),
    ))
}

#[derive(Deserialize)]
struct ExpectedRoot {
    path: String,
    #[serde(default)]
    value: Option<boon_data::Value>,
    #[serde(default)]
    map_entries: Option<Vec<(boon_data::Value, boon_data::Value)>>,
}

impl ExpectedRoot {
    fn value(self) -> Value {
        match (self.value, self.map_entries) {
            (Some(value), None) => Value::from_data(&value),
            (None, Some(entries)) => Value::Map(
                entries
                    .into_iter()
                    .map(|(key, value)| (Value::from_data(&key), Value::from_data(&value)))
                    .collect::<BTreeMap<_, _>>(),
            ),
            _ => panic!("foundation expectation must contain exactly one value representation"),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn universal_foundation_values_have_one_native_and_wasm_trace() {
    let compiled = compile_test_source(
        "foundations-vertical.bn",
        include_str!("../testdata/foundations_vertical.bn"),
        TargetProfile::SoftwareBounded,
        ProgramRole::Server,
    )
    .unwrap();
    let expected: Vec<ExpectedRoot> =
        serde_json::from_str(include_str!("../testdata/foundations_vertical.json")).unwrap();
    let mut machine = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    for expected in expected {
        let path = expected.path.clone();
        assert_eq!(
            machine.root_value_current(&path).unwrap(),
            expected.value(),
            "foundation trace changed at `{}`",
            path
        );
    }
}

fn row_ids(value: Value) -> Vec<boon_plan_executor::RowId> {
    let Value::List(rows) = value else {
        panic!("vertical row fixture did not publish a list")
    };
    rows.into_iter()
        .map(|row| match row {
            Value::Row { id, .. } | Value::MappedRow { id, .. } => id,
            other => panic!("vertical row fixture published a non-row: {other:?}"),
        })
        .collect()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn typed_views_have_one_native_and_wasm_trace() {
    let compiled = compile_test_source(
        "typed-views-vertical.bn",
        include_str!("../../../testdata/phase0/fixtures/typed_views_current.bn"),
        TargetProfile::SoftwareBounded,
        ProgramRole::Server,
    )
    .unwrap();
    let mut machine = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    assert_eq!(
        machine.root_value_current("selected").unwrap(),
        Value::List(vec![Value::integer(1).unwrap(), Value::integer(3).unwrap()])
    );
    assert_eq!(
        machine.root_value_current("selected_sum").unwrap(),
        Value::integer(4).unwrap()
    );
    let Value::Tag { tag, fields } = machine.root_value_current("page").unwrap() else {
        panic!("typed page did not publish its Page value")
    };
    assert_eq!(tag, "Page");
    assert!(matches!(fields.get("items"), Some(Value::List(items)) if items.len() == 2));
    assert!(fields.contains_key("next"));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn scoped_reactive_rows_have_one_native_and_wasm_trace() {
    let compiled = compile_test_source(
        "reactive-rows-vertical.bn",
        include_str!("../testdata/reactive_rows_vertical.bn"),
        TargetProfile::SoftwareBounded,
        ProgramRole::Server,
    )
    .unwrap();
    let mut machine = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    let rows = row_ids(machine.root_value_current("store.rows").unwrap());
    let selected_row = rows[1];
    let route = machine
        .source_route_token_for_path("store.rows.controls.select", &[selected_row])
        .unwrap();
    let source = route.source;

    let turn = machine
        .apply(SourceEvent {
            sequence: 1,
            route,
            source,
            target: Some(selected_row),
            payload: SourcePayload {
                address: Some("row://two".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();

    assert!(!turn.deltas.is_empty());
    assert_eq!(
        machine.root_value_current("store.selected").unwrap(),
        Value::Text("two".to_owned())
    );

    assert_eq!(
        machine.root_value_current("store.primary_names").unwrap(),
        Value::List(vec![Value::Text("one".to_owned())])
    );
}
