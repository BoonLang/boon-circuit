#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

use boon_plan::{ApplicationIdentity, FieldId, MachinePlan, ProgramRole, SourceId, TargetProfile};
use boon_plan_executor::{
    AuthorityDelta, MachineInstance, SessionOptions, SourceEvent, SourcePayload, Value, ValueTarget,
};
use std::collections::{BTreeMap, BTreeSet};

fn compile_test_source(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
) -> boon_compiler::CompilerResult<boon_compiler::CompiledMachinePlanFromSource> {
    boon_compiler::compile_machine_plan(boon_compiler::CompileRequest::source_text(
        source_label,
        source_text,
        target_profile,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))
}

fn source_id(plan: &MachinePlan, path: &str) -> SourceId {
    plan.source_routes
        .iter()
        .find(|route| route.path == path)
        .unwrap_or_else(|| panic!("missing SOURCE route `{path}`"))
        .source_id
}

fn field_id(plan: &MachinePlan, label: &str) -> FieldId {
    let id = &plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == label)
        .unwrap_or_else(|| panic!("missing field debug label `{label}`"))
        .id;
    FieldId(id.strip_prefix("field:").unwrap().parse().unwrap())
}

fn source_event(
    session: &MachineInstance,
    sequence: u64,
    source: SourceId,
    fields: BTreeMap<String, Value>,
) -> SourceEvent {
    SourceEvent {
        sequence,
        source,
        route: session.source_route_token(source, &[]).unwrap(),
        target: None,
        payload: SourcePayload {
            fields,
            ..SourcePayload::default()
        },
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn map_set_values_and_sparse_deltas_have_one_native_and_wasm_trace() {
    let compiled = compile_test_source(
        "map-set-cross-target.bn",
        r#"
store: [
    map_updates: SOURCE
    set_updates: SOURCE
    entry:
        [key: TEXT { middle }, value: 13]
        |> HOLD entry {
            map_updates.name |> THEN {
                [
                    key: map_updates.name
                    value:
                        map_updates.value |> Text/to_number() |> WHEN {
                            Parsed[value] => value
                            InvalidNumber[reason, position] => 0
                        }
                ]
            }
        }
    users: MAP {
        TEXT { zulu } => 26
        entry.key => entry.value
        TEXT { alpha } => 1
    }
    selected:
        users |> Map/get(key: TEXT { middle })
    role:
        Admin
        |> HOLD role {
            set_updates.role |> THEN {
                set_updates.role |> WHEN {
                    TEXT { Editor } => Editor
                    TEXT { Viewer } => Viewer
                    __ => Admin
                }
            }
        }
    roles: SET {
        Viewer
        role
        Viewer
    }
    has_editor:
        roles |> Set/contains(item: Editor)
    number_keys: MAP {
        0.5 => Half
        -1 => Negative
        0.25 => Quarter
    }
]

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { MAP and SET })
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let plan = compiled.plan;
    let map_updates = source_id(&plan, "store.map_updates");
    let set_updates = source_id(&plan, "store.set_updates");
    let users = field_id(&plan, "store.users");
    let selected = field_id(&plan, "store.selected");
    let roles = field_id(&plan, "store.roles");
    let has_editor = field_id(&plan, "store.has_editor");
    let mut session = MachineInstance::new_quiescent(plan, SessionOptions::default()).unwrap();

    assert_eq!(
        session.root_value_current("store.users").unwrap(),
        Value::Map(BTreeMap::from([
            (Value::Text("alpha".to_owned()), Value::integer(1).unwrap()),
            (
                Value::Text("middle".to_owned()),
                Value::integer(13).unwrap(),
            ),
            (Value::Text("zulu".to_owned()), Value::integer(26).unwrap()),
        ]))
    );
    assert_eq!(
        session.root_value_current("store.roles").unwrap(),
        Value::Set(BTreeSet::from([Value::tag("Admin"), Value::tag("Viewer")]))
    );
    assert_eq!(
        session.root_value_current("store.number_keys").unwrap(),
        Value::Map(BTreeMap::from([
            (Value::integer(-1).unwrap(), Value::tag("Negative")),
            (
                Value::Number("0.25".parse().unwrap()),
                Value::tag("Quarter"),
            ),
            (Value::Number("0.5".parse().unwrap()), Value::tag("Half")),
        ]))
    );

    let map_turn = session
        .apply_with_demand(
            source_event(
                &session,
                1,
                map_updates,
                BTreeMap::from([
                    ("name".to_owned(), Value::Text("bravo".to_owned())),
                    ("value".to_owned(), Value::Text("2".to_owned())),
                ]),
            ),
            &[ValueTarget::Field(users), ValueTarget::Field(selected)],
        )
        .unwrap();
    let map_deltas = map_turn
        .authority_deltas
        .iter()
        .filter(|delta| {
            matches!(
                delta,
                AuthorityDelta::MapUpsert { .. } | AuthorityDelta::MapRemove { .. }
            )
        })
        .collect::<Vec<_>>();
    assert!(
        matches!(
            map_deltas.as_slice(),
            [AuthorityDelta::MapUpsert {
                key: Value::Text(key),
                value,
                ..
            }] if key == "bravo" && *value == Value::integer(2).unwrap()
        ),
        "unexpected native/Wasm MAP deltas: {map_deltas:#?}; all deltas: {:#?}; entry: {:#?}; users: {:#?}; recomputed: {:#?}",
        map_turn.authority_deltas,
        session.root_value_current("store.entry").unwrap(),
        session.root_value_current("store.users").unwrap(),
        map_turn.metrics.recomputed_targets,
    );
    assert_eq!(
        session.root_value_current("store.selected").unwrap(),
        Value::tagged(
            "Found",
            BTreeMap::from([("value".to_owned(), Value::integer(13).unwrap())]),
        )
    );

    let set_turn = session
        .apply_with_demand(
            source_event(
                &session,
                2,
                set_updates,
                BTreeMap::from([("role".to_owned(), Value::Text("Editor".to_owned()))]),
            ),
            &[ValueTarget::Field(roles), ValueTarget::Field(has_editor)],
        )
        .unwrap();
    let set_deltas = set_turn
        .authority_deltas
        .iter()
        .filter(|delta| {
            matches!(
                delta,
                AuthorityDelta::SetAdd { .. } | AuthorityDelta::SetRemove { .. }
            )
        })
        .collect::<Vec<_>>();
    assert!(
        matches!(
            set_deltas.as_slice(),
            [AuthorityDelta::SetAdd {
                item: Value::Tag { tag, fields },
                ..
            }] if tag == "Editor" && fields.is_empty()
        ),
        "unexpected native/Wasm SET deltas: {set_deltas:#?}"
    );
    assert_eq!(
        session.root_value_current("store.roles").unwrap(),
        Value::Set(BTreeSet::from([
            Value::tag("Admin"),
            Value::tag("Editor"),
            Value::tag("Viewer"),
        ]))
    );
    assert_eq!(
        session.root_value_current("store.has_editor").unwrap(),
        Value::truth(true)
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn conflicting_map_writes_fail_and_roll_back_identically_on_native_and_wasm() {
    let compiled = compile_test_source(
        "map-conflict-cross-target.bn",
        r#"
store: [
    updates: SOURCE
    left_entry:
        [key: TEXT { A }, value: 1]
        |> HOLD entry {
            updates.name |> THEN {
                [
                    key: updates.name
                    value:
                        updates.left |> Text/to_number() |> WHEN {
                            Parsed[value] => value
                            InvalidNumber[reason, position] => 0
                        }
                ]
            }
        }
    right_entry:
        [key: TEXT { B }, value: 2]
        |> HOLD entry {
            updates.name |> THEN {
                [
                    key: updates.name
                    value:
                        updates.right |> Text/to_number() |> WHEN {
                            Parsed[value] => value
                            InvalidNumber[reason, position] => 0
                        }
                ]
            }
        }
    users: MAP {}
    left_users:
        users |> Map/upsert(entry: left_entry)
    right_users:
        users |> Map/upsert(entry: right_entry)
]

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { conflict })
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let plan = compiled.plan;
    let updates = source_id(&plan, "store.updates");
    let left_users = field_id(&plan, "store.left_users");
    let right_users = field_id(&plan, "store.right_users");
    let mut session = MachineInstance::new_quiescent(plan, SessionOptions::default()).unwrap();

    session.root_value_current("store.left_users").unwrap();
    session.root_value_current("store.right_users").unwrap();
    let before = session.authority_snapshot().unwrap();
    let result = session.apply_with_demand(
        source_event(
            &session,
            1,
            updates,
            BTreeMap::from([
                ("name".to_owned(), Value::Text("C".to_owned())),
                ("left".to_owned(), Value::Text("3".to_owned())),
                ("right".to_owned(), Value::Text("4".to_owned())),
            ]),
        ),
        &[
            ValueTarget::Field(left_users),
            ValueTarget::Field(right_users),
        ],
    );
    let error = result.unwrap_err();

    assert!(
        error.to_string().contains("conflicting MAP/SET operations"),
        "unexpected native/Wasm conflict: {error}"
    );
    assert_eq!(session.authority_snapshot().unwrap(), before);
}
