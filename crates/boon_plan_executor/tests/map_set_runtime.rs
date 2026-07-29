use boon_plan::{FieldId, MachinePlan, SourceId, TargetProfile};
use boon_plan_executor::{
    AuthorityDelta, MachineInstance, MachineInstanceBuilder, SessionOptions, SourceEvent,
    SourcePayload, Value, ValueTarget,
};
use std::collections::{BTreeMap, BTreeSet};

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

#[test]
fn structurally_identical_map_constructions_keep_distinct_authorities() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "map-authority-identity.bn",
        r#"
store: [
    left: MAP {}
    right: MAP {}
    updated_left:
        left
        |> Map/upsert(
            entry: [key: TEXT { only-left }, value: 1]
        )
]

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { identity })
)
"#,
        TargetProfile::SoftwareBounded,
    )
    .unwrap();
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    assert_eq!(
        session.root_value_current("store.updated_left").unwrap(),
        Value::Map(BTreeMap::from([(
            Value::Text("only-left".to_owned()),
            Value::integer(1).unwrap(),
        )]))
    );
    assert_eq!(
        session.root_value_current("store.right").unwrap(),
        Value::Map(BTreeMap::new())
    );
    let authority = session.authority_snapshot().unwrap();
    assert_eq!(authority.maps.len(), 2);
    assert_eq!(
        authority
            .maps
            .keys()
            .map(|address| address.authority)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
}

#[test]
fn map_authority_emits_sparse_deltas_and_wakes_only_the_observed_key() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "map-keyed-currentness.bn",
        r#"
store: [
    updates: SOURCE
    entry:
        [key: TEXT { A }, value: 1]
        |> HOLD entry {
            updates.name |> THEN {
                [
                    key: updates.name,
                    value:
                        updates.value |> Text/to_number() |> WHEN {
                            Parsed[value] => value
                            InvalidNumber[reason, position] => 0
                        }
                ]
            }
        }
    users: MAP {
        entry.key => entry.value
    }
    selected_a:
        users |> Map/get(key: TEXT { A })
    selected_b:
        users |> Map/get(key: TEXT { B })
    expected_initial: MAP {
        TEXT { A } => 1
    }
    only_initial_entry:
        users == expected_initial
]

