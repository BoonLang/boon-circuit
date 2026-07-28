fn closed_tag(name: &str) -> Type {
    Type::VariantSet(vec![Variant::Tag(name.to_owned())])
}

fn closed_record(fields: impl IntoIterator<Item = (&'static str, Type)>) -> Type {
    Type::Object(ObjectShape::from_ordered_fields(
        fields
            .into_iter()
            .map(|(name, ty)| (name.to_owned(), ty)),
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
    ])));
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
