#[test]
fn checked_when_consumes_the_comparison_result() {
    let parsed = boon_parser::parse_source(
        "checked-comparison-when.bn",
        r#"
store: [
    left: [value: 1]
    right: [value: 1]
    result:
        left
        == right
        |> WHEN {
            True => TEXT { equal }
            False => TEXT { different }
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
    let program = output.program.expect("valid source has a checked program");
    let when = program
        .expressions
        .iter()
        .find(|expression| matches!(expression.kind, CheckedExpressionKind::When { .. }))
        .expect("checked WHEN expression");
    let CheckedExpressionKind::When { input, .. } = when.kind else {
        unreachable!();
    };
    assert!(program.expressions.iter().any(|expression| {
        expression.id == input
            && matches!(
                expression.kind,
                CheckedExpressionKind::Infix { ref op, .. } if op == "=="
            )
    }));
}

#[test]
fn checked_multiline_then_keeps_the_temporal_statement_root() {
    let parsed = boon_parser::parse_source(
        "checked-multiline-then-root.bn",
        r#"
store: [
    start: SOURCE
    result:
        Initial |> HOLD result {
            start |> THEN {
                True
                == True
                |> WHEN {
                    True => Ready
                    False => SKIP
                }
            }
        }
]
"#,
    )
    .unwrap();
    let then = parsed
        .expressions
        .iter()
        .find(|expression| matches!(expression.kind, AstExprKind::Then { .. }))
        .expect("multiline THEN expression");
    let then_statement = parsed
        .ast
        .statements
        .iter()
        .flat_map(|statement| statement.children.iter())
        .flat_map(|statement| statement.children.iter())
        .flat_map(|statement| statement.children.iter())
        .find(|statement| statement.expr == Some(then.id))
        .expect("THEN statement");

    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("fixture is checked");
    let checked_statement = program
        .statements
        .iter()
        .find(|statement| statement.id == CheckedStatementId(then_statement.id as u32))
        .expect("checked THEN statement");
    assert_eq!(checked_statement.value, Some(CheckedExprId(then.id as u32)));
}

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

#[test]
fn checked_program_uses_the_parser_link_for_a_multiline_pipeline() {
    let parsed = boon_parser::parse_source(
        "checked-linked-pipeline.bn",
        r#"
result:
    1
    |> Number/ceil()
"#,
    )
    .unwrap();
    let pipeline = parsed
        .expressions
        .iter()
        .find(|expression| {
            matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/ceil")
        })
        .expect("multiline Number/ceil pipeline");
    let linked_input = pipeline
        .linked_input
        .expect("parser must own the multiline pipeline link");

    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("linked pipeline is checked");
    let call = program
        .calls
        .iter()
        .find(|call| call.expression == CheckedExprId(pipeline.id as u32))
        .expect("pipeline call");
    assert!(call.entries.iter().any(|entry| {
        matches!(
            entry,
            CheckedCallEntry::Input {
                value,
                from_pipe: true,
                ..
            } if *value == CheckedExprId(linked_input as u32)
        )
    }));
}

#[test]
fn checked_program_rejects_a_multiline_pipeline_without_its_parser_link() {
    let mut parsed = boon_parser::parse_source(
        "checked-missing-pipeline-link.bn",
        r#"
result:
    1
    |> Number/ceil()
"#,
    )
    .unwrap();
    let pipeline = parsed
        .expressions
        .iter_mut()
        .find(|expression| {
            matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/ceil")
        })
        .expect("multiline Number/ceil pipeline");
    assert!(pipeline.linked_input.take().is_some());

    let output = check_program(&parsed);
    assert!(output.program.is_none());
    assert_eq!(
        output
            .report
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.message == "pipeline continuation is missing its exact linked input"
            })
            .count(),
        1,
        "diagnostics: {:#?}",
        output.report.diagnostics
    );

    let provisional = check_program_with_external_types(
        &parsed,
        &ExternalTypeEnvironment::provisional(ProgramRole::Client),
    );
    assert!(
        provisional.program.is_none(),
        "structurally malformed linkage must not enter a provisional CheckedProgram"
    );
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
fn render_call_owns_element_state_without_shadowing_its_provider_input() {
    let parsed = boon_parser::parse_source(
        "render-call-context.bn",
        r#"
document: Document/new(root: render_button(element: []))

FUNCTION render_button(element) {
    Element/button(
        element: element
        style: [
            opacity:
                element.hovered |> WHILE {
                    True => 1
                    False => 0.5
                }
        ]
        label: TEXT { Test }
    )
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
    let program = output.program.expect("valid render source is checked");
    let call = program
        .calls
        .iter()
        .find(|call| call.function == "Element/button")
        .expect("button call is checked");
    let [context] = call.contexts.as_slice() else {
        panic!("button call must own exactly one element-state context")
    };
    let context_declaration = program
        .declarations
        .iter()
        .find(|declaration| declaration.id == context.declaration)
        .expect("element-state declaration exists");
    assert_eq!(
        context_declaration.kind,
        CheckedDeclarationKind::ElementState
    );
    let provider = call
        .entries
        .iter()
        .find_map(|entry| match entry {
            CheckedCallEntry::Input { name, value, .. } if name == "element" => Some(*value),
            _ => None,
        })
        .expect("element provider input exists");
    assert!(matches!(
        program
            .expressions
            .iter()
            .find(|expression| expression.id == provider)
            .expect("provider expression is checked")
            .kind,
        CheckedExpressionKind::Read {
            target,
            ref projection,
            ..
        } if target != context.declaration && projection.is_empty()
    ));
    assert!(program.expressions.iter().any(|expression| {
        matches!(
            &expression.kind,
            CheckedExpressionKind::Read {
                target, projection, ..
            }
                if *target == context.declaration && projection == &["hovered".to_owned()]
        )
    }));
    assert!(!program.expressions.iter().any(|expression| {
        matches!(
            &expression.kind,
            CheckedExpressionKind::ExternalRead { canonical_path }
                if canonical_path == "element.hovered"
        )
    }));
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
            initial |> HOLD value {
                PASSED
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
    assert!(
        effect.emits_source,
        "nested SOURCE must affect the callable"
    );
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
    assert_callable_parameters(
        &program,
        "Scene/Element/text_input",
        &[
            "element",
            "input_id",
            "style",
            "label",
            "text",
            "placeholder",
            "visible",
            "target",
            "focus",
        ],
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
        "List/query",
        "List/query_prefix",
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
fn numeric_infix_operators_reject_text_coercion() {
    for operator in ["+", "-", "*", "/", "%", ">", "<", ">=", "<="] {
        let parsed = boon_parser::parse_source(
            "numeric-infix-types.bn",
            &format!("value: TEXT {{ A }} {operator} 1\n"),
        )
        .unwrap();
        let report = check(&parsed);

        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .message
                    .contains(&format!("operator `{operator}` operand has incompatible type"))
                    && diagnostic.message.contains("expected: NUMBER")
                    && diagnostic.message.contains("found: TEXT")
            }),
            "operator `{operator}` diagnostics: {:#?}",
            report.diagnostics
        );
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
ordered:
    catalog
    |> List/sort_by(
        item
        key: item.name
        direction: Ascending
    )
    |> List/then_by(
        item
        key: item.id
        direction: Descending
    )
    |> List/take(count: 20)
page: ordered |> List/page(size: 20, after: Start)
visible: Bool/or(left: True, right: False)
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
        "List/sort_by",
        &["list", "item", "key", "direction"],
    );
    assert_callable_parameters(
        &program,
        "List/then_by",
        &["list", "item", "key", "direction"],
    );
    assert_callable_parameters(&program, "List/take", &["list", "count"]);
    assert_callable_parameters(&program, "List/page", &["list", "size", "after"]);
    assert_callable_parameters(&program, "Bool/or", &["left", "right"]);
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
    let sort = checked_callable(&program, "List/sort_by");
    let item = sort
        .parameters
        .iter()
        .find(|parameter| parameter.name == "item")
        .expect("sort item output");
    let key = sort
        .parameters
        .iter()
        .find(|parameter| parameter.name == "key")
        .expect("sort key");
    assert_eq!(item.kind, CheckedParameterKind::Out);
    assert_eq!(
        key.evaluation_scope,
        CheckedEvaluationScope::Output {
            formal: item.decl_id
        }
    );
    assert!(matches!(
        checked_callable(&program, "List/page").result.ty,
        Type::VariantSet(_)
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
value: items |> List/sort_by(item, direction: Ascending, key: item.name)
"#,
            "must be `key`, found `direction`",
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
fn checked_chunk_rejects_caller_selected_result_field_names() {
    let parsed = boon_parser::parse_source(
        "chunk-result-field-names.bn",
        r#"
rows: LIST { [value: 1] }
chunks: List/chunk(
    list: rows
    size: 1
    items: TEXT { custom_items }
    label: TEXT { custom_label }
)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(output.program.is_none());
    for name in ["items", "label"] {
        assert!(
            output.report.diagnostics.iter().any(|diagnostic| {
                diagnostic.message
                    == format!("`List/chunk` has an unexpected extra call entry `{name}`")
            }),
            "diagnostics: {:#?}",
            output.report.diagnostics
        );
    }
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
    let fresh_item = program
        .declarations
        .iter()
        .find(|declaration| {
            declaration.name == "item" && declaration.kind == CheckedDeclarationKind::FreshOut
        })
        .expect("List/map call creates one item output");
    assert!(program.scopes.iter().any(|scope| {
        scope.kind == CheckedScopeKind::RepeatedOutput
            && scope.parent == Some(fresh_item.scope_id)
            && scope.owner == Some(fresh_item.id)
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
            CheckedExpressionKind::Read {
                target,
                ref projection,
                ..
            }
                if target == payload.id && projection.is_empty()
        )
    }));
}

#[test]
fn propagating_error_branches_preserve_the_success_value_type() {
    let parsed = boon_parser::parse_source(
        "checked-propagating-error-result.bn",
        r#"
FUNCTION parse_number(text) {
    text |> Text/to_number() |> WHILE {
        NaN => Error/new(code: TEXT { invalid_number })
        number => number
    }
}

result: parse_number(text: TEXT { 41 }) + 1
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("error-capable function is checked");
    let signature = program
        .callables
        .iter()
        .find(|signature| signature.name == "parse_number")
        .expect("parse_number signature");
    assert_eq!(signature.result.ty, Type::Number);
}

#[test]
fn typed_find_rejects_an_unknown_found_payload_field() {
    let parsed = boon_parser::parse_source(
        "checked-direct-find-unknown-payload.bn",
        r#"
rows: LIST { [value: 1] }

selected:
    rows |> List/find(item, if: item.value == 1) |> WHEN {
        Found[row] => row
        NotFound => [value: 0]
    }
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(output.report.has_errors());
    assert!(output.report.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains(
            "tagged pattern `Found[row]` binds unknown payload field `row`; payload fields: value",
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
            rows |> List/find(item, if: item.index == index - 1) |> WHEN {
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
    rows |> List/find(item, if: item.index == 2) |> WHEN {
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
            CheckedExpressionKind::Read {
                target,
                ref projection,
                ..
            }
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
            CheckedExpressionKind::Read {
                target,
                ref projection,
                ..
            }
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
        Some(CheckedExpressionKind::Read {
            target, projection, ..
        })
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
        CheckedExpressionKind::Read {
            target, projection, ..
        }
            if *target == entry.decl_id && projection == &["id"]
    ));
    assert_no_unbound_calls(&parsed, &program);
}

#[test]
fn checked_record_field_initializer_reads_a_same_name_outer_parameter() {
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
    let program = output
        .program
        .expect("same-name outer parameter is checked");
    let signature = checked_callable(&program, "wrapped");
    let parameter = signature.parameters.first().expect("value parameter");
    let result = signature.result_expression.expect("record result");
    let field_value = program
        .expressions
        .iter()
        .find(|expression| expression.id == result)
        .and_then(|expression| match &expression.kind {
            CheckedExpressionKind::Record { fields } => fields.first().map(|field| field.value),
            _ => None,
        })
        .expect("record field value");
    assert!(matches!(
        program
            .expressions
            .iter()
            .find(|expression| expression.id == field_value)
            .map(|expression| &expression.kind),
        Some(CheckedExpressionKind::Read {
            target,
            projection,
            ..
        }) if *target == parameter.decl_id && projection.is_empty()
    ));
}

#[test]
fn checked_record_field_can_read_a_differently_named_outer_parameter() {
    let parsed = boon_parser::parse_source(
        "checked-record-distinct-parameter.bn",
        r#"
FUNCTION wrapped(input) {
    [
        value: input
    ]
}

result: wrapped(input: TEXT { ok })
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
    let parameter = signature.parameters.first().expect("input parameter");
    let result = signature
        .result_expression
        .expect("canonical record result");
    let record = program
        .expressions
        .iter()
        .find(|expression| expression.id == result)
        .expect("record result expression");
    let CheckedExpressionKind::Record { fields } = &record.kind else {
        panic!("record wrapper result is not a checked record: {record:?}");
    };
    let value = fields.first().expect("value field").value;
    assert!(matches!(
        program
            .expressions
            .iter()
            .find(|expression| expression.id == value)
            .map(|expression| &expression.kind),
        Some(CheckedExpressionKind::Read {
            target, projection, ..
        })
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
        first: second
        second: first
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
fn tagged_pattern_arms_require_only_their_own_payload_fields() {
    let parsed = boon_parser::parse_source(
        "tagged-pattern-parameter-requirements.bn",
        r#"
FUNCTION display(value) {
    value |> WHEN {
        TextValue => value.text
        NumberValue => value.number |> Number/to_text()
    }
}

text: display(value: TextValue[text: TEXT { ok }])
number: display(value: NumberValue[number: 3])
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    assert!(output.program.is_some());
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

#[test]
fn named_hold_alias_is_bound_to_its_state_declaration() {
    let parsed = boon_parser::parse_source(
        "named-hold-alias.bn",
        r#"
FUNCTION row(todo) {
    [
        change: SOURCE
        edit_text:
            todo.title |> HOLD draft {
                change |> THEN { draft }
            }
    ]
}

rows:
    LIST { [title: TEXT { one }] }
    |> List/map(item, new: row(todo: item))
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("checked program");
    let state = program
        .declarations
        .iter()
        .find(|declaration| declaration.name == "edit_text")
        .expect("edit_text declaration");
    assert!(
        program.expressions.iter().any(|expression| matches!(
            &expression.kind,
            CheckedExpressionKind::Read {
                target, projection, ..
            }
                if *target == state.id && projection.is_empty() && expression.span.line == 7
        )),
        "named HOLD alias must be an exact state read: {:#?}",
        program.expressions
    );
    assert!(
        !program.expressions.iter().any(|expression| matches!(
            &expression.kind,
            CheckedExpressionKind::ExternalRead { canonical_path } if canonical_path == "draft"
        )),
        "named HOLD alias escaped as an ambient read: {:#?}",
        program.expressions
    );
}

#[test]
fn render_helper_may_return_no_element_but_plain_state_may_not() {
    let render_source = boon_parser::parse_source(
        "render-helper-no-element.bn",
        r#"
FUNCTION optional_child(show) {
    show |> WHEN {
        True => Scene/Element/text(element: [], text: TEXT { visible })
        False => NoElement
    }
}

document: Scene/new(root: optional_child(show: True))
"#,
    )
    .unwrap();
    let render_output = check_program(&render_source);
    assert!(
        !render_output.report.has_errors(),
        "diagnostics: {:#?}",
        render_output.report.diagnostics
    );

    let state_source =
        boon_parser::parse_source("state-no-element.bn", "value: NoElement\n").unwrap();
    let state_output = check_program(&state_source);
    assert!(state_output.report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("`NoElement` can only be used as a render value")
    }));
}

#[test]
fn nested_render_helpers_preserve_parameter_types_through_chunked_rows() {
    let parsed = boon_parser::parse_source(
        "chunked-render-helper-parameters.bn",
        r#"
FUNCTION display_name(signal) {
    signal.alias |> Text/is_empty() |> WHEN {
        True => signal.name
        False => signal.alias
    }
}

FUNCTION name_text(label_text) {
    Scene/Element/text(element: [], text: label_text)
}

FUNCTION signal_button(signal) {
    signal_row(signal: signal)
}

FUNCTION signal_row(signal) {
    name_text(label_text: display_name(signal: signal))
}

FUNCTION chunk_row(row) {
    Scene/Element/stripe(
        element: []
        direction: Column
        items: row.items |> List/map(item, new: signal_button(signal: item))
    )
}

signal: TEXT { ambient binding must not type a function parameter }

rows:
    List/chunk(
        list: LIST { [name: TEXT { clock }, alias: Text/empty()] }
        size: 1
    )

document:
    Scene/new(
        root: Scene/Element/stripe(
            element: []
            direction: Column
            items: rows |> List/map(item, new: chunk_row(row: item))
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
}

#[test]
fn generic_user_calls_keep_exact_recursive_types_per_call_site() {
    let parsed = boon_parser::parse_source(
        "generic-user-call-instances.bn",
        r#"
FUNCTION chunk_rows(list, size) {
    list |> List/chunk(size: size)
}

text_rows: LIST { [value: TEXT { ready }] }
number_rows: LIST { [value: 7] }
text_result: text_rows |> chunk_rows(size: 1)
number_result: number_rows |> chunk_rows(size: 1)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("generic calls are checked");
    let calls = program
        .calls
        .iter()
        .filter(|call| call.function == "chunk_rows")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);

    let result_value_type = |call: &CheckedCall| {
        let Type::List(chunks) = &call.result.ty else {
            panic!("chunk helper result is not a list: {:?}", call.result.ty);
        };
        let Type::Object(chunk) = chunks.as_ref() else {
            panic!("chunk helper item is not a record: {chunks:?}");
        };
        let Some(Type::List(items)) = chunk.fields.get("items") else {
            panic!("chunk helper item has no typed items: {chunk:?}");
        };
        let Type::Object(item) = items.as_ref() else {
            panic!("chunk contents are not records: {items:?}");
        };
        item.fields.get("value").cloned()
    };
    assert_eq!(result_value_type(calls[0]), Some(Type::Text));
    assert_eq!(result_value_type(calls[1]), Some(Type::Number));
    assert_ne!(calls[0].type_substitutions, calls[1].type_substitutions);

    let signature = checked_callable(&program, "chunk_rows");
    assert!(checked_signature_is_generic(signature));
    assert!(checked_type_contains_var(&signature.result.ty));
}

#[test]
fn generic_user_call_flow_is_instantiated_per_call_site() {
    let parsed = boon_parser::parse_source(
        "generic-user-call-flow.bn",
        r#"
FUNCTION passthrough(value) {
    BLOCK {
        forwarded: value
        forwarded
    }
}

store: [pulse: SOURCE]
continuous: passthrough(value: TEXT { ready })
event: passthrough(value: store.pulse)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("passthrough calls are checked");
    let calls = program
        .calls
        .iter()
        .filter(|call| call.function == "passthrough")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].result.mode, FlowMode::Continuous);
    assert_eq!(calls[1].result.mode, FlowMode::PresentOrAbsent);
}

#[test]
fn generic_record_helpers_preserve_nested_call_results_in_list_rows() {
    let parsed = boon_parser::parse_source(
        "generic-record-list-rows.bn",
        r#"
store: [
    places: LIST {
        place(id: TEXT { alpha }, name: TEXT { Alpha }, x: 1.5, y: 2.5)
        place(id: TEXT { beta }, name: TEXT { Beta }, x: 3.5, y: 4.5)
    }
]

FUNCTION point(x, y) {
    BLOCK {
        point_x: x
        point_y: y
        [x: point_x, y: point_y]
    }
}

FUNCTION place(id, name, x, y) {
    BLOCK {
        place_id: id
        place_name: name
        [
            id: place_id
            name: place_name
            point: point(x: x, y: y)
        ]
    }
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
    let program = output.program.expect("record helpers are checked");
    let calls = program
        .calls
        .iter()
        .filter(|call| call.function == "place")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    for call in calls {
        let Type::Object(place) = &call.result.ty else {
            panic!("place result is not a record: {:?}", call.result.ty);
        };
        assert!(!place.open);
        assert_eq!(place.fields.get("id"), Some(&Type::Text));
        assert_eq!(place.fields.get("name"), Some(&Type::Text));
        let Some(Type::Object(point)) = place.fields.get("point") else {
            panic!("place result has no typed point: {place:?}");
        };
        assert!(!point.open);
        assert_eq!(point.fields.get("x"), Some(&Type::Number));
        assert_eq!(point.fields.get("y"), Some(&Type::Number));
    }
}

#[test]
fn forward_referenced_event_aliases_remain_present_or_absent_for_then() {
    let parsed = boon_parser::parse_source(
        "forward-event-alias-flow.bn",
        r#"
store: [
    reset: forwarded |> THEN { TEXT { reset } }
    forwarded: pressed
    pressed: SOURCE
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
    let program = output.program.expect("forward event aliases are checked");
    for name in ["pressed", "forwarded", "reset"] {
        let declaration = program
            .declarations
            .iter()
            .find(|declaration| declaration.name == name)
            .unwrap_or_else(|| panic!("missing declaration `{name}`"));
        assert_eq!(
            declaration.flow_type.mode,
            FlowMode::PresentOrAbsent,
            "`{name}` lost its event flow: {declaration:#?}"
        );
    }
}

#[test]
fn block_record_parameters_are_instantiated_without_value_placeholders() {
    let parsed = boon_parser::parse_source(
        "fjordpulse-health-record.bn",
        r#"
FUNCTION health(version, request_count, readiness) {
    BLOCK {
        health_version: version
        health_request_count: request_count
        health_readiness: readiness
        [
            status: TEXT { ok }
            version: health_version
            readiness: health_readiness
            request_count: health_request_count
        ]
    }
}

admin_status: health(
    version: TEXT { v2 }
    request_count: 41
    readiness: [ready: True]
)
text_status: health(
    version: TEXT { test }
    request_count: TEXT { synthetic }
    readiness: [ready: False]
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
    let program = output.program.expect("health records are checked");
    let calls = program
        .calls
        .iter()
        .filter(|call| call.function == "health")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    let request_count_type = |call: &CheckedCall| {
        let Type::Object(result) = &call.result.ty else {
            panic!("health result is not a record: {:?}", call.result.ty);
        };
        assert!(!result.open);
        result.fields.get("request_count").cloned()
    };
    assert_eq!(request_count_type(calls[0]), Some(Type::Number));
    assert_eq!(request_count_type(calls[1]), Some(Type::Text));
}

#[test]
fn projected_generic_rows_remain_closed_across_nested_list_maps() {
    let parsed = boon_parser::parse_source(
        "todo-migration-v2-generic-rows.bn",
        r#"
FUNCTION retiring_todo(todo) {
    [title: todo.title, completed: todo.completed]
}

FUNCTION new_task(task) {
    [
        sources: [toggle: SOURCE]
        title: task.title
        completed: task.completed
    ]
}

todos:
    LIST {
        [title: TEXT { Plan release }, completed: False]
        [title: TEXT { Test migration }, completed: True]
    }
    |> List/map(item, new: retiring_todo(todo: item))

tasks:
    todos
    |> List/map(item, new: new_task(task: item))
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("nested map rows are checked");
    let call = program
        .calls
        .iter()
        .find(|call| call.function == "new_task")
        .expect("new_task call");
    let Type::Object(task) = &call.result.ty else {
        panic!("new_task result is not a record: {:?}", call.result.ty);
    };
    assert!(!task.open);
    assert_eq!(task.fields.get("title"), Some(&Type::Text));
    assert!(matches!(
        task.fields.get("completed"),
        Some(Type::VariantSet(variants))
            if variants.contains(&Variant::Tag("True".to_owned()))
                && variants.contains(&Variant::Tag("False".to_owned()))
    ));
    let Some(Type::Object(sources)) = task.fields.get("sources") else {
        panic!("new_task result has no sources record: {task:?}");
    };
    assert!(!sources.open);
    assert!(sources.fields.contains_key("toggle"));
}

#[test]
fn static_when_specialization_visits_only_reachable_ordered_variant_arms() {
    let parsed = boon_parser::parse_source(
        "static-when-reachability.bn",
        r#"
result:
    choice |> WHEN {
        TraceEdge => 1
        TraceFill => TEXT { fill }
        fallback => BYTES {}
        Never => [value: 1]
    }
"#,
    )
    .unwrap();
    let when = parsed
        .expressions
        .iter()
        .find(|expression| matches!(expression.kind, AstExprKind::When { .. }))
        .expect("parsed WHEN expression");
    let trace_edge = Type::VariantSet(vec![Variant::Tag("TraceEdge".to_owned())]);

    let reachable = reachable_static_when_arms(when.id, &parsed.expressions, Some(&trace_edge));
    assert_eq!(reachable.len(), 1, "singleton variants select one arm");
    assert_eq!(reachable[0].pattern, ["TraceEdge"]);

    let bindings = BTreeMap::from([("choice".to_owned(), trace_edge)]);
    assert_eq!(
        static_when_type_from_bindings(when.id, &parsed.expressions, &bindings),
        Some(Type::Number),
        "unreachable text, bytes, and record arms must not widen the specialized result"
    );

    let edge_or_other = Type::VariantSet(vec![
        Variant::Tag("TraceEdge".to_owned()),
        Variant::Tag("Other".to_owned()),
    ]);
    let reachable = reachable_static_when_arms(when.id, &parsed.expressions, Some(&edge_or_other));
    assert_eq!(
        reachable
            .iter()
            .map(|arm| arm.pattern.join(" "))
            .collect::<Vec<_>>(),
        vec!["TraceEdge".to_owned(), "fallback".to_owned()],
        "the fallback consumes only variants left by earlier arms and ends ordered matching"
    );
}

#[test]
fn checked_nested_when_flow_inference_is_linear_per_epoch() {
    fn nested_when(depth: usize, indent: usize) -> String {
        let pad = " ".repeat(indent);
        if depth == 0 {
            return format!("{pad}1");
        }
        let nested = nested_when(depth - 1, indent + 8);
        format!("{pad}value |> WHEN {{\n{pad}    True =>\n{nested}\n{pad}    False => 0\n{pad}}}")
    }

    let call_count = 24;
    let calls = (0..call_count)
        .map(|index| format!("result_{index}: nested_flow(value: True)\n"))
        .collect::<String>();
    let source = format!(
        "FUNCTION nested_flow(value) {{\n{}\n}}\n\n{calls}",
        nested_when(32, 4)
    );
    let parsed = boon_parser::parse_source("nested-when-linear-flow.bn", &source).unwrap();
    reset_checked_flow_inference_test_stats();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let stats = checked_flow_inference_test_stats();
    assert!(stats.epoch_count > 0, "checked inference ran no epochs");
    assert!(
        stats.epoch_count < call_count,
        "inference epochs scaled with call count: stats={stats:?}, calls={call_count}"
    );
    assert!(
        stats.max_epoch_computations <= parsed.expressions.len(),
        "one epoch recomputed shared expressions: stats={stats:?}, expressions={}",
        parsed.expressions.len()
    );
}

#[test]
fn checked_user_call_validation_rejects_a_missing_required_field() {
    let parsed = boon_parser::parse_source(
        "checked-user-call-missing-field.bn",
        r#"
store: [
    label: row_label(row: [id: 1])
]

FUNCTION row_label(row) {
    row.name
}
"#,
    )
    .unwrap();
    let report = check(&parsed);

    assert!(report.has_errors());
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("object is missing field `name`")
    }));
}

#[test]
fn checked_user_call_validation_preserves_tagged_payload_narrowing() {
    let parsed = boon_parser::parse_source(
        "checked-user-call-tagged-payload.bn",
        r#"
store: [
    text_label: value_label(value: TextValue[text: TEXT { ready }])
    number_label: value_label(value: NumberValue[number: 5])
]

FUNCTION value_label(value) {
    value |> WHEN {
        TextValue => value.text
        NumberValue => value.number |> Number/to_text(radix: 10)
    }
}
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "tagged payload diagnostics: {:#?}",
        output.report.diagnostics
    );
    for path in ["store.text_label", "store.number_label"] {
        assert_eq!(
            output
                .report
                .named_value_type_table
                .entries
                .iter()
                .find(|entry| entry.path == path)
                .map(|entry| &entry.flow_type.ty),
            Some(&Type::Text),
            "{path}"
        );
    }
}

#[test]
fn projected_generic_calls_do_not_alias_unrelated_parameter_fields() {
    let parsed = boon_parser::parse_source(
        "projected-generic-call-fields.bn",
        r#"
store: [
    rows: LIST {
        [
            completed: False
            sources: [
                first: [events: [press: []]]
                second: [events: [press: []]]
            ]
        ]
    }
    rendered:
        rows
        |> List/map(item, new: render_row(row: item))
]

FUNCTION render_row(row) {
    [
        completed: bool_label(value: row.completed)
        first: preserve(value: row.sources.first.events)
        second: preserve(value: row.sources.second.events)
    ]
}

FUNCTION bool_label(value) {
    value |> WHEN {
        True => TEXT { complete }
        False => TEXT { active }
    }
}

FUNCTION preserve(value) {
    value
}
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "projected generic diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("valid source has a checked program");
    let row = program
        .callables
        .iter()
        .find(|signature| signature.name == "render_row")
        .and_then(|signature| signature.parameters.first())
        .map(|parameter| &parameter.flow_type.ty)
        .expect("render_row row parameter");
    let Type::Object(row) = row else {
        panic!("render_row row parameter is not structural: {row:?}");
    };
    let completed = row.fields.get("completed").expect("completed field");
    let events = type_for_nested_path(
        &Type::Object(row.clone()),
        &[
            "sources".to_owned(),
            "first".to_owned(),
            "events".to_owned(),
        ],
    )
    .expect("first event field");
    assert_ne!(completed, &events, "unrelated projections share one type variable");
}
