#[test]
fn checked_program_binds_fresh_and_forwarded_outputs() {
    let parsed = boon_parser::parse_source(
        "checked-out.bn",
        r#"
FUNCTION doubled(list, entry: OUT, new) {
    list
    |> List/map(
        item: entry
        new: new
    )
}

result:
    LIST { [value: 2] }
    |> doubled(
        entry
        new: entry.value * 2
    )
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("valid source has a checked program");

    let wrapper_call = program
        .calls
        .iter()
        .find(|call| call.function == "doubled")
        .expect("wrapper call is checked");
    assert!(
        wrapper_call.entries.iter().any(
            |entry| matches!(entry, CheckedCallEntry::FreshOut { name, .. } if name == "entry")
        )
    );

    let map_call = program
        .calls
        .iter()
        .find(|call| call.function == "List/map")
        .expect("inner map call is checked");
    assert_eq!(
        wrapper_signature_result_expression(&program, "doubled"),
        Some(map_call.expression),
        "a multiline wrapper must expose its canonical terminal call"
    );
    assert!(map_call.entries.iter().any(|entry| {
        matches!(
            entry,
            CheckedCallEntry::ForwardOut {
                name,
                target_name,
                ..
            } if name == "item" && target_name == "entry"
        )
    }));

    let wrapper_signature = program
        .callables
        .iter()
        .find(|signature| signature.name == "doubled")
        .expect("wrapper signature is checked");
    let wrapper_output = wrapper_signature
        .parameters
        .iter()
        .find(|parameter| parameter.name == "entry")
        .expect("wrapper output exists");
    let wrapper_new = wrapper_signature
        .parameters
        .iter()
        .find(|parameter| parameter.name == "new")
        .expect("wrapper new input exists");
    assert_eq!(
        wrapper_new.evaluation_scope,
        CheckedEvaluationScope::Output {
            formal: wrapper_output.decl_id
        }
    );
    assert!(wrapper_call.entries.iter().any(|entry| {
        matches!(
            entry,
            CheckedCallEntry::Input {
                name,
                evaluation_scope: CheckedEvaluationScope::Output { formal },
                ..
            } if name == "new" && *formal == wrapper_output.decl_id
        )
    }));
}

fn wrapper_signature_result_expression(
    program: &CheckedProgram,
    name: &str,
) -> Option<CheckedExprId> {
    program
        .callables
        .iter()
        .find(|signature| signature.name == name)
        .and_then(|signature| signature.result_expression)
}

fn checked_callable<'a>(program: &'a CheckedProgram, name: &str) -> &'a CheckedCallableSignature {
    program
        .callables
        .iter()
        .find(|signature| signature.name == name)
        .unwrap_or_else(|| panic!("missing checked callable `{name}`"))
}

#[test]
fn checked_callable_effects_include_nested_record_sources_and_state() {
    let parsed = boon_parser::parse_source(
        "nested-record-effects.bn",
        r#"
FUNCTION row(initial) {
    [
        trigger: SOURCE
        value:
            initial
            |> HOLD value {
                LATEST { PASSED }
            }
    ]
}

rows:
    List/range(from: 0, to: 1)
    |> List/map(item, new: row(initial: item))
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let checked = output.program.expect("checked program");
    let effect = checked_callable(&checked, "row").effect;
    assert!(effect.emits_source, "nested SOURCE must affect the callable");
    assert!(effect.writes_state, "nested HOLD must affect the callable");
}

fn assert_callable_parameters(program: &CheckedProgram, name: &str, expected: &[&str]) {
    let actual = checked_callable(program, name)
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "wrong canonical parameters for `{name}`");
}

fn assert_no_unbound_calls(parsed: &ParsedProgram, program: &CheckedProgram) {
    for expression in &parsed.expressions {
        let Some(function) = ast_callable_name(expression) else {
            continue;
        };
        let checked = program
            .expressions
            .iter()
            .find(|checked| checked.id == CheckedExprId(expression.id as u32))
            .unwrap_or_else(|| panic!("missing checked expression for `{function}`"));
        let call_id = match checked.kind {
            CheckedExpressionKind::Call { call } => call,
            ref kind => {
                panic!("AST call `{function}` lowered to {kind:?} instead of a checked call")
            }
        };
        let call = program
            .calls
            .iter()
            .find(|call| call.id == call_id)
            .unwrap_or_else(|| panic!("missing checked call {call_id:?} for `{function}`"));
        assert_eq!(call.function, function);
    }
    assert!(program.expressions.iter().all(|expression| {
        !matches!(
            &expression.kind,
            CheckedExpressionKind::Invalid { tokens }
                if tokens.iter().any(|token| token == "unbound_call")
        )
    }));
}

fn found_payload_type(ty: &Type) -> Option<&Type> {
    let Type::VariantSet(variants) = ty else {
        return None;
    };
    variants.iter().find_map(|variant| match variant {
        Variant::Tagged { tag, fields } if tag == "Found" => fields.fields.get("value"),
        _ => None,
    })
}

fn fresh_output_type<'a>(program: &'a CheckedProgram, call: &CheckedCall) -> &'a Type {
    let output = call
        .entries
        .iter()
        .find_map(|entry| match entry {
            CheckedCallEntry::FreshOut { output, .. } => Some(*output),
            _ => None,
        })
        .expect("contextual call has a fresh output");
    &program
        .declarations
        .iter()
        .find(|declaration| declaration.id == output)
        .expect("fresh output declaration exists")
        .flow_type
        .ty
}

fn assert_number_row(ty: &Type) {
    let Type::Object(shape) = ty else {
        panic!("expected a row object, found {ty:?}");
    };
    assert_eq!(shape.fields.get("value"), Some(&Type::Number));
}

