use boon_checked::Type;
use boon_compiler::{CompilerCheckRequest, check_editor_source};
use boon_plan::ProgramRole;

fn assert_untaken_requirement_is_not_exported(body: &str) {
    let source = format!(
        r#"
FUNCTION choose(which, value) {{
    which |> WHEN {{
        First => 0
        Second => {body}
    }}
}}

FUNCTION wrapper(value) {{
    choose(which: First, value: value)
}}
result: wrapper(value: TEXT {{ unconstrained }})
"#
    );
    let checked = check_editor_source(CompilerCheckRequest::source_text(
        "untaken-transfer-requirement.bn",
        &source,
        ProgramRole::Server,
    ))
    .unwrap();
    assert!(
        !checked.output.report.has_errors(),
        "an untaken branch must not reject the concrete caller: {:?}",
        checked.output.report
    );
    let wrapper = checked
        .output
        .report
        .function_type_table
        .entries
        .iter()
        .find(|function| function.name == "wrapper")
        .expect("wrapper interface");
    assert!(
        matches!(wrapper.parameters[0].flow_type.ty, Type::Var(_)),
        "untaken requirements escaped into wrapper: {wrapper:?}"
    );
    assert_eq!(wrapper.result.ty, Type::Number);
}

#[test]
fn untaken_pattern_requirement_does_not_constrain_wrapper_formal() {
    assert_untaken_requirement_is_not_exported("value |> WHEN { Wrapped[item] => item + 1 }");
}

#[test]
fn untaken_field_requirement_does_not_constrain_wrapper_formal() {
    assert_untaken_requirement_is_not_exported("value.item + 1");
}

#[test]
fn untaken_nested_field_requirement_does_not_constrain_wrapper_formal() {
    assert_untaken_requirement_is_not_exported("value.item.inner + 1");
}

#[test]
fn taken_nested_field_requirement_constrains_wrapper_formal() {
    let source = r#"
FUNCTION choose(which, value) {
    which |> WHEN {
        First => 0
        Second => value.item.inner + 1
    }
}

FUNCTION wrapper(value) {
    choose(which: Second, value: value)
}
result: wrapper(value: [item: [inner: 2]])
"#;
    let checked = check_editor_source(CompilerCheckRequest::source_text(
        "taken-transfer-requirement.bn",
        source,
        ProgramRole::Server,
    ))
    .unwrap();
    assert!(
        !checked.output.report.has_errors(),
        "{:?}",
        checked.output.report
    );
    let wrapper = checked
        .output
        .report
        .function_type_table
        .entries
        .iter()
        .find(|function| function.name == "wrapper")
        .expect("wrapper interface");
    let Type::Object(root) = &wrapper.parameters[0].flow_type.ty else {
        panic!("taken requirement must constrain wrapper: {wrapper:?}")
    };
    let Type::Object(item) = &root.fields["item"] else {
        panic!("taken nested requirement must retain its path: {root:?}")
    };
    assert_eq!(item.fields["inner"], Type::Number);
    assert_eq!(wrapper.result.ty, Type::Number);
}