document: Document/new(
    root: Element/label(element: [], style: [], label: TEXT { keyed MAP })
)
"#,
        TargetProfile::SoftwareBounded,
    )
    .unwrap();
    let plan = compiled.plan;
    let change = source_id(&plan, "store.updates");
    let users = field_id(&plan, "store.users");
    let selected_a = field_id(&plan, "store.selected_a");
    let selected_b = field_id(&plan, "store.selected_b");
    let only_initial_entry = field_id(&plan, "store.only_initial_entry");
    let mut session = MachineInstance::new(plan.clone(), SessionOptions::default()).unwrap();

    assert_eq!(
        session.root_value_current("store.selected_a").unwrap(),
        Value::tagged(
            "Found",
            BTreeMap::from([("value".to_owned(), Value::integer(1).unwrap())]),
        )
    );
    assert_eq!(
        session.root_value_current("store.selected_b").unwrap(),
        Value::tag("NotFound")
    );
    assert_eq!(
        session
            .root_value_current("store.only_initial_entry")
            .unwrap(),
        Value::truth(true)
    );

    let turn = session
        .apply_with_demand(
            source_event(
                &session,
                1,
                change,
                BTreeMap::from([
                    ("name".to_owned(), Value::Text("B".to_owned())),
                    ("value".to_owned(), Value::Text("2".to_owned())),
                ]),
            ),
            &[
                ValueTarget::Field(users),
                ValueTarget::Field(selected_a),
                ValueTarget::Field(selected_b),
                ValueTarget::Field(only_initial_entry),
            ],
        )
        .unwrap();
    let collection_deltas = turn
        .authority_deltas
        .iter()
        .filter(|delta| {
            matches!(
                delta,
                AuthorityDelta::MapUpsert { .. }
                    | AuthorityDelta::MapRemove { .. }
                    | AuthorityDelta::SetAdd { .. }
                    | AuthorityDelta::SetRemove { .. }
            )
        })
        .collect::<Vec<_>>();
    assert!(
        matches!(
            collection_deltas.as_slice(),
            [AuthorityDelta::MapUpsert {
                key: Value::Text(key),
                value,
                ..
            }] if key == "B" && *value == Value::integer(2).unwrap()
        ),
        "unexpected collection deltas: {collection_deltas:#?}; entry: {:#?}; users: {:#?}; recomputed: {:#?}",
        session.root_value_current("store.entry").unwrap(),
        session.root_value_current("store.users").unwrap(),
        turn.metrics.recomputed_targets
    );
    assert!(
        !turn
            .metrics
            .recomputed_targets
            .contains(&ValueTarget::Field(selected_a)),
        "an unrelated key write must not wake Map/get(A)"
    );
    assert!(
        turn.metrics
            .recomputed_targets
            .contains(&ValueTarget::Field(selected_b)),
        "Map/get(B) must wake when B is inserted"
    );
    assert!(
        turn.metrics
            .recomputed_targets
            .contains(&ValueTarget::Field(only_initial_entry)),
        "a full MAP equality observation must wake on any changed key"
    );
    assert_eq!(
        session
            .root_value_current("store.only_initial_entry")
            .unwrap(),
        Value::truth(false)
    );
    assert_eq!(
        session.root_value_current("store.users").unwrap(),
        Value::Map(BTreeMap::from([
            (Value::Text("A".to_owned()), Value::integer(1).unwrap(),),
            (Value::Text("B".to_owned()), Value::integer(2).unwrap(),),
        ]))
    );

    let authority = session.authority_snapshot().unwrap();
    assert_eq!(authority.maps.len(), 2);
    assert!(authority.maps.values().any(|authority| authority.entries
        == BTreeMap::from([
            (Value::Text("A".to_owned()), Value::integer(1).unwrap(),),
            (Value::Text("B".to_owned()), Value::integer(2).unwrap(),),
        ])));
    let mut restored = MachineInstanceBuilder::new(plan, SessionOptions::default())
        .unwrap()
        .restore(authority)
        .build()
        .unwrap();
    assert_eq!(
        restored.root_value_current("store.users").unwrap(),
        Value::Map(BTreeMap::from([
            (Value::Text("A".to_owned()), Value::integer(1).unwrap(),),
            (Value::Text("B".to_owned()), Value::integer(2).unwrap(),),
        ]))
    );
}

#[test]
fn conflicting_same_turn_map_writes_roll_back_the_complete_authority() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "map-conflict-rollback.bn",
        r#"
store: [
    updates: SOURCE
    left_entry:
        [key: TEXT { A }, value: 1]
        |> HOLD entry {
            updates.name |> THEN {
                [
                    key: updates.name,
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
                    key: updates.name,
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
        TargetProfile::SoftwareBounded,
    )
    .unwrap();
    let plan = compiled.plan;
    let change = source_id(&plan, "store.updates");
    let left_users = field_id(&plan, "store.left_users");
    let right_users = field_id(&plan, "store.right_users");
    let mut session = MachineInstance::new(plan, SessionOptions::default()).unwrap();

    session.root_value_current("store.left_users").unwrap();
    session.root_value_current("store.right_users").unwrap();
    let before = session.authority_snapshot().unwrap();
    let result = session.apply_with_demand(
        source_event(
            &session,
            1,
            change,
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
    let error = match result {
        Err(error) => error,
        Ok(turn) => panic!(
            "conflicting turn unexpectedly succeeded: {turn:?}; authority: {:#?}",
            session.authority_snapshot().unwrap()
        ),
    };
    assert!(
        error.to_string().contains("conflicting MAP/SET operations"),
        "unexpected conflict error: {error}"
    );
    assert_eq!(session.authority_snapshot().unwrap(), before);
    assert_eq!(
        session.root_value_current("store.users").unwrap(),
        Value::Map(BTreeMap::from([
            (Value::Text("A".to_owned()), Value::integer(1).unwrap(),),
            (Value::Text("B".to_owned()), Value::integer(2).unwrap(),),
        ]))
    );
}
