fn closed_tag(name: &str) -> Type {
    Type::VariantSet(vec![Variant::Tag(name.to_owned())].into())
}

fn closed_record(fields: impl IntoIterator<Item = (&'static str, Type)>) -> Type {
    Type::object(ObjectShape::from_ordered_fields(
        fields.into_iter().map(|(name, ty)| (name.to_owned(), ty)),
        false,
    ))
}

#[test]
fn boundary_union_is_canonical_and_keeps_structural_alternatives() {
    let result = canonical_union_type(vec![
        Type::Number,
        closed_tag("Invalid"),
        Type::Union(vec![Type::Number, closed_tag("Unavailable")]),
        Type::Absent,
    ]);

    let Type::Union(members) = result else {
        panic!("expected a structural union");
    };
    assert_eq!(members.len(), 2);
    assert!(members.contains(&Type::Number));
    assert!(members.contains(&Type::VariantSet(vec![
        Variant::Tag("Invalid".to_owned()),
        Variant::Tag("Unavailable".to_owned()),
    ]
    .into())));
}

#[test]
fn boundary_union_assignability_is_covariant_over_every_actual_member() {
    let error = closed_tag("Invalid");
    let expected = canonical_union_type(vec![Type::Number, error.clone()]);

    assert!(type_is_assignable_to(&Type::Number, &expected));
    assert!(type_is_assignable_to(&error, &expected));
    assert!(!type_is_assignable_to(&Type::Text, &expected));
    assert!(type_is_assignable_to(
        &canonical_union_type(vec![Type::Number, error.clone()]),
        &expected,
    ));
    assert!(!type_is_assignable_to(
        &canonical_union_type(vec![Type::Number, Type::Text]),
        &expected,
    ));
    assert!(!type_is_assignable_to(&Type::Union(Vec::new()), &expected));
    assert!(!type_is_assignable_to(
        &Type::Number,
        &Type::Union(Vec::new())
    ));
}

#[test]
fn boundary_union_conflicts_only_when_every_alternative_conflicts() {
    let error = closed_tag("Invalid");
    let union = canonical_union_type(vec![Type::Number, error.clone()]);

    assert!(!concrete_type_conflict(&union, &Type::Number));
    assert!(!concrete_type_conflict(&union, &error));
    assert!(concrete_type_conflict(&union, &Type::Text));
}

#[test]
fn boundary_union_projection_preserves_every_projectable_member() {
    let union = canonical_union_type(vec![
        closed_record([("value", Type::Number)]),
        closed_record([("value", Type::Text)]),
        closed_tag("Invalid"),
    ]);

    assert_eq!(
        type_for_nested_path(&union, &["value".to_owned()]),
        Some(canonical_union_type(vec![Type::Number, Type::Text])),
    );
}

#[test]
fn boundary_union_unification_selects_the_compatible_structural_arm() {
    let variable = TypeVar(99);
    let pattern = canonical_union_type(vec![
        Type::Text,
        closed_record([("value", Type::Var(variable))]),
    ]);
    let actual = closed_record([("value", Type::Number)]);
    let mut substitutions = BTreeMap::new();

    unify_checked_type_pattern(&pattern, &actual, &mut substitutions);

    assert_eq!(substitutions.get(&variable), Some(&Type::Number));
}

#[test]
fn resolved_structural_assignability_preserves_authority_and_narrowing() {
    let truth = Type::VariantSet(vec![
        Variant::Tag("False".to_owned()),
        Variant::Tag("True".to_owned()),
    ]
    .into());
    let false_only = closed_tag("False");
    let authority = closed_record([("completed", truth.clone()), ("title", Type::Text)]);
    let narrower = Type::object(ObjectShape::from_ordered_fields(
        [
            ("completed".to_owned(), false_only.clone()),
            ("title".to_owned(), Type::Text),
            ("extra".to_owned(), Type::Number),
        ],
        false,
    ));

    assert!(resolved_type_is_assignable_to(&false_only, &truth));
    assert!(!resolved_type_is_assignable_to(&truth, &false_only));
    assert!(resolved_type_is_assignable_to(&narrower, &authority));
    assert!(!resolved_type_is_assignable_to(&authority, &narrower));
    assert!(!resolved_type_is_assignable_to(
        &Type::object(ObjectShape::from_ordered_fields([], true)),
        &authority,
    ));
}

#[test]
fn named_flush_boundary_reports_the_exposed_payload_union() {
    let parsed = boon_parser::parse_source(
        "named-flush-boundary.bn",
        r#"
store: [
    result:
        Valid
        |> WHEN {
            Valid => Complete
            __ => FLUSH { Invalid }
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
    let entry = output
        .report
        .named_value_type_table
        .entries
        .iter()
        .find(|entry| entry.path == "store.result")
        .expect("named result entry");
    assert_eq!(
        entry.flow_type.ty,
        canonical_union_type(vec![closed_tag("Complete"), closed_tag("Invalid")]),
        "checked program: {:#?}",
        output.program
    );
}

#[test]
fn executable_flush_example_reports_every_named_boundary_payload() {
    let parsed = boon_parser::parse_source(
        "flush_error_propagation.bn",
        include_str!("../../../../examples/flush_error_propagation.bn"),
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let entry = output
        .report
        .named_value_type_table
        .entries
        .iter()
        .find(|entry| entry.path == "store.normal_pipeline")
        .expect("normal pipeline entry");
    assert_eq!(
        entry.flow_type.ty,
        canonical_union_type(vec![
            closed_tag("NormalComplete"),
            closed_tag("NormalError")
        ]),
        "checked program: {:#?}",
        output.program
    );
}

#[test]
fn potentially_flushing_hold_initializer_is_rejected() {
    let parsed = boon_parser::parse_source(
        "flushing-hold-initializer.bn",
        r#"
store: [
    state:
        FLUSH { InitialError }
        |> HOLD state {}
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(output.program.is_none());
    assert!(output.report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("a `HOLD` initializer must produce a valid storable value and cannot `FLUSH`")
    }));
}

#[test]
fn collection_and_flow_authority_flush_payloads_are_rejected() {
    for (name, source) in [
        (
            "collection-flush-payload.bn",
            "result: FLUSH { LIST { Invalid } }\n",
        ),
        (
            "flow-flush-payload.bn",
            "trigger: SOURCE\nresult: FLUSH { trigger }\n",
        ),
    ] {
        let parsed = boon_parser::parse_source(name, source).unwrap();
        let output = check_program(&parsed);
        assert!(output.program.is_none(), "{name} unexpectedly typechecked");
        assert!(
            output.report.diagnostics.iter().any(|diagnostic| {
                diagnostic.message.contains(
                    "`FLUSH` payload must be a continuous closed Tag, tagged object, or closed union without collection, flow, or host values",
                )
            }),
            "{name} diagnostics: {:#?}",
            output.report.diagnostics
        );
    }
}
