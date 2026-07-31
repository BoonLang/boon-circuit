#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

use boon_compiler::compile_source_text_to_machine_plan_for_role;
use boon_plan::{ProgramRole, TargetProfile};
use boon_plan_executor::{MachineInstance, SessionOptions, Value};
use serde::Deserialize;
use std::collections::BTreeMap;

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
    let compiled = compile_source_text_to_machine_plan_for_role(
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
