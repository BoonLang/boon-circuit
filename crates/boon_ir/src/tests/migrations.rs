// Included by `../tests.rs`; kept in the parent test module for private IR helper access.

fn lower_migration_fixture(source: &str) -> Result<TypedProgram, String> {
    let parsed = boon_parser::parse_source("migration-fixture.bn", source).unwrap();
    lower(&parsed)
}

fn assert_markers_are_metadata_only(program: &TypedProgram) {
    let marker_ids = program
        .expressions
        .iter()
        .filter(|expr| {
            matches!(
                expr.kind,
                AstExprKind::Drain { .. } | AstExprKind::Draining { .. }
            )
        })
        .map(|expr| ExprId(expr.id))
        .collect::<BTreeSet<_>>();
    assert!(program.nodes.iter().all(|node| {
        node.expr_id
            .is_none_or(|expr_id| !marker_ids.contains(&expr_id))
    }));
}

fn assert_migration_error(name: &str, source: &str, expected: &str) {
    let error = lower_migration_fixture(source).expect_err("migration fixture must be rejected");
    assert!(
        error.contains(expected),
        "invalid migration case {name:?}: expected error containing {expected:?}, got {error:?}"
    );
}

#[test]
fn semantic_migration_lowers_valid_scalar_rename() {
    let program = lower_migration_fixture(
        r#"
old_count: 0 |> HOLD count { LATEST {} } |> DRAINING
click_count: DRAIN { old_count } |> HOLD count { LATEST {} }
"#,
    )
    .unwrap();

    let old = program
        .semantic_memory
        .iter()
        .find(|memory| memory.identity.semantic_path == "old_count")
        .unwrap();
    let new = program
        .semantic_memory
        .iter()
        .find(|memory| memory.identity.semantic_path == "click_count")
        .unwrap();
    assert!(matches!(old.status, SemanticMemoryStatus::Draining { .. }));
    assert_eq!(old.identity.kind, SemanticMemoryKind::RootScalar);
    assert_eq!(old.data_type, SemanticDataType::Number);
    assert_eq!(program.migration_edges.len(), 1);
    let edge = &program.migration_edges[0];
    assert_eq!(edge.destination.memory_id, new.id);
    assert_eq!(edge.destination.semantic_path, "click_count");
    assert_eq!(edge.transfer_kind, MigrationTransferKind::Scalar);
    assert_eq!(edge.transform, MigrationTransform::Identity);
    assert_eq!(edge.source_leaves.len(), 1);
    assert_eq!(edge.source_leaves[0].memory_id, old.id);
    assert_markers_are_metadata_only(&program);
}

#[test]
fn semantic_migration_lowers_pure_conversion_root() {
    let program = lower_migration_fixture(
        r#"
old_count: 0 |> HOLD count { LATEST {} } |> DRAINING
count_text:
    DRAIN { old_count }
    |> Number/to_text()
    |> HOLD text { LATEST {} }
"#,
    )
    .unwrap();

    let edge = program.migration_edges.first().unwrap();
    assert_eq!(edge.destination.data_type, SemanticDataType::Text);
    let MigrationTransform::PureExpression {
        expression_root,
        pipeline,
    } = &edge.transform
    else {
        panic!("conversion must retain a pure transform root: {edge:?}");
    };
    assert!(!pipeline.is_empty());
    assert!(matches!(
        program.expressions[expression_root.as_usize()].kind,
        AstExprKind::Pipe { ref op, .. } if op == "Number/to_text"
    ));
    assert_markers_are_metadata_only(&program);
}

#[test]
fn semantic_migration_allows_pure_when_selection() {
    let program = lower_migration_fixture(
        r#"
completed: False |> HOLD completed { LATEST {} } |> DRAINING
status:
    DRAIN { completed }
    |> WHEN {
        True => Done
        False => Open
    }
    |> HOLD status { LATEST {} }
"#,
    )
    .unwrap();

    let edge = program.migration_edges.first().unwrap();
    assert!(matches!(
        edge.transform,
        MigrationTransform::PureExpression { .. }
    ));
    assert_eq!(
        edge.destination.data_type,
        SemanticDataType::Variant {
            variants: vec![
                SemanticVariantType {
                    tag: "Done".to_owned(),
                    fields: Vec::new(),
                    open: false,
                },
                SemanticVariantType {
                    tag: "Open".to_owned(),
                    fields: Vec::new(),
                    open: false,
                },
            ],
        }
    );
    assert_markers_are_metadata_only(&program);
}

#[test]
fn semantic_migration_lowers_whole_list_transfer() {
    let source = r#"
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
    let program = lower_migration_fixture(source).unwrap();

    let edge = program.migration_edges.first().unwrap();
    assert_eq!(edge.transfer_kind, MigrationTransferKind::List);
    assert_eq!(edge.source_leaves.len(), 1);
    assert_eq!(edge.transform, MigrationTransform::Identity);
    assert!(matches!(
        program.semantic_memory[edge.source_leaves[0].memory_id.as_usize()]
            .identity
            .kind,
        SemanticMemoryKind::ListOwner
    ));
    assert_markers_are_metadata_only(&program);
}

