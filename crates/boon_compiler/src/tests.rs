use super::*;

#[test]
fn timer_interval_lowers_once_as_a_scheduled_source_route() {
    let compiled = compile_source_text_to_machine_plan(
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
use boon_plan::{
    DocumentExprId, DocumentExprOp, DocumentMaterializationSource, DocumentRead,
    DocumentValueClass, PLAN_MAJOR_VERSION, PlanDerivedExpression, PlanOpKind, PlanRowExpression,
    RootOutputDemand, ValueRef, plan_sha256,
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

#[test]
fn compiler_emits_machine_plan_v2_as_its_only_output() {
    let compiled = compile_source_text_to_machine_plan(
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
fn compiler_root_demand_is_sorted_and_unique() {
    let compiled = compile_source_text_to_machine_plan(
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
    let compiled = compile_source_text_to_machine_plan(
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

    assert!(matches!(
        op.kind,
        PlanOpKind::DerivedValue {
            expression: Some(PlanDerivedExpression::SourceEventTransform { .. }),
            ..
        }
    ));
}

#[test]
fn source_event_transform_uses_the_branch_owned_by_each_source() {
    let compiled = compile_source_text_to_machine_plan(
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
    let field = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.format")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|field| field.parse::<usize>().ok())
        .map(boon_plan::FieldId)
        .expect("format field");
    let op = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find(|op| op.output == Some(ValueRef::Field(field)))
        .expect("format operation");
    let PlanOpKind::DerivedValue {
        expression: Some(PlanDerivedExpression::SourceEventTransform { arms, .. }),
        ..
    } = &op.kind
    else {
        panic!("format must lower as a source event transform: {op:#?}");
    };
    let reset = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.sources.reset")
        .expect("reset source");
    let reset_arm = arms
        .iter()
        .find(|arm| arm.source_id == reset.source_id)
        .expect("reset arm");
    let PlanRowExpression::Constant { constant_id } = &reset_arm.value else {
        panic!("reset arm must remain a constant: {reset_arm:#?}");
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
    let compiled = compile_source_text_to_machine_plan(
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
    let compiled = compile_source_text_to_machine_plan(
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
    let compiled = compile_source_text_to_machine_plan(
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
    let compiled = compile_source_text_to_machine_plan(
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
fn document_row_alias_arguments_remain_rows_and_selects_follow_dynamic_inputs() {
    let compiled = compile_source_text_to_machine_plan(
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
    let error = compile_source_text_to_machine_plan(
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