#[test]
fn checked_host_effect_uses_the_authoritative_schema() {
    let parsed = boon_parser::parse_source(
        "checked-host-effect.bn",
        r#"
store: [
    read: SOURCE
    selected: PackageAsset[url: TEXT { asset://files/primary.bin }]
    result:
        NotStarted |> HOLD result {
            read |> THEN {
                File/read_stream(
                    file: selected
                    retain_content: False
                )
            }
        }
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("valid host effect is checked");

    assert_callable_parameters(
        &program,
        "File/read_stream",
        &["file", "chunk_bytes", "retain_content"],
    );
    assert!(
        checked_callable(&program, "File/read_stream")
            .effect
            .invokes_host
    );
    assert_no_unbound_calls(&parsed, &program);
}

#[test]
fn checked_render_constructors_use_exact_canonical_parameters() {
    let parsed = boon_parser::parse_source(
        "checked-render-callables.bn",
        r#"
document: Document/new(
    root: Element/label(
        element: []
        label: TEXT { status }
    )
)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output
        .program
        .expect("valid render constructors are checked");

    assert_callable_parameters(&program, "Document/new", &["root"]);
    assert_callable_parameters(
        &program,
        "Element/label",
        &["element", "style", "label", "visible", "target"],
    );
    assert_no_unbound_calls(&parsed, &program);
}

#[test]
fn checked_scalar_list_and_bytes_builtins_have_bound_calls() {
    let parsed = boon_parser::parse_source(
        "checked-ordinary-builtins.bn",
        r#"
trimmed: TEXT { value } |> Text/trim()
sum: Number/add(left: 1, right: 2)
numbers: List/range(from: 0, to: 3)
count: numbers |> List/count()
encoded: TEXT { 00ff } |> Bytes/from_hex()
first: encoded |> Bytes/get(index: 0)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("valid ordinary builtins are checked");

    for (name, parameters) in [
        ("Text/trim", &["input"][..]),
        ("Number/add", &["left", "right"][..]),
        ("List/range", &["from", "to"][..]),
        ("List/count", &["list"][..]),
        ("Bytes/from_hex", &["input"][..]),
        ("Bytes/get", &["input", "index"][..]),
    ] {
        assert_callable_parameters(&program, name, parameters);
    }
    assert_no_unbound_calls(&parsed, &program);
}

#[test]
fn checked_field_projection_from_a_record_helper_is_bound() {
    let parsed = boon_parser::parse_source(
        "checked-field-projection.bn",
        r#"
FUNCTION detail(row) {
    [
        label: row.name
    ]
}

row: [name: TEXT { ready }]
label: detail(row: row).label
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output
        .program
        .expect("record helper field projection is checked");

    assert_callable_parameters(&program, "Field/label", &["input"]);
    assert!(matches!(
        checked_callable(&program, "Field/label").result.ty,
        Type::Text
    ));
    assert_no_unbound_calls(&parsed, &program);
}

#[test]
fn checked_program_keeps_unimplemented_callable_names_fail_closed() {
    for function in [
        "List/move_field_first",
        "List/move_field_last",
        "Widget/table",
        "Widget/selected",
        "Widget/rows",
    ] {
        let parsed = boon_parser::parse_source(
            "checked-missing-schema.bn",
            &format!("value: {function}()\n"),
        )
        .unwrap();
        let output = check_program(&parsed);

        assert!(output.program.is_none(), "`{function}` must stay rejected");
        assert!(output.report.diagnostics.iter().any(|diagnostic| {
            diagnostic.message
                == format!(
                    "`{function}` has no authoritative canonical argument schema for CheckedProgram lowering"
                )
        }));
    }
}

#[test]
fn checked_runtime_builtins_use_exact_authoritative_schemas() {
    let parsed = boon_parser::parse_source(
        "checked-runtime-contracts.bn",
        r#"
route: Router/route()
navigation: Router/go_to(route: TEXT { /ready })
catalog: LIST {
    [id: TEXT { 1 }, name: TEXT { Alpha }, remove: False]
}
remaining:
    catalog
    |> List/remove(
        item
        when: item.remove
    )
page:
    catalog
    |> List/query(
        fields: TEXT { name }
        normalization: TEXT { TrimLowercase }
        select: Prefix
        prefix: TEXT { al }
        limit: 20
        unique: False
        order: Ascending
        residual: None
    )
directional: Light/directional(
    azimuth: 90
    altitude: 60
    spread: 0
    intensity: 1.5
    color: [lightness: 1]
)
ambient: Light/ambient(
    intensity: 0.3
    color: [lightness: 0.7]
)
spot: Light/spot(
    target: FocusedElement
    color: [lightness: 0.9]
    intensity: 0.5
    radius: 40
    softness: 0.1
)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("runtime contracts are checked");

    assert_callable_parameters(&program, "Router/route", &[]);
    assert_callable_parameters(&program, "Router/go_to", &["route"]);
    assert_callable_parameters(&program, "List/remove", &["list", "item", "when"]);
    assert_callable_parameters(
        &program,
        "List/query",
        &[
            "list",
            "fields",
            "normalization",
            "multi_value",
            "select",
            "key",
            "leading",
            "prefix",
            "lower",
            "upper",
            "lower_inclusive",
            "upper_inclusive",
            "keys",
            "limit",
            "unique",
            "order",
            "residual",
            "residual_field",
            "residual_value",
            "needle",
            "minimum",
            "maximum",
            "latitude_field",
            "longitude_field",
            "center_latitude",
            "center_longitude",
            "radius_meters",
            "cursor",
        ],
    );
    assert_callable_parameters(
        &program,
        "Light/directional",
        &["azimuth", "altitude", "spread", "intensity", "color"],
    );
    assert_callable_parameters(&program, "Light/ambient", &["intensity", "color"]);
    assert_callable_parameters(
        &program,
        "Light/spot",
        &["target", "color", "intensity", "radius", "softness"],
    );

    let remove = checked_callable(&program, "List/remove");
    let item = remove
        .parameters
        .iter()
        .find(|parameter| parameter.name == "item")
        .expect("remove item output");
    let when = remove
        .parameters
        .iter()
        .find(|parameter| parameter.name == "when")
        .expect("remove predicate");
    assert_eq!(item.kind, CheckedParameterKind::Out);
    assert_eq!(
        when.evaluation_scope,
        CheckedEvaluationScope::Output {
            formal: item.decl_id
        }
    );
    assert!(matches!(
        checked_callable(&program, "List/query").result.ty,
        Type::Object(ObjectShape { open: false, .. })
    ));
    assert_no_unbound_calls(&parsed, &program);
}

#[test]
fn checked_specialized_render_constructors_use_real_host_contracts() {
    let parsed = boon_parser::parse_source(
        "checked-specialized-render-contracts.bn",
        r#"
scene: Scene/new(
    root: Scene/Element/stripe(
        element: []
        direction: Column
        items: LIST {
            Scene/Element/program(
                element: []
                style: [width: Fill, height: Fill]
                source: TEXT { scene: Scene/new(root: NoElement) }
                revision: 1
                capability_profile: PublicClient
            )
            Scene/Element/embedded_media(
                element: []
                style: [media_kind: TEXT { video }]
                title: TEXT { Demo }
                to: TEXT { https://example.invalid/demo }
                child: Scene/Element/text(
                    element: []
                    text: TEXT { Poster }
                )
            )
            Scene/Element/map(
                element: []
                style: [width: Fill, height: Fill]
                generation: 1
                camera: [longitude: 0, latitude: 0, zoom: 2, bearing: 0]
                bounds: [width: 640, height: 480, scale: 1]
                tile_source: [
                    id: TEXT { fixture }
                    url_template_capability: TEXT { fixture_xyz }
                    min_zoom: 0
                    max_zoom: 6
                    tile_size: 256
                    attribution: TEXT { Fixture }
                    allowed_origins: LIST { TEXT { boon-local://fixture } }
                ]
                interaction: [pan: True, wheel_zoom: True, pinch_zoom: True, keyboard_zoom: True]
                overlays: LIST {}
            )
        }
    )
)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output
        .program
        .expect("specialized render calls are checked");

    let program_parameters = [
        "element",
        "style",
        "source",
        "support_sources",
        "artifact_id",
        "revision",
        "artifact_retention",
        "bootstrap_source",
        "bootstrap_artifact_id",
        "bootstrap_revision",
        "capability_profile",
        "session_key",
        "mount",
    ];
    assert_callable_parameters(&program, "Element/program", &program_parameters);
    assert_callable_parameters(&program, "Scene/Element/program", &program_parameters);
    assert_callable_parameters(
        &program,
        "Scene/Element/embedded_media",
        &["element", "style", "title", "to", "child"],
    );
    assert_callable_parameters(
        &program,
        "Scene/Element/map",
        &[
            "element",
            "style",
            "generation",
            "camera",
            "bounds",
            "tile_source",
            "interaction",
            "overlays",
            "items",
        ],
    );
    assert_no_unbound_calls(&parsed, &program);
}

#[test]
fn authoritative_runtime_and_render_schemas_reject_wrong_arguments() {
    for (source, expected) in [
        (
            "value: Router/go_to(path: TEXT { /wrong })\n",
            "must be `route`, found `path`",
        ),
        (
            r#"
items: LIST { [remove: False] }
value: items |> List/remove(item, on: item.remove)
"#,
            "must be `when`, found `on`",
        ),
        (
            r#"
items: LIST { [name: TEXT { Alpha }] }
value:
    items
    |> List/query(
        fields: TEXT { name }
        select: Prefix
        normalization: TEXT { TrimLowercase }
        prefix: TEXT { al }
        limit: 20
        order: Ascending
        residual: None
    )
"#,
            "must be `normalization`, found `select`",
        ),
        (
            "value: Light/ambient(intensity: TEXT { high }, color: [])\n",
            "`Light/ambient` argument `intensity` has incompatible type",
        ),
        (
            r#"
value: Scene/Element/program(
    element: []
    source: TEXT { scene: Scene/new(root: NoElement) }
    revision: 1
    profile: PublicClient
)
"#,
            "must be `capability_profile`, found `profile`",
        ),
        (
            r#"
value: Scene/Element/map(
    element: []
    camera: [longitude: 0, latitude: 0, zoom: 2, bearing: 0]
    bounds: [width: 640, height: 480, scale: 1]
    interaction: [pan: True]
    overlays: LIST {}
)
"#,
            "must be `tile_source`, found `interaction`",
        ),
    ] {
        let parsed = boon_parser::parse_source("checked-wrong-contract.bn", source).unwrap();
        let output = check_program(&parsed);
        assert!(
            output.program.is_none(),
            "invalid source unexpectedly produced a CheckedProgram: {source}"
        );
        assert!(
            output
                .report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "missing `{expected}` in diagnostics: {:#?}",
            output.report.diagnostics
        );
    }
}

#[test]
fn structural_builtin_metadata_references_exact_formals() {
    let parsed = boon_parser::parse_source(
        "checked-contextual-builtins.bn",
        r#"
FUNCTION identity(value) {
    value
}

result: identity(value: 1)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("valid source has a checked program");

    for (name, parameter_names) in [
        ("List/map", ["list", "item", "new"]),
        ("List/filter", ["list", "item", "if"]),
        ("List/retain", ["list", "item", "if"]),
        ("List/every", ["list", "item", "if"]),
        ("List/any", ["list", "item", "if"]),
        ("List/find", ["list", "item", "if"]),
    ] {
        let signature = program
            .callables
            .iter()
            .find(|signature| signature.name == name)
            .unwrap_or_else(|| panic!("missing {name} signature"));
        assert_eq!(signature.kind, CheckedCallableKind::Builtin);
        assert_eq!(
            signature
                .parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            parameter_names
        );
        let expected = (
            signature.parameters[0].decl_id,
            signature.parameters[1].decl_id,
            signature.parameters[2].decl_id,
        );
        let actual = match (name, signature.contextual_operation) {
            ("List/map", Some(CheckedContextualOperation::Map { list, row, body })) => {
                (list, row, body)
            }
            (
                "List/filter",
                Some(CheckedContextualOperation::Filter {
                    list,
                    row,
                    predicate,
                }),
            )
            | (
                "List/retain",
                Some(CheckedContextualOperation::Retain {
                    list,
                    row,
                    predicate,
                }),
            )
            | (
                "List/every",
                Some(CheckedContextualOperation::Every {
                    list,
                    row,
                    predicate,
                }),
            )
            | (
                "List/any",
                Some(CheckedContextualOperation::Any {
                    list,
                    row,
                    predicate,
                }),
            )
            | (
                "List/find",
                Some(CheckedContextualOperation::Find {
                    list,
                    row,
                    predicate,
                }),
            ) => (list, row, predicate),
            (_, operation) => panic!("wrong contextual metadata for {name}: {operation:?}"),
        };
        assert_eq!(actual, expected, "{name} must reference its own formals");

        let item = Type::Var(CONTEXTUAL_ITEM_VAR);
        assert_eq!(
            signature.parameters[0].flow_type.ty,
            Type::List(Box::new(item.clone())),
            "{name} must quantify its list item per call"
        );
        assert_eq!(signature.parameters[1].flow_type.ty, item);
        match name {
            "List/map" => {
                assert_eq!(
                    signature.parameters[2].flow_type.ty,
                    Type::Var(CONTEXTUAL_RESULT_VAR)
                );
                assert_eq!(
                    signature.result.ty,
                    Type::List(Box::new(Type::Var(CONTEXTUAL_RESULT_VAR)))
                );
            }
            "List/filter" | "List/retain" => {
                assert_eq!(signature.parameters[2].flow_type.ty, true_false_type());
                assert_eq!(
                    signature.result.ty,
                    Type::List(Box::new(Type::Var(CONTEXTUAL_ITEM_VAR)))
                );
            }
            "List/every" | "List/any" => {
                assert_eq!(signature.parameters[2].flow_type.ty, true_false_type());
                assert_eq!(signature.result.ty, true_false_type());
            }
            "List/find" => {
                assert_eq!(signature.parameters[2].flow_type.ty, true_false_type());
                assert_eq!(
                    found_payload_type(&signature.result.ty),
                    Some(&Type::Var(CONTEXTUAL_ITEM_VAR))
                );
            }
            _ => unreachable!(),
        }
    }

    let chunk = program
        .callables
        .iter()
        .find(|signature| signature.name == "List/chunk")
        .expect("List/chunk signature");
    assert!(chunk.contextual_operation.is_none());
    assert_eq!(
        chunk.parameters[0].flow_type.ty,
        Type::List(Box::new(Type::Var(CONTEXTUAL_ITEM_VAR)))
    );
    let Type::List(chunk_row) = &chunk.result.ty else {
        panic!("List/chunk must return a list");
    };
    let Type::Object(chunk_row) = chunk_row.as_ref() else {
        panic!("List/chunk rows must be typed records");
    };
    assert_eq!(chunk_row.field_order, ["label", "items"]);
    assert_eq!(chunk_row.fields.get("label"), Some(&Type::Text));
    assert_eq!(
        chunk_row.fields.get("items"),
        Some(&Type::List(Box::new(Type::Var(CONTEXTUAL_ITEM_VAR))))
    );
    assert!(
        program
            .callables
            .iter()
            .find(|signature| signature.name == "identity")
            .expect("user signature")
            .contextual_operation
            .is_none()
    );
}

#[test]
fn checked_render_slot_use_is_owned_by_the_exact_statement() {
    let parsed = boon_parser::parse_source(
        "checked-render-slot-use.bn",
        r#"
document: [
    root: [kind: Text]
    debug: 1
]
ordinary: 2
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("valid source has a checked program");
    let root_statement = parsed
        .ast
        .statements
        .iter()
        .flat_map(|statement| statement.children.iter())
        .find(|statement| statement_field(statement).as_deref() == Some("root"))
        .expect("root statement");
    let checked_root = program
        .statements
        .iter()
        .find(|statement| statement.id == CheckedStatementId(root_statement.id as u32))
        .expect("checked root statement");

    assert_eq!(checked_root.value_use, CheckedValueUse::RenderSlot);
    assert_eq!(
        program
            .statements
            .iter()
            .filter(|statement| statement.value_use == CheckedValueUse::RenderSlot)
            .map(|statement| statement.id)
            .collect::<Vec<_>>(),
        [checked_root.id]
    );
    assert_eq!(
        output.report.render_slot_table.slots[0].value_expr_id,
        checked_root.value.map(|expression| expression.0 as usize)
    );
}

#[test]
fn checked_program_rejects_an_undriven_wrapper_output() {
    let parsed = boon_parser::parse_source(
        "undriven-out.bn",
        r#"
FUNCTION broken(list, entry: OUT, new) {
    list |> List/map(item, new: new)
}

result: LIST { 1 } |> broken(entry, new: entry)
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("output `entry` in `FUNCTION broken` has no structural producer")
    }));
}

#[test]
fn checked_program_rejects_multiple_output_producers() {
    let parsed = boon_parser::parse_source(
        "multiple-out-producers.bn",
        r#"
FUNCTION broken(list, entry: OUT, new) {
    BLOCK {
        first: list |> List/map(item: entry, new: new)
        second: list |> List/map(item: entry, new: new)
        first
    }
}

result: LIST { 1 } |> broken(entry, new: entry)
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("output `entry` in `FUNCTION broken` has 2 structural producers")
    }));
}

#[test]
fn checked_program_rejects_output_forwarding_cycles() {
    let parsed = boon_parser::parse_source(
        "out-cycle.bn",
        r#"
FUNCTION first(list, entry: OUT, new) {
    list |> second(entry: entry, new: new)
}

FUNCTION second(list, entry: OUT, new) {
    list |> first(entry: entry, new: new)
}

result: LIST { 1 } |> first(entry, new: entry)
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("OUT forwarding cycle") })
    );
}

#[test]
fn checked_program_enforces_declared_call_order() {
    let parsed = boon_parser::parse_source(
        "call-order.bn",
        r#"
FUNCTION combine(left, right) {
    left + right
}

result: combine(right: 2, left: 1)
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("call entry 1 must be `left`, found `right`")
    }));
}

#[test]
fn checked_program_keeps_pass_outside_function_arity() {
    let parsed = boon_parser::parse_source(
        "checked-pass.bn",
        r#"
FUNCTION render(value) {
    value
}

result: render(value: 1, PASS: [store: [count: 2]])
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    let program = output.program.expect("valid source has a checked program");
    let call = program
        .calls
        .iter()
        .find(|call| call.function == "render")
        .expect("render call is checked");
    assert_eq!(call.entries.len(), 1);
    assert!(call.pass.is_some());
}

#[test]
fn checked_program_owns_typed_scopes_and_contextual_evaluation() {
    let parsed = boon_parser::parse_source(
        "checked-scopes.bn",
        r#"
rows: LIST { [value: 1] }

result:
    rows
    |> List/map(
        item
        new: item.value * 2
    )
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("valid source has a checked program");
    assert_eq!(program.role, ProgramRole::Client);
    assert!(program.scopes.iter().any(|scope| {
        scope.kind == CheckedScopeKind::RepeatedOutput && scope.parent == Some(program.root_scope)
    }));
    assert!(program.declarations.iter().any(|declaration| {
        declaration.name == "rows" && declaration.kind == CheckedDeclarationKind::List
    }));
    assert!(
        program.expressions.iter().all(|expression| {
            !matches!(expression.kind, CheckedExpressionKind::Invalid { .. })
        })
    );
    assert!(
        program
            .expressions
            .iter()
            .any(|expression| { matches!(expression.flow_type.ty, Type::Number) })
    );

    let signature = program
        .callables
        .iter()
        .find(|signature| signature.name == "List/map")
        .expect("List/map signature exists");
    let item = signature
        .parameters
        .iter()
        .find(|parameter| parameter.name == "item")
        .expect("item OUT exists");
    let new = signature
        .parameters
        .iter()
        .find(|parameter| parameter.name == "new")
        .expect("new input exists");
    assert_eq!(
        new.evaluation_scope,
        CheckedEvaluationScope::Output {
            formal: item.decl_id
        }
    );
    let call = program
        .calls
        .iter()
        .find(|call| call.function == "List/map")
        .expect("map call exists");
    assert!(call.entries.iter().any(|entry| {
        matches!(
            entry,
            CheckedCallEntry::Input {
                name,
                evaluation_scope: CheckedEvaluationScope::Output { formal },
                ..
            } if name == "new" && *formal == item.decl_id
        )
    }));
}

#[test]
fn direct_find_instantiates_a_typed_found_payload() {
    let parsed = boon_parser::parse_source(
        "checked-direct-find.bn",
        r#"
rows: LIST { [value: 1] }

found:
    rows
    |> List/find(
        item
        if: item.value == 1
    )

selected:
    found |> WHEN {
        Found[value] => value
        NotFound => [value: 0]
    }
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("direct find is checked");
    let call = program
        .calls
        .iter()
        .find(|call| call.function == "List/find")
        .expect("find call");
    assert_number_row(found_payload_type(&call.result.ty).expect("Found payload"));
    assert_number_row(fresh_output_type(&program, call));

    let payload = program
        .declarations
        .iter()
        .find(|declaration| {
            declaration.kind == CheckedDeclarationKind::PatternBinding
                && declaration.name == "value"
        })
        .expect("Found payload declaration");
    assert_number_row(&payload.flow_type.ty);
    assert!(program.expressions.iter().any(|expression| {
        matches!(
            expression.kind,
            CheckedExpressionKind::Read { target, ref projection }
                if target == payload.id && projection.is_empty()
        )
    }));
}

#[test]
fn typed_find_converges_for_a_recursive_list_dependency() {
    let parsed = boon_parser::parse_source(
        "checked-recursive-list-find.bn",
        r#"
FUNCTION row_value(index) {
    index == 0 |> WHILE {
        True => 1
        False =>
            rows
            |> List/find(item, if: item.index == index - 1)
            |> WHEN {
                Found[value] => value.value
                NotFound => 0
            }
    }
}

rows:
    List/range(from: 0, to: 3)
    |> List/map(item, new: [
        index: item
        value: row_value(index: item)
    ])

selected:
    rows
    |> List/find(item, if: item.index == 2)
    |> WHEN {
        Found[value] => value.value
        NotFound => 0
    }
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("recursive typed find is checked");
    let finds = program
        .calls
        .iter()
        .filter(|call| call.function == "List/find")
        .collect::<Vec<_>>();
    assert_eq!(finds.len(), 2);
    for find in finds {
        let payload = found_payload_type(&find.result.ty).expect("typed Found payload");
        let Type::Object(shape) = payload else {
            panic!("Found payload is not a row: {payload:#?}");
        };
        assert_eq!(shape.fields.get("index"), Some(&Type::Number));
        assert_eq!(shape.fields.get("value"), Some(&Type::Number));
    }
}

#[test]
fn checked_selectors_retain_multiline_match_arms() {
    let parsed = boon_parser::parse_source(
        "checked-selector-arms.bn",
        r#"
FUNCTION choose(input) {
    input |> WHEN {
        Ready[value] => value
        fallback => fallback
    }
}

FUNCTION choose_continuous(input) {
    input |> WHILE {
        Ready[value] => value
        fallback => fallback
    }
}

selected: choose(input: Ready[value: 1])
continuous: choose_continuous(input: Ready[value: 2])
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("selectors are checked");

    let when_arms = program
        .expressions
        .iter()
        .find_map(|expression| match &expression.kind {
            CheckedExpressionKind::When { arms, .. } if arms.len() == 2 => Some(arms),
            _ => None,
        });
    let while_arms = program
        .expressions
        .iter()
        .find_map(|expression| match &expression.kind {
            CheckedExpressionKind::While { arms, .. } if arms.len() == 2 => Some(arms),
            _ => None,
        });
    for arms in [
        when_arms.expect("WHEN arms"),
        while_arms.expect("WHILE arms"),
    ] {
        assert!(arms.iter().all(|arm| matches!(
            program
                .expressions
                .iter()
                .find(|expression| expression.id == *arm)
                .map(|expression| &expression.kind),
            Some(CheckedExpressionKind::MatchArm {
                output: Some(_),
                ..
            })
        )));
    }
}

#[test]
fn checked_catch_all_binding_is_arm_local_and_visible_in_its_block() {
    let parsed = boon_parser::parse_source(
        "checked-catch-all-local.bn",
        r#"
FUNCTION recover(input) {
    input |> WHEN {
        Ready[value] => value
        fallback => BLOCK {
            copied: fallback
            copied
        }
    }
}

result: recover(input: Ready[value: 1])
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("catch-all binding is checked");
    let fallback = program
        .declarations
        .iter()
        .find(|declaration| {
            declaration.kind == CheckedDeclarationKind::PatternBinding
                && declaration.name == "fallback"
        })
        .expect("fallback declaration");
    let fallback_arm = program
        .expressions
        .iter()
        .find(|expression| {
            matches!(
                &expression.kind,
                CheckedExpressionKind::MatchArm { bindings, .. }
                    if bindings.contains(&fallback.id)
            )
        })
        .expect("arm carries fallback declaration");
    assert_ne!(fallback_arm.scope_id, program.root_scope);
    assert!(program.expressions.iter().any(|expression| {
        matches!(
            expression.kind,
            CheckedExpressionKind::Read { target, ref projection }
                if target == fallback.id && projection.is_empty()
        )
    }));
    assert!(!program.expressions.iter().any(|expression| {
        matches!(
            &expression.kind,
            CheckedExpressionKind::ExternalRead { canonical_path }
                if canonical_path == "fallback"
        )
    }));
}

#[test]
fn checked_catch_all_binding_is_visible_through_a_multiline_arm_pipeline() {
    let parsed = boon_parser::parse_source(
        "checked-catch-all-multiline-pipeline.bn",
        r#"
store: [
    active: TEXT { main.vcd }
    compare: TEXT { none }
    status:
        compare |> WHEN {
            TEXT { none } => TEXT { idle }
            selected =>
                TEXT { comparing }
                |> Text/concat(with: selected, separator: " ")
                |> Text/concat(with: active, separator: " ")
        }
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}; statements: {:#?}",
        output.report.diagnostics,
        parsed.ast.statements
    );
    let program = output.program.expect("multiline catch-all arm is checked");
    let selected = program
        .declarations
        .iter()
        .find(|declaration| {
            declaration.kind == CheckedDeclarationKind::PatternBinding
                && declaration.name == "selected"
        })
        .expect("selected pattern binding");
    assert!(program.expressions.iter().any(|expression| {
        matches!(
            expression.kind,
            CheckedExpressionKind::Read { target, ref projection }
                if target == selected.id && projection.is_empty()
        )
    }));
}

#[test]
fn checked_block_retains_stable_locals_result_and_forward_references() {
    let parsed = boon_parser::parse_source(
        "checked-block-forward-reference.bn",
        r#"
FUNCTION calculate(input) {
    BLOCK {
        answer: doubled + 1
        doubled: input * 2
        answer
    }
}

result: calculate(input: 3)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("BLOCK is checked");
    let signature = checked_callable(&program, "calculate");
    let block = program
        .expressions
        .iter()
        .find(|expression| expression.id == signature.result_expression.expect("BLOCK result"))
        .expect("checked BLOCK expression");
    let CheckedExpressionKind::Block { bindings, result } = &block.kind else {
        panic!("function result is not a checked BLOCK: {block:?}");
    };
    assert_eq!(bindings.len(), 2);
    let answer = program
        .declarations
        .iter()
        .find(|declaration| declaration.id == bindings[0].declaration)
        .expect("answer declaration");
    let doubled = program
        .declarations
        .iter()
        .find(|declaration| declaration.id == bindings[1].declaration)
        .expect("doubled declaration");
    assert_eq!(answer.name, "answer");
    assert_eq!(doubled.name, "doubled");
    assert!(matches!(
        program
            .expressions
            .iter()
            .find(|expression| expression.id == bindings[0].value)
            .map(|expression| &expression.kind),
        Some(CheckedExpressionKind::Infix { left, .. })
            if program.expressions.iter().any(|expression| {
                expression.id == *left
                    && matches!(expression.kind, CheckedExpressionKind::Read { target, .. } if target == doubled.id)
            })
    ));
    assert!(matches!(
        result.and_then(|result| {
            program
                .expressions
                .iter()
                .find(|expression| expression.id == result)
                .map(|expression| &expression.kind)
        }),
        Some(CheckedExpressionKind::Read { target, projection })
            if *target == answer.id && projection.is_empty()
    ));
}

#[test]
fn one_wrapper_find_preserves_the_call_site_item_type() {
    let parsed = boon_parser::parse_source(
        "checked-one-wrapper-find.bn",
        r#"
FUNCTION find_row(list, entry: OUT, if) {
    list |> List/find(item: entry, if: if)
}

rows: LIST { [value: 1] }
found: rows |> find_row(entry, if: entry.value == 1)
selected:
    found |> WHEN {
        Found[value] => value
        NotFound => [value: 0]
    }
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("wrapped find is checked");
    let call = program
        .calls
        .iter()
        .find(|call| call.function == "find_row")
        .expect("wrapper call");
    assert_number_row(found_payload_type(&call.result.ty).expect("Found payload"));
    assert_number_row(fresh_output_type(&program, call));
    let signature = checked_callable(&program, "find_row");
    assert_eq!(
        found_payload_type(&signature.result.ty),
        Some(&Type::Var(CONTEXTUAL_ITEM_VAR))
    );
}

#[test]
fn two_wrapper_find_preserves_the_call_site_item_type() {
    let parsed = boon_parser::parse_source(
        "checked-two-wrapper-find.bn",
        r#"
FUNCTION find_row(list, entry: OUT, if) {
    list |> List/find(item: entry, if: if)
}

FUNCTION find_row_twice(list, row: OUT, if) {
    list |> find_row(entry: row, if: if)
}

rows: LIST { [value: 1] }
found: rows |> find_row_twice(row, if: row.value == 1)
selected:
    found |> WHEN {
        Found[value] => value
        NotFound => [value: 0]
    }
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("twice-wrapped find is checked");
    let call = program
        .calls
        .iter()
        .find(|call| call.function == "find_row_twice")
        .expect("outer wrapper call");
    assert_number_row(found_payload_type(&call.result.ty).expect("Found payload"));
    assert_number_row(fresh_output_type(&program, call));
    for wrapper in ["find_row", "find_row_twice"] {
        assert_eq!(
            found_payload_type(&checked_callable(&program, wrapper).result.ty),
            Some(&Type::Var(CONTEXTUAL_ITEM_VAR)),
            "{wrapper} must remain generic"
        );
    }
}

#[test]
fn scalar_range_map_keeps_number_item_and_result_types() {
    let parsed = boon_parser::parse_source(
        "checked-range-map.bn",
        r#"
result:
    List/range(from: 0, to: 3)
    |> List/map(item, new: item * 2)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("scalar map is checked");
    let call = program
        .calls
        .iter()
        .find(|call| call.function == "List/map")
        .expect("map call");
    assert_eq!(fresh_output_type(&program, call), &Type::Number);
    assert_eq!(call.result.ty, Type::List(Box::new(Type::Number)));
    assert_eq!(
        output
            .report
            .expr_type_table
            .entries
            .iter()
            .find(|entry| entry.expr_id == call.expression.0 as usize)
            .map(|entry| &entry.flow_type.ty),
        Some(&Type::List(Box::new(Type::Number))),
        "the report must expose CheckedProgram's call-site instance"
    );
}

#[test]
fn identical_binder_names_are_instantiated_independently() {
    let parsed = boon_parser::parse_source(
        "checked-independent-binders.bn",
        r#"
numbers:
    List/range(from: 0, to: 3)
    |> List/map(item, new: item * 2)

rows: LIST { [label: TEXT { ready }] }
labels:
    rows
    |> List/map(item, new: item.label)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("independent maps are checked");
    let calls = program
        .calls
        .iter()
        .filter(|call| call.function == "List/map")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    let output_types = calls
        .iter()
        .map(|call| fresh_output_type(&program, call).clone())
        .collect::<Vec<_>>();
    assert!(output_types.contains(&Type::Number));
    assert!(output_types.iter().any(|ty| {
        matches!(ty, Type::Object(shape) if shape.fields.get("label") == Some(&Type::Text))
    }));
    let result_types = calls
        .iter()
        .map(|call| call.result.ty.clone())
        .collect::<Vec<_>>();
    assert!(result_types.contains(&Type::List(Box::new(Type::Number))));
    assert!(result_types.contains(&Type::List(Box::new(Type::Text))));
}

#[test]
fn scalar_contextual_output_rejects_field_projection() {
    let parsed = boon_parser::parse_source(
        "checked-number-field-projection.bn",
        r#"
result:
    List/range(from: 0, to: 3)
    |> List/map(item, new: item.value)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(output.program.is_none());
    assert!(output.report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("cannot project field `value` from NUMBER")
    }));
}

#[test]
fn mapped_stateful_record_preserves_all_fields_for_a_second_map() {
    let parsed = boon_parser::parse_source(
        "checked-stateful-record-map.bn",
        r#"
FUNCTION remember_channels(row) {
    [
        count: row.count |> HOLD count { LATEST {} }
        label: row.label |> HOLD label { LATEST {} }
        enabled: row.enabled |> HOLD enabled { LATEST {} }
    ]
}

remembered:
    LIST {
        [count: 1, label: TEXT { ready }, enabled: True]
    }
    |> List/map(item, new: remember_channels(row: item))

projected:
    remembered
    |> List/map(item, new: [
        count: item.count
        label: item.label
        enabled: item.enabled
    ])
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("stateful record maps are checked");
    let map_results = program
        .calls
        .iter()
        .filter(|call| call.function == "List/map")
        .map(|call| &call.result.ty)
        .collect::<Vec<_>>();
    assert_eq!(map_results.len(), 2);
    for (index, result) in map_results.into_iter().enumerate() {
        let Type::List(item) = result else {
            panic!("map result is not a list: {result:?}");
        };
        let Type::Object(shape) = item.as_ref() else {
            panic!("map item is not a record: {item:?}");
        };
        assert_eq!(
            shape.fields.get("count"),
            Some(&Type::Number),
            "map {index} lost count: {result:?}"
        );
        assert_eq!(
            shape.fields.get("label"),
            Some(&Type::Text),
            "map {index} lost label: {result:?}"
        );
        assert!(
            shape.fields.contains_key("enabled"),
            "map {index} lost enabled: {result:?}"
        );
    }
}

#[test]
fn filtered_list_when_preserves_the_source_item_shape() {
    let parsed = boon_parser::parse_source(
        "checked-filtered-list-when.bn",
        r#"
catalog: LIST {
    [id: TEXT { alpha }]
    [id: TEXT { beta }]
}

visible:
    True |> WHEN {
        True => catalog |> List/filter(item, if: item.id == TEXT { alpha })
        False => catalog
    }
"#,
    )
    .unwrap();
    let visible_statement = parsed
        .ast
        .statements
        .iter()
        .find(|statement| {
            matches!(
                &statement.kind,
                AstStatementKind::Field { name } if name == "visible"
            )
        })
        .expect("visible statement");
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("filtered list WHEN is checked");
    let expected = Type::List(Box::new(Type::Object(ObjectShape::from_ordered_fields(
        [("id".to_owned(), Type::Text)],
        false,
    ))));

    let filter = program
        .calls
        .iter()
        .find(|call| call.function == "List/filter")
        .expect("filter call");
    assert_eq!(filter.result.ty, expected);
    let declaration = program
        .declarations
        .iter()
        .find(|declaration| {
            declaration.name == "visible" && declaration.kind == CheckedDeclarationKind::Field
        })
        .expect("visible declaration");
    assert_eq!(declaration.flow_type.ty, expected);
    let checked_statement = program
        .statements
        .iter()
        .find(|statement| statement.id == CheckedStatementId(visible_statement.id as u32))
        .expect("checked visible statement");
    let value = checked_statement.value.expect("visible checked value");
    let expression = program
        .expressions
        .iter()
        .find(|expression| expression.id == value)
        .expect("visible checked expression");
    assert_eq!(expression.flow_type.ty, expected);
    assert_eq!(
        output
            .report
            .expr_type_table
            .entries
            .iter()
            .find(|entry| entry.expr_id == value.0 as usize)
            .map(|entry| &entry.flow_type.ty),
        Some(&expected)
    );
}

#[test]
fn checked_record_function_exposes_a_canonical_result_expression() {
    let parsed = boon_parser::parse_source(
        "checked-record-result.bn",
        r#"
FUNCTION entry_view(entry) {
    [
        id: entry.id
    ]
}

result: entry_view(entry: [id: 1])
"#,
    )
    .unwrap();
    let function = parsed.ast.statements.first().expect("function statement");
    assert!(matches!(function.kind, AstStatementKind::Function { .. }));
    let [record_block] = function.children.as_slice() else {
        panic!("record function must parse as Function -> Block");
    };
    assert!(matches!(record_block.kind, AstStatementKind::Block));
    let [field] = record_block.children.as_slice() else {
        panic!("record block must contain exactly one field");
    };
    assert!(matches!(
        &field.kind,
        AstStatementKind::Field { name } if name == "id"
    ));
    assert!(record_block.indent > function.indent);

    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("valid source has a checked program");
    let result = wrapper_signature_result_expression(&program, "entry_view").unwrap_or_else(|| {
        panic!(
            "record function has no checked result; AST statements: {:#?}; expressions: {:#?}",
            parsed.ast.statements, parsed.expressions
        )
    });
    assert_eq!(
        record_block
            .expr
            .map(|expression| CheckedExprId(expression as u32)),
        Some(result),
        "record result must retain the parser-owned structural expression ID"
    );

    let signature = checked_callable(&program, "entry_view");
    let entry = signature
        .parameters
        .iter()
        .find(|parameter| parameter.name == "entry")
        .expect("entry parameter");
    assert_eq!(signature.body, Some(CheckedStatementId(function.id as u32)));
    let checked_function = program
        .statements
        .iter()
        .find(|statement| statement.id == CheckedStatementId(function.id as u32))
        .expect("checked function statement");
    let checked_block = program
        .statements
        .iter()
        .find(|statement| statement.id == CheckedStatementId(record_block.id as u32))
        .expect("checked record block");
    assert_eq!(checked_function.value, Some(result));
    assert_eq!(checked_block.value, Some(result));

    let record = program
        .expressions
        .iter()
        .find(|expression| expression.id == result)
        .expect("parser-owned checked record");
    let CheckedExpressionKind::Record { fields } = &record.kind else {
        panic!("record block lowered to non-record expression: {record:?}");
    };
    let [id_field] = fields.as_slice() else {
        panic!("record must contain exactly one field");
    };
    assert_eq!(id_field.name, "id");
    assert_ne!(id_field.value, result);
    let value = program
        .expressions
        .iter()
        .find(|expression| expression.id == id_field.value)
        .expect("record field value is closed over a checked expression");
    assert!(matches!(
        &value.kind,
        CheckedExpressionKind::Read { target, projection }
            if *target == entry.decl_id && projection == &["id"]
    ));
    assert_no_unbound_calls(&parsed, &program);
}

#[test]
fn checked_record_field_with_a_parameter_name_is_not_self_referential() {
    let parsed = boon_parser::parse_source(
        "checked-record-parameter-shadow.bn",
        r#"
FUNCTION wrapped(value) {
    [
        value: value
    ]
}

result: wrapped(value: TEXT { ok })
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("valid record wrapper is checked");
    let signature = checked_callable(&program, "wrapped");
    let parameter = signature.parameters.first().expect("value parameter");
    let result = signature
        .result_expression
        .expect("canonical record result");
    let record = program
        .expressions
        .iter()
        .find(|expression| expression.id == result)
        .expect("record result expression");
    let CheckedExpressionKind::Record { fields } = &record.kind else {
        panic!("record wrapper result is not a checked record");
    };
    let value = fields.first().expect("value field").value;
    assert!(matches!(
        program
            .expressions
            .iter()
            .find(|expression| expression.id == value)
            .map(|expression| &expression.kind),
        Some(CheckedExpressionKind::Read { target, projection })
            if *target == parameter.decl_id && projection.is_empty()
    ));
    assert_no_unbound_calls(&parsed, &program);
}

#[test]
fn checked_program_rejects_a_canonical_local_expansion_cycle() {
    let parsed = boon_parser::parse_source(
        "checked-record-cycle.bn",
        r#"
FUNCTION broken() {
    [
        value: value
    ]
}

result: broken()
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(output.program.is_none());
    assert!(output.report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("canonical checked value contains an expansion cycle")
    }));
}

#[test]
fn checked_program_rejects_a_root_sibling_as_a_function_result() {
    let parsed = boon_parser::parse_source(
        "checked-unindented-record-result.bn",
        r#"
FUNCTION broken(value) {
[
    value: value
]
}
"#,
    )
    .unwrap();
    let function = parsed.ast.statements.first().expect("function statement");
    assert!(function.children.is_empty());
    assert!(
        parsed
            .ast
            .statements
            .iter()
            .skip(1)
            .any(|statement| matches!(statement.kind, AstStatementKind::Block))
    );

    let output = check_program(&parsed);
    assert!(output.program.is_none());
    assert!(output.report.diagnostics.iter().any(|diagnostic| {
        diagnostic.message
            == "FUNCTION `broken` has no canonical checked result expression in its indented body"
    }));
}
#[test]
fn checked_nested_hold_update_reads_use_the_nearest_field_owner() {
    let parsed = boon_parser::parse_source(
        "nearest-hold-owner.bn",
        r#"
store: [
    change: SOURCE
    prefix:
        TEXT { al } |> HOLD prefix {
            change.text |> THEN { change.text }
        }
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "{:#?}",
        output.report.diagnostics
    );
    let program = output.program.unwrap();
    let change = program
        .declarations
        .iter()
        .find(|declaration| {
            declaration.name == "change" && declaration.kind == CheckedDeclarationKind::Source
        })
        .unwrap()
        .id;
    let prefix = program
        .declarations
        .iter()
        .find(|declaration| {
            declaration.name == "prefix" && declaration.kind == CheckedDeclarationKind::Field
        })
        .unwrap()
        .id;
    let reads = program
        .expressions
        .iter()
        .filter_map(|expression| match expression.kind {
            CheckedExpressionKind::Read { target, .. } if target == change => {
                Some((expression.id, expression.declaration))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        reads.len(),
        2,
        "checked expressions: {:#?}",
        program.expressions
    );
    assert!(
        reads.iter().all(|(_, owner)| *owner == Some(prefix)),
        "source reads: {reads:#?}; expected owner: {prefix:?}",
    );
}
