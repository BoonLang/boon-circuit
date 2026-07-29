use boon_plan::TargetProfile;
use boon_plan_executor::{MachineInstance, SessionOptions, Value};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn map_and_set_literals_and_operations_execute_canonically() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "map-set-runtime.bn",
        r#"
store: [
    users: MAP {
        TEXT { alice } => [score: 1]
    }
    updated_users:
        users
        |> Map/upsert(entry: [key: TEXT { bob }, value: [score: 2]])
    selected_user:
        updated_users
        |> Map/get(key: TEXT { bob })
    roles: SET {
        Admin
        Editor
    }
    updated_roles:
        roles
        |> Set/remove(item: Admin)
    has_editor:
        updated_roles
        |> Set/contains(item: Editor)
]

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { MAP and SET })
)
"#,
        TargetProfile::SoftwareBounded,
    )
    .unwrap();
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    assert_eq!(
        session.root_value_current("store.updated_users").unwrap(),
        Value::Map(BTreeMap::from([
            (
                Value::Text("alice".to_owned()),
                Value::Record(BTreeMap::from([(
                    "score".to_owned(),
                    Value::integer(1).unwrap(),
                )])),
            ),
            (
                Value::Text("bob".to_owned()),
                Value::Record(BTreeMap::from([(
                    "score".to_owned(),
                    Value::integer(2).unwrap(),
                )])),
            ),
        ]))
    );
    assert_eq!(
        session.root_value_current("store.selected_user").unwrap(),
        Value::tagged(
            "Found",
            BTreeMap::from([(
                "value".to_owned(),
                Value::Record(BTreeMap::from([(
                    "score".to_owned(),
                    Value::integer(2).unwrap(),
                )])),
            )]),
        )
    );
    assert_eq!(
        session.root_value_current("store.updated_roles").unwrap(),
        Value::Set(BTreeSet::from([Value::tag("Editor")]))
    );
    assert_eq!(
        session.root_value_current("store.has_editor").unwrap(),
        Value::truth(true)
    );
}