#[test]
fn semantic_migration_lowers_indexed_row_field_rename() {
    let program = lower_migration_fixture(
        r#"
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
"#,
    )
    .unwrap();

    let edge = program.migration_edges.first().unwrap();
    assert_eq!(edge.transfer_kind, MigrationTransferKind::IndexedField);
    let source = &program.semantic_memory[edge.source_leaves[0].memory_id.as_usize()];
    let destination = &program.semantic_memory[edge.destination.memory_id.as_usize()];
    assert_eq!(source.identity.owner_path, destination.identity.owner_path);
    assert_eq!(source.identity.kind, SemanticMemoryKind::IndexedField);
    assert_eq!(destination.identity.kind, SemanticMemoryKind::IndexedField);
    assert_eq!(source.data_type, SemanticDataType::Text);
    assert_markers_are_metadata_only(&program);
}

#[test]
fn semantic_migration_rejects_invalid_graphs_and_effects() {
    let cases = [
        (
            "missing destination pair",
            r#"
old: 0 |> HOLD value { LATEST {} } |> DRAINING
"#,
            "has no DRAIN destination (missing pair)",
        ),
        (
            "non-authority source",
            r#"
derived: 1
old: 0 |> HOLD value { LATEST {} } |> DRAINING
new: DRAIN { derived } |> HOLD value { LATEST {} }
"#,
            "does not resolve to semantic authority",
        ),
        (
            "ordinary draining read",
            r#"
old: 0 |> HOLD value { LATEST {} } |> DRAINING
copy: old |> Number/add(right: 1)
new: DRAIN { old } |> HOLD value { LATEST {} }
"#,
            "ordinary reference to DRAINING authority",
        ),
        (
            "self drain",
            r#"
old: DRAIN { old } |> HOLD value { LATEST {} } |> DRAINING
"#,
            "self drain is not allowed",
        ),
        (
            "double drain",
            r#"
old: 0 |> HOLD value { LATEST {} } |> DRAINING
first: DRAIN { old } |> HOLD value { LATEST {} }
second: DRAIN { old } |> HOLD value { LATEST {} }
"#,
            "drained more than once",
        ),
        (
            "partial source coverage",
            r#"
old: [left: 1, right: 2] |> HOLD value { LATEST {} } |> DRAINING
new_left: DRAIN { old.left } |> HOLD value { LATEST {} }
"#,
            "partial coverage",
        ),
        (
            "ancestor descendant overlap",
            r#"
old: [left: 1, right: 2] |> HOLD value { LATEST {} } |> DRAINING
new: [all: DRAIN { old }, left: DRAIN { old.left }] |> HOLD value { LATEST {} }
"#,
            "overlapping ancestor/descendant drains",
        ),
        (
            "migration cycle",
            r#"
left: DRAIN { right } |> Number/add(right: 0) |> HOLD value { LATEST {} } |> DRAINING
right: DRAIN { left } |> Number/add(right: 0) |> HOLD value { LATEST {} } |> DRAINING
"#,
            "migration graph cycle",
        ),
        (
            "conflicting destination authority",
            r#"
old: 0 |> HOLD value { LATEST {} } |> DRAINING
new:
    1 |> HOLD value { LATEST {} }
    DRAIN { old }
"#,
            "conflicts with existing destination authority",
        ),
        (
            "mutable source read in transform",
            r#"
old: 0 |> HOLD value { LATEST {} } |> DRAINING
other: 1 |> HOLD value { LATEST {} }
new:
    DRAIN { old }
    |> Number/add(right: other)
    |> HOLD value { LATEST {} }
"#,
            "outside its DRAIN inputs",
        ),
        (
            "transitive timer effect",
            r#"
FUNCTION impure(value) {
    value |> Number/add(right: Duration[seconds: 1] |> Timer/interval())
}

old: 0 |> HOLD value { LATEST {} } |> DRAINING
new: impure(value: DRAIN { old }) |> Number/add(right: 0) |> HOLD value { LATEST {} }
"#,
            "Timer/interval",
        ),
        (
            "incompatible list owner",
            r#"
FUNCTION old_row(row) {
    [value: TEXT { old }]
}

FUNCTION new_row(row) {
    [value: 1]
}

old_items:
    LIST { [value: TEXT { old }] }
    |> List/map(row, new: old_row(row: row))
    |> DRAINING

new_items:
    DRAIN { old_items }
    |> List/map(row, new: new_row(row: row))
"#,
            "incompatible list owner change",
        ),
    ];

    for (name, source, expected) in cases {
        assert_migration_error(name, source, expected);
    }
}

#[test]
fn whole_list_drain_stops_before_runtime_row_reconstruction() {
    let program =
        lower_migration_fixture(include_str!("../../../../examples/migrations/todo/v2.bn"))
            .unwrap();
    assert!(program.migration_edges.iter().any(|edge| {
        edge.transfer_kind == MigrationTransferKind::List
            && edge.transform == MigrationTransform::Identity
    }));
}

#[test]
fn identity_drain_can_widen_into_the_destination_variant_set() {
    let program =
        lower_migration_fixture(include_str!("../../../../examples/migrations/todo/v4.bn"))
            .unwrap();
    assert_eq!(program.migration_edges.len(), 2);
    assert!(
        program
            .migration_edges
            .iter()
            .all(|edge| edge.transform == MigrationTransform::Identity)
    );
}

#[test]
fn multiline_hold_keeps_the_preceding_value_as_its_default() {
    let program = lower_migration_fixture(
        r#"
old_theme:
    Light
    |> HOLD theme { LATEST {} }
    |> DRAINING

theme: DRAIN { old_theme } |> HOLD theme { LATEST {} }
"#,
    )
    .unwrap();
    let old_theme = program
        .state_cells
        .iter()
        .find(|state| state.path == "old_theme")
        .unwrap();
    assert_eq!(
        old_theme.initial_value,
        InitialValue::Enum {
            value: "Light".to_owned()
        }
    );
}