#[test]
fn whole_selector_forwarding_does_not_export_an_inactive_callee_alternative() {
    let source = r#"
FUNCTION inner(which) {
    which |> WHEN {
        First => 1
        InnerOnly => 2
    }
}
FUNCTION wrapper(which) {
    which |> WHEN {
        First => inner(which: which)
        Second => 0
    }
}
result: wrapper(which: First)
"#;
    let legacy_source =
        boon_parser::parse_source("whole-selector-transfer-guard.bn", source).unwrap();
    let legacy = boon_typecheck::check_program(&legacy_source);
    assert!(
        legacy.report.diagnostics.is_empty(),
        "legacy oracle: {:?}",
        legacy.report.diagnostics
    );
    let legacy_wrapper = legacy
        .report
        .function_type_table
        .entries
        .iter()
        .find(|function| function.name == "wrapper")
        .expect("legacy wrapper interface");
    let expected_domain = Type::VariantSet(
        vec![
            boon_checked::Variant::Tag("First".to_owned()),
            boon_checked::Variant::Tag("Second".to_owned()),
        ]
        .into(),
    );
    assert_eq!(legacy_wrapper.parameters[0].flow_type.ty, expected_domain);
    let checked = check_editor_source(CompilerCheckRequest::source_text(
        "whole-selector-transfer-guard.bn",
        source,
        ProgramRole::Server,
    ))
    .unwrap();
    assert!(
        !checked.output.report.has_errors(),
        "{:?}",
        checked.output.report
    );
    let wrapper = checked
        .output
        .report
        .function_type_table
        .entries
        .iter()
        .find(|function| function.name == "wrapper")
        .expect("wrapper interface");
    let Type::VariantSet(domain) = &wrapper.parameters[0].flow_type.ty else {
        panic!("wrapper must retain its own closed domain: {wrapper:?}");
    };
    assert!(!domain.iter().any(|variant| matches!(variant,
        boon_checked::Variant::Tag(tag) | boon_checked::Variant::Tagged { tag, .. } if tag == "InnerOnly")),
        "an inner alternative cannot escape an outer First-only invocation: {wrapper:?}");
}

#[test]
fn whole_selector_guard_does_not_hide_an_incompatible_callee() {
    let source = r#"
FUNCTION inner(which) {
    which + 1
}
FUNCTION wrapper(which) {
    which |> WHEN {
        First => inner(which: which)
        Second => 0
    }
}
result: wrapper(which: First)
"#;
    let parsed = boon_parser::parse_source("incompatible-whole-selector-guard.bn", source).unwrap();
    let legacy = boon_typecheck::check_program(&parsed);
    assert!(
        legacy.report.has_errors(),
        "legacy oracle must reject the guarded call"
    );
    let checked = check_editor_source(CompilerCheckRequest::source_text(
        "incompatible-whole-selector-guard.bn",
        source,
        ProgramRole::Server,
    ))
    .unwrap();
    assert!(
        checked.output.report.diagnostics.iter().any(|diagnostic| {
            diagnostic.line == 7
                && diagnostic
                    .message
                    .contains("`FUNCTION inner` argument `which` has an incompatible type")
        }),
        "filtering reverse requirements must not erase the incompatible guarded actual: {:?}",
        checked.output.report
    );
}

#[test]
fn shared_pattern_transfer_keeps_bare_alternatives_in_closed_domain() {
    let source = r#"
FUNCTION choose(which) {
    which |> WHEN {
        Plain => 0
        Wrapped[value] => value + 1
    }
}
FUNCTION wrapper(which) {
    choose(which: which)
}
result: wrapper(which: Plain)
"#;
    let checked = check_editor_source(CompilerCheckRequest::source_text(
        "closed-pattern-transfer-domain.bn",
        source,
        ProgramRole::Server,
    ))
    .unwrap();
    assert!(
        !checked.output.report.has_errors(),
        "{:?}",
        checked.output.report
    );
    let wrapper = checked
        .output
        .report
        .function_type_table
        .entries
        .iter()
        .find(|function| function.name == "wrapper")
        .expect("wrapper interface");
    let Type::VariantSet(domain) = &wrapper.parameters[0].flow_type.ty else {
        panic!("closed domain must remain a variant set: {wrapper:?}")
    };
    assert!(
        domain
            .iter()
            .any(|variant| matches!(variant, boon_checked::Variant::Tag(tag) if tag == "Plain"))
    );
    assert!(domain.iter().any(|variant| matches!(variant,
        boon_checked::Variant::Tagged { tag, fields } if tag == "Wrapped" && fields.fields["value"] == Type::Number)));
}

#[test]
fn tagged_whole_selector_forwarding_keeps_the_tag_and_payload() {
    let source = r#"
FUNCTION inner(which) {
    which.value + 1
}
FUNCTION wrapper(which) {
    which |> WHEN {
        Wrapped[value] => inner(which: which)
        Other => 0
    }
}
result: wrapper(which: Wrapped[value: 1])
"#;
    let legacy_source =
        boon_parser::parse_source("tagged-whole-selector-transfer.bn", source).unwrap();
    let legacy = boon_typecheck::check_program(&legacy_source);
    assert!(
        legacy.report.diagnostics.is_empty(),
        "legacy oracle: {:?}",
        legacy.report.diagnostics
    );
    let checked = check_editor_source(CompilerCheckRequest::source_text(
        "tagged-whole-selector-transfer.bn",
        source,
        ProgramRole::Server,
    ))
    .unwrap();
    assert!(
        !checked.output.report.has_errors(),
        "{:?}",
        checked.output.report
    );
    let wrapper = checked
        .output
        .report
        .function_type_table
        .entries
        .iter()
        .find(|function| function.name == "wrapper")
        .expect("wrapper interface");
    let Type::VariantSet(domain) = &wrapper.parameters[0].flow_type.ty else {
        panic!("wrapper must retain its own closed domain: {wrapper:?}")
    };
    assert!(
        domain
            .iter()
            .any(|variant| matches!(variant, boon_checked::Variant::Tag(tag) if tag == "Other")),
        "the bare alternative must survive whole-value forwarding: {wrapper:?}"
    );
    assert!(
        domain.iter().any(|variant| matches!(variant,
            boon_checked::Variant::Tagged { tag, fields }
                if tag == "Wrapped" && fields.fields["value"] == Type::Number)),
        "the guarded call must keep the tag and its payload requirement: {wrapper:?}"
    );
}

#[test]
fn tagged_whole_selector_forwarding_rejects_an_incompatible_payload() {
    let source = r#"
FUNCTION inner(which) {
    which.value + 1
}
FUNCTION wrapper(which) {
    which |> WHEN {
        Wrapped[value] => inner(which: which)
        Other => 0
    }
}
result: wrapper(which: Wrapped[value: TEXT { x }])
"#;
    let parsed = boon_parser::parse_source("incompatible-tagged-selector.bn", source).unwrap();
    let legacy = boon_typecheck::check_program(&parsed);
    assert!(
        legacy.report.has_errors(),
        "legacy oracle must reject the tagged call"
    );
    let checked = check_editor_source(CompilerCheckRequest::source_text(
        "incompatible-tagged-selector.bn",
        source,
        ProgramRole::Server,
    ))
    .unwrap();
    let errors = checked
        .output
        .report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == boon_checked::DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert_eq!(
        errors.len(),
        1,
        "the incompatible tagged payload must stay rejected: {:?}",
        checked.output.report
    );
    let message = &errors[0].message;
    assert!(
        message.contains("NUMBER") && message.contains("TEXT"),
        "the rejection must name the incompatible payload types: {message}"
    );
    // The legacy oracle attributes this to the guarded inner call. The
    // candidate transmits the payload requirement through the wrapper's public
    // formal, so the same rejection is reported at the call whose concrete
    // actual violates it. The rejection itself is the checked contract here.
}

#[test]
fn nested_tagged_whole_selector_forwarding_rejects_an_incompatible_payload() {
    let source = r#"
FUNCTION inner(which) {
    which.value + 1
}
FUNCTION middle(which) {
    which |> WHEN {
        Wrapped[value] => inner(which: which)
        Other => 0
    }
}
FUNCTION outer(which) {
    middle(which: which)
}
result: outer(which: Wrapped[value: TEXT { x }])
"#;
    let parsed =
        boon_parser::parse_source("nested-incompatible-tagged-selector.bn", source).unwrap();
    let legacy = boon_typecheck::check_program(&parsed);
    assert!(
        legacy.report.has_errors(),
        "legacy oracle must reject the nested tagged call"
    );
    let checked = check_editor_source(CompilerCheckRequest::source_text(
        "nested-incompatible-tagged-selector.bn",
        source,
        ProgramRole::Server,
    ))
    .unwrap();
    assert!(
        checked.output.report.has_errors(),
        "the nested tagged payload mismatch must stay rejected: {:?}",
        checked.output.report
    );
}
