use super::*;

#[test]
fn structural_widening_reuses_unchanged_shared_nodes() {
    let nested = Type::object(ObjectShape::from_ordered_fields(
        [("value".to_owned(), Type::Text)],
        false,
    ));
    let object = Type::object(ObjectShape::from_ordered_fields(
        [
            ("name".to_owned(), Type::Text),
            ("nested".to_owned(), nested),
        ],
        false,
    ));
    let Type::Object(object_shape) = &object else {
        unreachable!();
    };

    let widened_equal = widen_structural_type(&object, &object.clone());
    let Type::Object(widened_equal_shape) = &widened_equal else {
        unreachable!();
    };
    assert!(SharedObjectShape::ptr_eq(object_shape, widened_equal_shape));

    let partial = Type::object(ObjectShape::from_ordered_fields(
        [("name".to_owned(), Type::Text)],
        false,
    ));
    let widened_partial = widen_structural_type(&object, &partial);
    let Type::Object(widened_partial_shape) = &widened_partial else {
        unreachable!();
    };
    assert!(SharedObjectShape::ptr_eq(
        object_shape,
        widened_partial_shape
    ));

    let list_item = Type::shared(object);
    let list = Type::List(list_item.clone());
    let widened_list = widen_structural_type(&list, &list.clone());
    let Type::List(widened_list_item) = widened_list else {
        unreachable!();
    };
    assert!(SharedType::ptr_eq(&list_item, &widened_list_item));
}

#[test]
fn structural_widening_still_materializes_real_object_growth() {
    let left = Type::object(ObjectShape::from_ordered_fields(
        [("left".to_owned(), Type::Text)],
        false,
    ));
    let right = Type::object(ObjectShape::from_ordered_fields(
        [("right".to_owned(), Type::Number)],
        true,
    ));
    let widened = widen_structural_type(&left, &right);
    let Type::Object(shape) = widened else {
        unreachable!();
    };
    assert_eq!(
        shape.field_order,
        vec!["left".to_owned(), "right".to_owned()]
    );
    assert_eq!(shape.fields.get("left"), Some(&Type::Text));
    assert_eq!(shape.fields.get("right"), Some(&Type::Number));
    assert!(shape.open);
}

#[test]
fn structural_widening_normalizes_an_incomplete_object_order() {
    let left = Type::object(ObjectShape {
        fields: BTreeMap::from([("value".to_owned(), Type::Text)]),
        field_order: Vec::new(),
        open: false,
    });
    let right = Type::object(ObjectShape::from_ordered_fields(
        [("value".to_owned(), Type::Text)],
        false,
    ));
    let widened = widen_structural_type(&left, &right);
    let Type::Object(shape) = widened else {
        unreachable!();
    };
    assert_eq!(shape.field_order, vec!["value".to_owned()]);
}

#[test]
fn structural_widening_reuses_canonical_variant_sets_until_they_grow() {
    let variants = SharedVariantSet::new(vec![Variant::Tag("Ready".to_owned())]);
    let current = Type::VariantSet(variants.clone());
    let widened_equal = widen_structural_type(&current, &current.clone());
    let Type::VariantSet(widened_equal_variants) = widened_equal else {
        unreachable!();
    };
    assert!(SharedVariantSet::ptr_eq(&variants, &widened_equal_variants));

    let widened_growth = widen_structural_type(
        &current,
        &Type::VariantSet(vec![Variant::Tag("Waiting".to_owned())].into()),
    );
    let Type::VariantSet(widened_growth_variants) = widened_growth else {
        unreachable!();
    };
    assert!(!SharedVariantSet::ptr_eq(
        &variants,
        &widened_growth_variants
    ));
    assert_eq!(
        widened_growth_variants.as_ref(),
        &vec![
            Variant::Tag("Ready".to_owned()),
            Variant::Tag("Waiting".to_owned())
        ]
    );
}

#[test]
fn allocation_free_variant_order_matches_the_canonical_text_key() {
    let tagged = |tag: &str, field_count: usize| Variant::Tagged {
        tag: tag.to_owned(),
        fields: ObjectShape::from_ordered_fields::<SharedObjectShape>(
            (0..field_count).map(|index| (format!("field_{index}"), Type::Text)),
            false,
        ),
    };
    let candidates = vec![
        tagged("a", 2),
        tagged("a", 10),
        tagged("a:2", 0),
        Variant::Tag("z".to_owned()),
        Variant::Tag("a".to_owned()),
    ];
    let mut expected = candidates.clone();
    expected.sort_by_key(|variant| match variant {
        Variant::Tag(tag) => format!("0:{tag}"),
        Variant::Tagged { tag, fields } => format!("1:{tag}:{}", fields.fields.len()),
    });
    let mut actual = candidates;
    actual.sort_by(compare_variants_canonically);
    assert_eq!(actual, expected);
}

#[test]
fn stable_input_retries_are_deferred_but_other_call_causes_remain_live() {
    let call = CheckedCallId(7);
    let mut pending = CheckedTypeInferencePending {
        defer_inputs_enabled: true,
        ..CheckedTypeInferencePending::default()
    };
    let mut stats = CheckedTypeInferenceWorkStats::default();

    pending.defer_input(call);
    assert!(!pending.input_is_deferred(call));
    pending.defer_input(call);
    assert!(pending.input_is_deferred(call));

    CheckedProgramDatabase::enqueue_checked_call(
        call,
        CheckedCallEnqueueCause::Input,
        &mut pending,
        &mut stats,
    );
    assert!(!pending.calls.contains(call));
    assert!(pending.artifact_calls.contains(call));
    assert_eq!(stats.call_input_enqueues, 0);
    assert_eq!(stats.call_artifact_enqueues, 1);

    CheckedProgramDatabase::enqueue_checked_call(
        call,
        CheckedCallEnqueueCause::Callee,
        &mut pending,
        &mut stats,
    );
    assert!(pending.calls.contains(call));
    assert_eq!(stats.call_callee_enqueues, 1);

    pending.clear_deferred_input(call);
    assert!(!pending.input_is_deferred(call));
}

#[test]
fn concrete_syntax_result_survives_closed_principal_finalization() {
    let principal = Type::Number;
    let occurrence = Type::object(ObjectShape::from_ordered_fields(
        [("color".to_owned(), Type::Text)],
        false,
    ));

    assert_eq!(
        finalize_checked_call_occurrence_result(&principal, &occurrence, true),
        occurrence
    );
    assert_eq!(
        finalize_checked_call_occurrence_result(&principal, &occurrence, false),
        principal
    );
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

fn first_json_difference(
    path: &mut String,
    left: &serde_json::Value,
    right: &serde_json::Value,
) -> Option<(String, serde_json::Value, serde_json::Value)> {
    match (left, right) {
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                let previous_len = path.len();
                path.push_str(&format!("[{index}]"));
                if let Some(difference) = first_json_difference(path, left, right) {
                    return Some(difference);
                }
                path.truncate(previous_len);
            }
            (left.len() != right.len()).then(|| {
                (
                    format!("{path}.length"),
                    left.len().into(),
                    right.len().into(),
                )
            })
        }
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            let mut keys = left.keys().chain(right.keys()).collect::<Vec<_>>();
            keys.sort_unstable();
            keys.dedup();
            for key in keys {
                let previous_len = path.len();
                path.push('.');
                path.push_str(key);
                let difference = match (left.get(key), right.get(key)) {
                    (Some(left), Some(right)) => first_json_difference(path, left, right),
                    (left, right) => Some((
                        path.clone(),
                        left.cloned().unwrap_or(serde_json::Value::Null),
                        right.cloned().unwrap_or(serde_json::Value::Null),
                    )),
                };
                if difference.is_some() {
                    return difference;
                }
                path.truncate(previous_len);
            }
            None
        }
        _ if left == right => None,
        _ => Some((path.clone(), left.clone(), right.clone())),
    }
}

// Typecheck tests are grouped by language surface while staying in this module for private helper access.
include!("tests/reactive_collections.rs");
include!("tests/flush.rs");
include!("tests/maps_sets.rs");
include!("tests/bits.rs");
include!("tests/numbers.rs");
include!("tests/pulses.rs");
include!("tests/styles.rs");

#[test]
fn checked_type_inference_pending_sets_drain_in_canonical_id_order() {
    let mut pending = CheckedTypeInferencePending::default();
    pending.expressions.extend([9, 2, 5, 2]);
    pending
        .declarations
        .extend([DeclId(8), DeclId(1), DeclId(3), DeclId(1)]);
    pending
        .callables
        .extend([DeclId(12), DeclId(4), DeclId(7), DeclId(4)]);
    pending.calls.extend([
        CheckedCallId(6),
        CheckedCallId(0),
        CheckedCallId(2),
        CheckedCallId(0),
    ]);

    let expressions = pending.expressions.drain_sorted();
    assert_eq!(expressions, vec![2, 5, 9]);
    let expression_capacity = expressions.capacity();
    pending.expressions.recycle(expressions);
    assert_eq!(
        pending.declarations.drain_sorted(),
        vec![DeclId(1), DeclId(3), DeclId(8)]
    );
    assert_eq!(
        pending.callables.drain_sorted(),
        vec![DeclId(4), DeclId(7), DeclId(12)]
    );
    assert_eq!(
        pending.calls.drain_sorted(),
        vec![CheckedCallId(0), CheckedCallId(2), CheckedCallId(6)]
    );
    assert!(!pending.any());

    // Recycling retains the dense worklist buffer and still sorts a later
    // round canonically.
    pending.expressions.extend([4, 1]);
    let expressions = pending.expressions.drain_sorted();
    assert_eq!(expressions, vec![1, 4]);
    assert!(expressions.capacity() >= expression_capacity);
}

#[test]
fn dense_flag_set_tracks_membership_and_rejects_invalid_ids() {
    let mut flags = DenseFlagSet::with_len(4);
    assert_eq!(flags.len(), 0);
    assert!(!flags.contains(&0));

    assert!(flags.insert(2));
    assert!(flags.contains(&2));
    assert_eq!(flags.len(), 1);
    assert!(!flags.insert(2));
    assert_eq!(flags.len(), 1);

    assert!(!flags.insert(4));
    assert!(!flags.contains(&4));
    assert!(!flags.remove(&4));
    assert_eq!(flags.flags.len(), 4);
    assert_eq!(flags.len(), 1);

    assert!(flags.remove(&2));
    assert!(!flags.contains(&2));
    assert_eq!(flags.len(), 0);
    assert!(!flags.remove(&2));
    assert!(flags.insert(2));
    assert_eq!(flags.len(), 1);
}

#[test]
fn ordered_checked_diagnostic_projection_matches_recursive_oracle() {
    fn project(
        parsed: &ParsedProgram,
        ownership: CheckOutputOwnership,
        mode: CheckedDiagnosticProjectionMode,
    ) -> CheckOutput {
        let (mut checker, profile) = CheckedProgramDatabase::new_profiled(parsed);
        checker.checked_diagnostic_projection_mode = mode;
        checker.finish_program_profiled(ownership, profile).0
    }

    let fixtures = [
        (
            "projection-duplicate-map.bn",
            r#"
users: MAP {
    1 => TEXT { first }
    1.0 => TEXT { second }
}
"#,
        ),
        (
            "projection-deferred-style.bn",
            r#"
FUNCTION sized_box(width) {
    Element/container(
        element: []
        style: [width: width]
        child: Element/label(
            element: []
            style: []
            label: TEXT { child }
        )
    )
}

document: Document/new(root: sized_box(width: TEXT { invalid }))
"#,
        ),
        (
            "projection-mixed-errors.bn",
            r#"
items: LIST { 1 2 }
bad_number: TEXT { no } + 1
bad_set: SET { items }
state:
    [items: items]
    |> HOLD state {}
"#,
        ),
        (
            "projection-user-call-errors.bn",
            r#"
FUNCTION identity(value) {
    value
}

bad_direct: identity(wrong: 1)
bad_pipe: 1 |> identity(wrong: 2)
"#,
        ),
    ];
    let ownerships = [
        CheckOutputOwnership::ReportOwned,
        CheckOutputOwnership::EditorOwned,
        CheckOutputOwnership::DiagnosticsOwned,
        CheckOutputOwnership::RuntimeOwned,
    ];

    for (path, source) in fixtures {
        let parsed = boon_parser::parse_source(path, source).expect("projection fixture parses");
        for ownership in ownerships {
            let ordered = project(&parsed, ownership, CheckedDiagnosticProjectionMode::Ordered);
            let oracle = project(
                &parsed,
                ownership,
                CheckedDiagnosticProjectionMode::RecursiveOracle,
            );
            assert_eq!(
                ordered, oracle,
                "ordered projection diverged for {path} in {ownership:?}"
            );
            if ownership == CheckOutputOwnership::DiagnosticsOwned {
                let (lean_checker, lean_profile) =
                    CheckedProgramDatabase::new_diagnostics_profiled(&parsed);
                let lean = lean_checker
                    .finish_program_profiled(CheckOutputOwnership::DiagnosticsOwned, lean_profile)
                    .0;
                assert_eq!(
                    ordered, lean,
                    "checked-only diagnostic initialization diverged for {path}"
                );
            }
        }
    }
}

#[test]
#[ignore = "product-scale checked diagnostic projection parity gate"]
fn ordered_checked_diagnostic_projection_matches_product_examples() {
    fn project(parsed: &ParsedProgram, mode: CheckedDiagnosticProjectionMode) -> CheckOutput {
        let (mut checker, profile) = CheckedProgramDatabase::new_profiled(parsed);
        checker.checked_diagnostic_projection_mode = mode;
        checker
            .finish_program_profiled(CheckOutputOwnership::ReportOwned, profile)
            .0
    }

    fn project_diagnostics(parsed: &ParsedProgram, legacy_bindings: bool) -> CheckOutput {
        let (checker, profile) = if legacy_bindings {
            CheckedProgramDatabase::new_profiled(parsed)
        } else {
            CheckedProgramDatabase::new_diagnostics_profiled(parsed)
        };
        checker
            .finish_program_profiled(CheckOutputOwnership::DiagnosticsOwned, profile)
            .0
    }

    fn project_units(root: &std::path::Path) -> Vec<(String, String)> {
        fn collect(
            root: &std::path::Path,
            path: &std::path::Path,
            output: &mut Vec<(String, String)>,
        ) {
            for entry in std::fs::read_dir(path).expect("example directory is readable") {
                let entry = entry.expect("example directory entry is readable");
                let path = entry.path();
                if path.is_dir() {
                    collect(root, &path, output);
                    continue;
                }
                if path.extension().and_then(|extension| extension.to_str()) != Some("bn")
                    || path.file_name().and_then(|name| name.to_str()) == Some("BUILD.bn")
                {
                    continue;
                }
                let relative = path
                    .strip_prefix(root)
                    .expect("example unit remains under its root")
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                output.push((
                    relative,
                    std::fs::read_to_string(&path).expect("example unit is readable"),
                ));
            }
        }

        let mut output = Vec::new();
        collect(root, root, &mut output);
        output.sort_by(|left, right| left.0.cmp(&right.0));
        output
    }

    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let counter = boon_parser::parse_source(
        "counter.bn",
        &std::fs::read_to_string(examples.join("counter.bn")).expect("Counter source is readable"),
    )
    .expect("Counter parses");
    let todo =
        boon_parser::parse_project("RUN.bn", project_units(&examples.join("todo_mvc_physical")))
            .expect("physical TodoMVC parses");
    let novywave = boon_parser::parse_project("RUN.bn", project_units(&examples.join("novywave")))
        .expect("NovyWave parses");

    for (name, parsed) in [
        ("Counter", counter),
        ("physical TodoMVC", todo),
        ("NovyWave", novywave),
    ] {
        let ordered = project(&parsed, CheckedDiagnosticProjectionMode::Ordered);
        let oracle = project(&parsed, CheckedDiagnosticProjectionMode::RecursiveOracle);
        if ordered != oracle {
            fn first_difference(
                path: &mut String,
                left: &serde_json::Value,
                right: &serde_json::Value,
            ) -> Option<(String, serde_json::Value, serde_json::Value)> {
                match (left, right) {
                    (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
                        for (index, (left, right)) in left.iter().zip(right).enumerate() {
                            let previous_len = path.len();
                            path.push_str(&format!("[{index}]"));
                            if let Some(difference) = first_difference(path, left, right) {
                                return Some(difference);
                            }
                            path.truncate(previous_len);
                        }
                        (left.len() != right.len()).then(|| {
                            (
                                format!("{path}.length"),
                                left.len().into(),
                                right.len().into(),
                            )
                        })
                    }
                    (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
                        let mut keys = left.keys().chain(right.keys()).collect::<Vec<_>>();
                        keys.sort_unstable();
                        keys.dedup();
                        for key in keys {
                            let previous_len = path.len();
                            path.push('.');
                            path.push_str(key);
                            let difference = match (left.get(key), right.get(key)) {
                                (Some(left), Some(right)) => first_difference(path, left, right),
                                (left, right) => Some((
                                    path.clone(),
                                    left.cloned().unwrap_or(serde_json::Value::Null),
                                    right.cloned().unwrap_or(serde_json::Value::Null),
                                )),
                            };
                            if difference.is_some() {
                                return difference;
                            }
                            path.truncate(previous_len);
                        }
                        None
                    }
                    _ if left == right => None,
                    _ => Some((path.clone(), left.clone(), right.clone())),
                }
            }

            let ordered_json = serde_json::to_value((&ordered.program, &ordered.report))
                .expect("ordered projection serializes");
            let oracle_json = serde_json::to_value((&oracle.program, &oracle.report))
                .expect("recursive projection serializes");
            let (path, ordered_value, oracle_value) =
                first_difference(&mut "$".to_owned(), &ordered_json, &oracle_json)
                    .expect("unequal projections have a serialized difference");
            panic!(
                "ordered projection diverged for {name} at {path}: ordered={ordered_value} oracle={oracle_value}"
            );
        }
        let eager = project_diagnostics(&parsed, true);
        let lean = project_diagnostics(&parsed, false);
        if eager != lean {
            let eager_json = serde_json::to_value((&eager.program, &eager.report))
                .expect("eager diagnostics serialize");
            let lean_json = serde_json::to_value((&lean.program, &lean.report))
                .expect("checked-only diagnostics serialize");
            let (path, eager_value, lean_value) =
                first_json_difference(&mut "$".to_owned(), &eager_json, &lean_json)
                    .expect("unequal diagnostics have a serialized difference");
            panic!(
                "checked-only diagnostic initialization diverged for {name} at {path}: eager={eager_value} checked_only={lean_value}"
            );
        }
    }
}

#[test]
fn dense_index_table_bounded_insert_does_not_expand_for_invalid_expression_ids() {
    let mut table = DenseIndexTable::with_len(2);
    assert!(table.insert_if_in_bounds(1, TypeVar(7)));
    assert_eq!(table.get(&1), Some(&TypeVar(7)));

    assert!(!table.insert_if_in_bounds(10, TypeVar(8)));
    assert_eq!(table.entries.len(), 2);
    assert_eq!(table.get(&10), None);
}

#[test]
fn packed_adjacency_sorts_deduplicates_and_rejects_invalid_rows() {
    let mut builder = PackedAdjacencyBuilder::with_rows(4);
    assert!(builder.push(2, 9usize));
    assert!(builder.push(0, 4));
    assert!(builder.push(2, 3));
    assert!(builder.push(2, 9));
    assert!(builder.extend(3, [8, 1, 8]));
    assert!(!builder.push(4, 99));
    assert!(!builder.extend(usize::MAX, [100]));

    let adjacency = builder.finish_sorted_unique();
    assert_eq!(adjacency.len(), 4);
    assert_eq!(adjacency.edge_count(), 5);
    assert_eq!(adjacency.get(0), Some(&[4][..]));
    assert_eq!(adjacency.get(1), Some(&[][..]));
    assert_eq!(adjacency.get(2), Some(&[3, 9][..]));
    assert_eq!(adjacency.get(3), Some(&[1, 8][..]));
    assert_eq!(adjacency.get(4), None);
    assert_eq!(
        adjacency.iter().collect::<Vec<_>>(),
        vec![&[4][..], &[][..], &[3, 9][..], &[1, 8][..]]
    );
}

#[test]
fn exact_statement_root_index_matches_first_depth_first_statement() {
    fn statement(id: usize, expr: Option<usize>, children: Vec<AstStatement>) -> AstStatement {
        AstStatement {
            id,
            line: 1,
            indent: 0,
            start: 0,
            end: 0,
            kind: AstStatementKind::Expression,
            expr,
            children,
        }
    }

    let statements = vec![
        statement(
            10,
            Some(2),
            vec![
                statement(11, Some(2), Vec::new()),
                statement(12, Some(1), Vec::new()),
            ],
        ),
        statement(13, Some(1), Vec::new()),
        statement(14, Some(4), Vec::new()),
    ];
    let index = exact_statement_by_root_expression(&statements, 3);

    for expr_id in 0..3 {
        assert_eq!(
            index.get(&expr_id).copied(),
            exact_expression_statement(&statements, expr_id).map(|statement| statement.id)
        );
    }
    assert_eq!(index.entries.len(), 3);
    assert_eq!(index.get(&4), None);
}

#[test]
fn known_statement_index_preserves_nested_and_following_pipeline_values() {
    fn expression(id: usize, kind: AstExprKind) -> AstExpr {
        AstExpr {
            id,
            line: 1,
            start: 0,
            end: 0,
            linked_input: None,
            kind,
        }
    }

    fn statement(id: usize, expr: usize, children: Vec<AstStatement>) -> AstStatement {
        AstStatement {
            id,
            line: 1,
            indent: 0,
            start: 0,
            end: 0,
            kind: AstStatementKind::Expression,
            expr: Some(expr),
            children,
        }
    }

    let expressions = vec![
        expression(0, AstExprKind::Identifier("input".to_owned())),
        expression(1, AstExprKind::Delimiter),
        expression(
            2,
            AstExprKind::Pipe {
                input: 1,
                op: "Number/abs".to_owned(),
                args: Vec::new(),
                pass: None,
                arms: Vec::new(),
            },
        ),
        expression(3, AstExprKind::Delimiter),
        expression(
            4,
            AstExprKind::Pipe {
                input: 3,
                op: "Number/floor".to_owned(),
                args: Vec::new(),
                pass: None,
                arms: Vec::new(),
            },
        ),
    ];
    let nested = vec![statement(
        0,
        0,
        vec![statement(1, 2, vec![statement(2, 4, Vec::new())])],
    )];
    let following = vec![
        statement(3, 0, Vec::new()),
        statement(4, 2, Vec::new()),
        statement(5, 4, Vec::new()),
    ];
    let direct = vec![statement(6, 0, Vec::new())];

    for statements in [&nested, &following, &direct] {
        for (statement_index, statement) in statements.iter().enumerate() {
            assert_eq!(
                canonical_statement_value_expression_at_known_index(
                    statements,
                    statement_index,
                    &expressions,
                ),
                canonical_statement_value_expression(statements, statement, &expressions)
            );
        }
    }
    assert_eq!(
        canonical_block_value_expression(&nested, &expressions),
        Some(4)
    );
    assert_eq!(
        canonical_block_value_expression(&following, &expressions),
        Some(4)
    );
    assert_eq!(
        canonical_block_value_expression(&direct, &expressions),
        Some(0)
    );
}

#[test]
fn suffix_indexes_match_exact_legacy_lookup_and_ambiguity_rules() {
    let exact_declarations = BTreeMap::from([
        ("alpha.rows".to_owned(), 1usize),
        ("beta.rows".to_owned(), 1usize),
        ("alpha.unique.value".to_owned(), 3usize),
        ("rows".to_owned(), 4usize),
    ]);
    let declaration_oracle = |path: &str| {
        exact_declarations.get(path).copied().or_else(|| {
            let suffix = format!(".{path}");
            let mut matches = exact_declarations
                .iter()
                .filter(|(candidate, _)| candidate.ends_with(&suffix))
                .map(|(_, expression)| *expression);
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        })
    };
    let declarations = DeclarationExprIndex::from_exact(exact_declarations.clone());
    for path in ["rows", "unique.value", "value", "missing"] {
        assert_eq!(declarations.resolve(path), declaration_oracle(path));
    }
    assert_eq!(declarations.resolve("rows"), Some(4));
    let ambiguous_same_expression = DeclarationExprIndex::from_exact(BTreeMap::from([
        ("alpha.rows".to_owned(), 1usize),
        ("beta.rows".to_owned(), 1usize),
    ]));
    assert_eq!(ambiguous_same_expression.resolve("rows"), None);

    let mut modes = FlowModeIndex::default();
    modes.insert("alpha.event".to_owned(), FlowMode::Continuous);
    modes.insert("beta.event".to_owned(), FlowMode::PresentOrAbsent);
    modes.insert("alpha.stable".to_owned(), FlowMode::Continuous);
    modes.insert("event".to_owned(), FlowMode::TickPresent);
    let flow_oracle = |path: &str| {
        modes.exact.get(path).copied().or_else(|| {
            let suffix = format!(".{path}");
            modes
                .exact
                .iter()
                .filter(|(candidate, _)| candidate.ends_with(&suffix))
                .map(|(_, mode)| *mode)
                .reduce(merge_flow_modes)
        })
    };
    for path in ["event", "stable", "missing"] {
        assert_eq!(modes.resolve(path), flow_oracle(path));
    }
    assert_eq!(modes.resolve("event"), Some(FlowMode::TickPresent));
    let subtree_oracle = |path: &str| {
        let descendant_prefix = format!("{path}.");
        modes
            .exact
            .iter()
            .filter(|(candidate, _)| {
                candidate.as_str() == path || candidate.starts_with(&descendant_prefix)
            })
            .map(|(_, mode)| *mode)
            .reduce(merge_flow_modes)
    };
    for path in ["alpha", "beta", "event", "missing"] {
        assert_eq!(modes.subtree_mode(path), subtree_oracle(path));
    }
}

#[test]
fn source_path_index_preserves_longest_prefix_and_ambiguity_rules() {
    let source = |id: u32, anchor: u32, projection: &[&str]| CheckedSource {
        id: CheckedSourceId(id),
        declaration: DeclId(anchor),
        statement: CheckedStatementId(id),
        expression: CheckedExprId(id),
        owner_scope: LexicalScopeId(0),
        path: CheckedSemanticPath {
            anchor: DeclId(anchor),
            projection: projection.iter().map(|part| (*part).to_owned()).collect(),
        },
        interval_ms: None,
        payload_type: Type::Unknown,
        span: CheckedSpan::default(),
    };
    let sources = vec![
        source(0, 7, &["root"]),
        source(1, 7, &["root", "nested"]),
        source(2, 8, &["root", "nested"]),
    ];
    let index = CheckedSourcePathIndex::new(&sources);
    assert_eq!(
        index.exact_read(
            DeclId(7),
            &["root".to_owned(), "nested".to_owned(), "value".to_owned()]
        ),
        Some(CheckedSourceRead {
            source: CheckedSourceId(1),
            payload_projection: vec!["value".to_owned()],
        })
    );
    assert_eq!(
        index.exact_read(DeclId(7), &["root".to_owned(), "other".to_owned()]),
        Some(CheckedSourceRead {
            source: CheckedSourceId(0),
            payload_projection: vec!["other".to_owned()],
        })
    );
    assert_eq!(index.exact_read(DeclId(99), &["root".to_owned()]), None);

    let ambiguous_sources = vec![
        source(3, 9, &["same"]),
        source(4, 9, &["same"]),
        source(5, 9, &[]),
    ];
    let ambiguous = CheckedSourcePathIndex::new(&ambiguous_sources);
    assert_eq!(
        ambiguous.exact_read(DeclId(9), &["same".to_owned(), "value".to_owned()]),
        None
    );
}

#[test]
fn flow_mode_subtree_index_matches_boundary_aware_legacy_scan() {
    let entries = vec![
        ("a".to_owned(), FlowMode::Continuous),
        ("a.b".to_owned(), FlowMode::TickPresent),
        ("a.b".to_owned(), FlowMode::PresentOrAbsent),
        ("a.bc".to_owned(), FlowMode::Absent),
        ("other.a".to_owned(), FlowMode::TickPresent),
    ];
    let mut index = FlowModeIndex::default();
    for (path, mode) in &entries {
        index.insert(path.clone(), *mode);
    }
    for target in ["a", "a.b", "a.bc", "other", "missing"] {
        let descendant_prefix = format!("{target}.");
        let legacy = entries
            .iter()
            .filter(|(path, _)| path == target || path.starts_with(&descendant_prefix))
            .map(|(_, mode)| *mode)
            .reduce(merge_flow_modes);
        assert_eq!(index.subtree_mode(target), legacy, "subtree {target}");
    }
}

fn provisional_flow_mode_visits_for_pipeline(pipe_count: usize) -> (usize, usize) {
    let mut source = "value: True".to_owned();
    for _ in 0..pipe_count {
        source.push_str("\n    |> Bool/not()");
    }
    source.push('\n');
    let parsed = boon_parser::parse_source("flow-mode-linear.bn", &source)
        .expect("generated boolean pipeline parses");
    let expression_count = parsed.expressions.len();
    let (mut checker, _) = CheckedProgramDatabase::new_profiled(&parsed);
    for statement in &parsed.ast.statements {
        checker.check_statement(&parsed, statement, false);
    }
    for expression in &parsed.expressions {
        checker.ensure_expr(expression.id);
    }
    (checker.flow_mode_expression_visits.get(), expression_count)
}

#[test]
fn provisional_flow_mode_walk_is_linear_for_linked_pipelines() {
    let (short_visits, short_expressions) = provisional_flow_mode_visits_for_pipeline(64);
    let (long_visits, long_expressions) = provisional_flow_mode_visits_for_pipeline(128);

    assert!(short_visits <= short_expressions.saturating_mul(2));
    assert!(long_visits <= long_expressions.saturating_mul(2));
    assert!(
        long_visits <= short_visits.saturating_mul(2).saturating_add(8),
        "doubling a linked pipeline must not re-walk every checked prefix: short={short_visits}, long={long_visits}"
    );
}

#[test]
fn parser_pipeline_edges_match_the_structural_reference_in_nested_state() {
    let parsed = boon_parser::parse_source(
        "pipeline-edge-parity.bn",
        r#"
value: fibonacci(position: 10)

FUNCTION fibonacci(position) {
    position
    |> THEN {
        position |> WHILE {
            1 => 1
            n =>
                [previous: 0, current: 1]
                |> HOLD state {
                    n - 1
                    |> Stream/pulses()
                    |> THEN { state.current }
                }
                |> Stream/skip(count: n - 1)
                |> .current
        }
    }
}
"#,
    )
    .expect("nested pipeline parity fixture parses");

    for expression in &parsed.expressions {
        let Some(raw_input) = checked_pipeline_raw_input(expression) else {
            continue;
        };
        let expected = if parsed
            .expressions
            .get(raw_input)
            .is_some_and(expr_is_pipe_placeholder)
        {
            previous_pipeline_expr_id(&parsed.ast.statements, expression.id, &parsed.expressions)
                .unwrap_or(raw_input)
        } else {
            raw_input
        };
        assert_eq!(
            pipeline_source_expr_id(
                &parsed.ast.statements,
                expression.id,
                raw_input,
                &parsed.expressions,
            ),
            expected,
            "pipeline predecessor mismatch for expression {} ({:?})",
            expression.id,
            expression.kind,
        );
    }
}

#[test]
fn indexed_projection_validation_preserves_scalar_diagnostics() {
    let parsed = boon_parser::parse_source(
        "indexed-projection-validation.bn",
        r#"
number: 1
bad_path: number.missing
bad_pipe:
    number
    |> .missing
record: [scalar: 2]
bad_nested: record.scalar.missing
"#,
    )
    .expect("projection validation fixture parses");
    let output = check_program(&parsed);
    let messages = output
        .report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.starts_with("cannot project field"))
        .map(|diagnostic| (diagnostic.line, diagnostic.message.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        vec![
            (3, "cannot project field `missing` from NUMBER"),
            (6, "cannot project field `missing` from NUMBER"),
            (8, "cannot project field `missing` from NUMBER"),
        ]
    );
}

#[test]
fn contextual_scheme_worklist_preserves_inherited_and_explicit_pass_edges() {
    let parsed = boon_parser::parse_source(
        "context-scheme-worklist.bn",
        r#"
FUNCTION leaf() {
    PASSED.store.count
}

FUNCTION inherited() {
    leaf()
}

FUNCTION explicit(store) {
    leaf(PASS: [store: store])
}

inherited_value: inherited(PASS: [store: [count: 1]])
explicit_value: explicit(store: [count: 2])
"#,
    )
    .expect("context worklist fixture parses");
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "context worklist fixture diagnostics: {:?}",
        output.report.diagnostics
    );
    let program = output.program.expect("context fixture checks");
    let callable = |name: &str| {
        program
            .callables
            .iter()
            .find(|callable| callable.name == name)
            .unwrap_or_else(|| panic!("missing callable {name}"))
    };
    assert!(callable("leaf").context_formal.is_some());
    assert!(callable("inherited").context_formal.is_some());
    assert!(callable("explicit").context_formal.is_none());
    assert!(program.calls.iter().any(|call| {
        call.owner_callable == Some(callable("inherited").decl_id)
            && matches!(
                call.context_binding,
                CheckedContextBinding::Inherited { .. }
            )
    }));
    assert!(program.calls.iter().any(|call| {
        call.owner_callable == Some(callable("explicit").decl_id)
            && matches!(call.context_binding, CheckedContextBinding::Explicit { .. })
    }));
}

#[test]
fn profiled_typecheck_work_counters_are_deterministic_and_fully_accounted() {
    let parsed = boon_parser::parse_source(
        "profiled-work-counters.bn",
        r#"
FUNCTION leaf() {
    PASSED.store.count
}

FUNCTION wrapper() {
    leaf()
}

value: wrapper(PASS: [store: [count: 1]])
"#,
    )
    .expect("profiled work-counter fixture parses");
    let run = || check_diagnostics_program_profiled(&parsed).1.work_counters;

    let baseline = run();
    let repeated = run();
    assert_eq!(repeated, baseline, "timer-free work must be repeatable");
    assert!(baseline.inference_invocations > 0);
    assert!(baseline.inference_rounds > 0);
    assert!(baseline.inference_call_visits > 0);
    assert!(baseline.context_scheme_worklist_invocations > 0);
    assert!(baseline.wrapper_scheme_worklist_invocations > 0);
    assert!(
        baseline
            .checked_flow_cache_hits
            .saturating_add(baseline.checked_flow_cache_misses)
            > 0
    );
    assert!(baseline.diagnostic_replay_requests > 0);
    assert!(baseline.inference_call_visits_are_fully_classified());
    assert!(baseline.diagnostic_replay_is_fully_accounted());
}

#[test]
fn nested_user_function_body_cache_reuses_equal_argument_shapes() {
    let parsed = boon_parser::parse_source(
        "nested-call-cache.bn",
        r#"
FUNCTION identity(value) {
    value
}

FUNCTION first(item) {
    identity(value: item)
}

FUNCTION second(item) {
    identity(value: item)
}
"#,
    )
    .expect("nested cache fixture parses");
    let identity_calls = parsed
        .expressions
        .iter()
        .filter(|expression| {
            matches!(
                &expression.kind,
                AstExprKind::Call { function, .. } if function == "identity"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(identity_calls.len(), 2);

    let (checker, _) = CheckedProgramDatabase::new_profiled(&parsed);
    checker.function_call_return_type_cache_misses.set(0);
    let equal_shape = || {
        Type::object(ObjectShape {
            fields: BTreeMap::from([("label".to_owned(), Type::Text)]),
            field_order: vec!["label".to_owned()],
            open: false,
        })
    };
    let infer = |expression: &AstExpr, argument_type: Type| {
        checker.static_user_call_type_with_bindings(
            expression,
            &mut BTreeSet::new(),
            &BTreeMap::from([("item".to_owned(), argument_type)]),
        )
    };

    assert_eq!(infer(identity_calls[0], equal_shape()), Some(equal_shape()));
    assert_eq!(infer(identity_calls[1], equal_shape()), Some(equal_shape()));
    assert_eq!(checker.function_call_return_type_cache_misses.get(), 1);

    assert_eq!(infer(identity_calls[0], Type::Number), Some(Type::Number));
    assert_eq!(checker.function_call_return_type_cache_misses.get(), 2);
}

#[test]
fn dense_recursion_guard_reuses_storage_and_forgets_the_previous_root_generation() {
    let mut active = DenseRecursionGuard::with_len(3);
    active.begin_root();
    assert!(active.insert(1));
    assert!(!active.insert(1));
    assert!(!active.insert(3));
    assert_eq!(active.generations.len(), 3);
    assert_eq!(active.len(), 1);

    // A new root ignores stale membership without clearing or reallocating the
    // dense arena. Ordinary recursion still removes membership eagerly.
    active.begin_root();
    assert_eq!(active.len(), 0);
    assert!(active.insert(1));
    assert!(active.remove(&1));
    assert_eq!(active.len(), 0);
}

#[test]
fn checked_flow_cache_reuses_unaffected_entries_and_invalidates_only_reverse_closure() {
    let flow = |ty| FlowType {
        mode: FlowMode::Continuous,
        ty,
    };
    let mut cache = CheckedFlowInferenceCache::with_len(4);
    for (expression, ty) in [
        (0, Type::Text),
        (1, Type::Number),
        (2, Type::Bits { width: 8 }),
        (3, Type::Bytes(BytesType::Dynamic)),
    ] {
        assert!(cache.insert(expression, flow(ty)));
    }
    assert_eq!(
        cache.get_cloned(3),
        Some(flow(Type::Bytes(BytesType::Dynamic)))
    );

    let dependents = vec![vec![1], vec![2], Vec::new(), Vec::new()];
    let mut seen = DenseGenerationSet::with_len(4);
    invalidate_checked_flow_cache_reverse_closure(&mut cache, &dependents, [0, 1], &mut seen);

    assert_eq!(cache.entries.get(&0), None);
    assert_eq!(cache.entries.get(&1), None);
    assert_eq!(cache.entries.get(&2), None);
    assert_eq!(
        cache.get_cloned(3),
        Some(flow(Type::Bytes(BytesType::Dynamic)))
    );
    assert_eq!(cache.stats.invalidations, 3);
    assert_eq!(cache.stats.reverse_invalidation_traversals, 1);
    assert_eq!(cache.stats.hits, 2);

    invalidate_checked_flow_cache_reverse_closure(
        &mut cache,
        &dependents,
        std::iter::empty(),
        &mut seen,
    );
    assert_eq!(cache.stats.reverse_invalidation_traversals, 1);

    assert!(!cache.insert(9, flow(Type::Unknown)));
    assert_eq!(cache.entries.entries.len(), 4);
    assert_eq!(cache.stats.rejected_invalid_ids, 1);

    invalidate_checked_flow_cache_reverse_closure(&mut cache, &dependents, [9], &mut seen);
    assert_eq!(cache.entries.entries.len(), 4);
    assert_eq!(cache.stats.rejected_invalid_ids, 2);
    assert_eq!(cache.stats.reverse_invalidation_traversals, 2);

    cache.reset_for_new_topology();
    assert_eq!(cache.entries.entries.len(), 4);
    assert!(cache.entries.values().next().is_none());
    assert_eq!(cache.stats.full_resets, 1);
}

#[test]
fn checked_flow_declaration_invalidation_follows_forwarded_outputs_without_cycles() {
    let mut readers = PackedAdjacencyBuilder::with_rows(6);
    readers.push(2, 7);
    readers.push(3, 9);
    readers.extend(4, [11, 7]);
    let readers = readers.finish_sorted_unique();
    let mut dependents = PackedAdjacencyBuilder::with_rows(6);
    dependents.push(1, DeclId(2));
    dependents.push(2, DeclId(3));
    dependents.push(3, DeclId(4));
    dependents.push(4, DeclId(2));
    let dependents = dependents.finish_sorted_unique();
    let mut seen = DenseGenerationSet::with_len(5);

    assert_eq!(
        checked_flow_declaration_invalidation_roots(&readers, &dependents, DeclId(1), &mut seen,),
        vec![7, 9, 11]
    );
    assert_eq!(
        checked_flow_declarations_invalidation_roots(
            &readers,
            &dependents,
            [DeclId(1), DeclId(2), DeclId(1)],
            &mut seen,
        ),
        vec![7, 9, 11]
    );
    assert!(
        checked_flow_declaration_invalidation_roots(&readers, &dependents, DeclId(5), &mut seen,)
            .is_empty()
    );
}

#[test]
fn cloned_object_and_tagged_types_share_their_sealed_shapes() {
    let shape = ObjectShape {
        fields: BTreeMap::from([
            (
                "nested".to_owned(),
                Type::object(ObjectShape {
                    fields: BTreeMap::from([("value".to_owned(), Type::Text)]),
                    field_order: vec!["value".to_owned()],
                    open: false,
                }),
            ),
            ("count".to_owned(), Type::Number),
        ]),
        field_order: vec!["count".to_owned(), "nested".to_owned()],
        open: false,
    };

    let object = Type::object(shape.clone());
    let object_clone = object.clone();
    let (Type::Object(object_shape), Type::Object(cloned_object_shape)) = (&object, &object_clone)
    else {
        unreachable!("fixture is an object type")
    };
    assert!(SharedObjectShape::ptr_eq(object_shape, cloned_object_shape));

    let tagged = Variant::tagged("Found".to_owned(), shape);
    let tagged_clone = tagged.clone();
    let (
        Variant::Tagged {
            fields: tagged_shape,
            ..
        },
        Variant::Tagged {
            fields: cloned_tagged_shape,
            ..
        },
    ) = (&tagged, &tagged_clone)
    else {
        unreachable!("fixture is a tagged variant")
    };
    assert!(SharedObjectShape::ptr_eq(tagged_shape, cloned_tagged_shape));

    let variants = Type::VariantSet(vec![tagged].into());
    let variants_clone = variants.clone();
    let (Type::VariantSet(variants), Type::VariantSet(cloned_variants)) =
        (&variants, &variants_clone)
    else {
        unreachable!("fixture is a variant-set type")
    };
    assert!(SharedVariantSet::ptr_eq(variants, cloned_variants));
}

#[test]
fn shared_object_shape_preserves_object_and_tagged_json() {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let shape = ObjectShape {
        fields: BTreeMap::from([
            ("zeta".to_owned(), Type::Number),
            ("alpha".to_owned(), Type::Text),
        ]),
        field_order: vec!["zeta".to_owned(), "alpha".to_owned()],
        open: true,
    };
    let shape_json = serde_json::to_value(&shape).expect("owned shape serializes");
    let shared = SharedObjectShape::new(shape.clone());
    let separately_sealed = SharedObjectShape::new(shape.clone());
    assert!(!SharedObjectShape::ptr_eq(&shared, &separately_sealed));
    assert_eq!(shared, separately_sealed);
    assert_eq!(shared.field_order, shape.field_order);
    let hash = |value: &SharedObjectShape| {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(hash(&shared), hash(&separately_sealed));
    assert_eq!(
        serde_json::to_value(&shared).expect("shared shape serializes"),
        shape_json
    );

    let object = Type::object(shape.clone());
    assert_eq!(
        serde_json::to_value(&object).expect("object type serializes"),
        serde_json::json!({ "Object": shape_json.clone() })
    );
    let decoded_object: Type = serde_json::from_value(
        serde_json::to_value(&object).expect("object type serializes for round trip"),
    )
    .expect("object type deserializes");
    assert_eq!(decoded_object, object);

    let variants = Type::VariantSet(vec![Variant::tagged("Found".to_owned(), shape)].into());
    assert_eq!(
        serde_json::to_value(&variants).expect("tagged type serializes"),
        serde_json::json!({
            "VariantSet": [{
                "Tagged": {
                    "tag": "Found",
                    "fields": shape_json
                }
            }]
        })
    );
    let decoded_variants: Type = serde_json::from_value(
        serde_json::to_value(&variants).expect("tagged type serializes for round trip"),
    )
    .expect("tagged type deserializes");
    assert_eq!(decoded_variants, variants);
}

#[test]
fn closed_type_transforms_reuse_object_and_tagged_shape_allocations() {
    let tagged = Type::VariantSet(
        vec![Variant::tagged(
            "Ready".to_owned(),
            ObjectShape {
                fields: BTreeMap::from([("label".to_owned(), Type::Text)]),
                field_order: vec!["label".to_owned()],
                open: false,
            },
        )]
        .into(),
    );
    let closed = Type::object(ObjectShape {
        fields: BTreeMap::from([("state".to_owned(), tagged)]),
        field_order: vec!["state".to_owned()],
        open: false,
    });
    let Type::Object(original_object) = &closed else {
        unreachable!("fixture is an object")
    };
    let Type::VariantSet(original_variants) = original_object
        .fields
        .get("state")
        .expect("state field exists")
    else {
        unreachable!("state is a variant set")
    };
    let Variant::Tagged {
        fields: original_tagged,
        ..
    } = &original_variants[0]
    else {
        unreachable!("state is tagged")
    };

    let mut call_vars = BTreeMap::new();
    let mut next_var = 100;
    let instantiated = instantiate_checked_type_scheme_for_call(
        &closed,
        CheckedCallId(9),
        &mut call_vars,
        &mut next_var,
    );
    let Type::Object(instantiated_object) = &instantiated else {
        unreachable!("instantiated fixture remains an object")
    };
    assert!(SharedObjectShape::ptr_eq(
        original_object,
        instantiated_object
    ));
    assert!(call_vars.is_empty());
    assert_eq!(next_var, 100);

    let irrelevant = BTreeMap::from([(TypeVar(404), Type::Number)]);
    let substituted = substitute_checked_type(&closed, &irrelevant);
    let Type::Object(substituted_object) = &substituted else {
        unreachable!("substituted fixture remains an object")
    };
    assert!(SharedObjectShape::ptr_eq(
        original_object,
        substituted_object
    ));
    let Type::VariantSet(substituted_variants) = substituted_object
        .fields
        .get("state")
        .expect("state field remains")
    else {
        unreachable!("state remains a variant set")
    };
    let Variant::Tagged {
        fields: substituted_tagged,
        ..
    } = &substituted_variants[0]
    else {
        unreachable!("state remains tagged")
    };
    assert!(SharedObjectShape::ptr_eq(
        original_tagged,
        substituted_tagged
    ));
}

#[test]
fn generic_type_transforms_rebuild_only_shapes_with_applicable_variables() {
    let variable = TypeVar(7);
    let closed_child = Type::object(ObjectShape {
        fields: BTreeMap::from([("label".to_owned(), Type::Text)]),
        field_order: vec!["label".to_owned()],
        open: false,
    });
    let generic = Type::object(ObjectShape {
        fields: BTreeMap::from([
            ("closed".to_owned(), closed_child),
            ("generic".to_owned(), Type::Var(variable)),
            (
                "tagged".to_owned(),
                Type::VariantSet(
                    vec![Variant::tagged(
                        "Some".to_owned(),
                        ObjectShape {
                            fields: BTreeMap::from([("value".to_owned(), Type::Var(variable))]),
                            field_order: vec!["value".to_owned()],
                            open: false,
                        },
                    )]
                    .into(),
                ),
            ),
        ]),
        field_order: vec![
            "closed".to_owned(),
            "generic".to_owned(),
            "tagged".to_owned(),
        ],
        open: false,
    });
    let Type::Object(original) = &generic else {
        unreachable!("fixture is an object")
    };
    let Type::Object(original_closed) = original.fields.get("closed").expect("closed field exists")
    else {
        unreachable!("closed field is an object")
    };
    let Type::VariantSet(original_variants) =
        original.fields.get("tagged").expect("tagged field exists")
    else {
        unreachable!("tagged field is a variant set")
    };
    let Variant::Tagged {
        fields: original_tagged,
        ..
    } = &original_variants[0]
    else {
        unreachable!("tagged field has a payload")
    };

    let substituted =
        substitute_checked_type(&generic, &BTreeMap::from([(variable, Type::Number)]));
    let Type::Object(substituted_shape) = &substituted else {
        unreachable!("substituted fixture remains an object")
    };
    assert!(!SharedObjectShape::ptr_eq(original, substituted_shape));
    assert_eq!(substituted_shape.fields.get("generic"), Some(&Type::Number));
    let Type::Object(substituted_closed) = substituted_shape
        .fields
        .get("closed")
        .expect("closed field remains")
    else {
        unreachable!("closed field remains an object")
    };
    assert!(SharedObjectShape::ptr_eq(
        original_closed,
        substituted_closed
    ));
    let Type::VariantSet(substituted_variants) = substituted_shape
        .fields
        .get("tagged")
        .expect("tagged field remains")
    else {
        unreachable!("tagged field remains a variant set")
    };
    let Variant::Tagged {
        fields: substituted_tagged,
        ..
    } = &substituted_variants[0]
    else {
        unreachable!("tagged field retains a payload")
    };
    assert!(!SharedObjectShape::ptr_eq(
        original_tagged,
        substituted_tagged
    ));
    assert_eq!(substituted_tagged.fields.get("value"), Some(&Type::Number));

    let mut call_vars = BTreeMap::new();
    let mut next_var = 100;
    let instantiated = instantiate_checked_type_scheme_for_call(
        &generic,
        CheckedCallId(10),
        &mut call_vars,
        &mut next_var,
    );
    let Type::Object(instantiated_shape) = &instantiated else {
        unreachable!("instantiated fixture remains an object")
    };
    assert!(!SharedObjectShape::ptr_eq(original, instantiated_shape));
    assert_eq!(
        instantiated_shape.fields.get("generic"),
        Some(&Type::Var(TypeVar(100)))
    );
    let Type::Object(instantiated_closed) = instantiated_shape
        .fields
        .get("closed")
        .expect("closed field remains")
    else {
        unreachable!("closed field remains an object")
    };
    assert!(SharedObjectShape::ptr_eq(
        original_closed,
        instantiated_closed
    ));
    assert_eq!(next_var, 101);
}

#[test]
fn runtime_owned_success_preserves_the_exact_checked_program() {
    let parsed = boon_parser::parse_source(
        "runtime-owned-success.bn",
        r#"
input: 41
output: increment(value: input)

FUNCTION increment(value) {
    value + 1
}
"#,
    )
    .expect("runtime ownership fixture parses");

    let editor = check_program(&parsed);
    let runtime = check_runtime_program_profiled(&parsed).0;
    assert!(
        !editor.report.has_errors(),
        "editor diagnostics: {:#?}",
        editor.report.diagnostics
    );
    assert_eq!(runtime.report.diagnostics, editor.report.diagnostics);
    assert_eq!(runtime.program.as_ref(), editor.program.as_ref());

    let checked = runtime.program.expect("valid runtime program");
    assert!(!checked.lowering_metadata.expr_type_table.entries.is_empty());
    assert!(
        !checked
            .lowering_metadata
            .function_type_table
            .entries
            .is_empty()
    );
    assert!(
        !checked
            .lowering_metadata
            .named_value_type_table
            .entries
            .is_empty()
    );
    assert!(runtime.report.expr_type_table.entries.is_empty());
    assert!(runtime.report.function_type_table.entries.is_empty());
    assert!(runtime.report.named_value_type_table.entries.is_empty());
    assert!(runtime.report.constraints.is_empty());
    assert_eq!(
        runtime.report.resolved_constant_table,
        ResolvedConstantTable::default()
    );
}

#[test]
fn nested_contextual_builtin_overlay_closes_selected_user_call_result() {
    let parsed = boon_parser::parse_source(
        "nested-contextual-builtin-overlay.bn",
        r#"
rows:
    LIST {
        [
            kind: VariableRow
            id: TEXT { signal-1 }
            segments: LIST {
                [
                    label: TEXT { high }
                    signal_id: TEXT { signal-1 }
                ]
            }
        ]
    }
    |> List/map(item, new: lane_row(row: item))

FUNCTION segment_rows(row) {
    row.segments
    |> List/map(item, new: [
        label: item.label
        lane_id: row.id
        signal_id: item.signal_id
    ])
}

FUNCTION variable_lane(row) {
    [
        kind: row.kind
        id: row.id
        segments: segment_rows(row: row)
    ]
}

FUNCTION group_lane(row) {
    [
        kind: row.kind
        id: row.id
        segments: segment_rows(row: row)
    ]
}

FUNCTION lane_row(row) {
    row.kind |> WHEN {
        VariableRow => variable_lane(row: row)
        __ => group_lane(row: row)
    }
}
"#,
    )
    .expect("nested contextual fixture parses");
    let checked = check_program(&parsed);
    assert!(
        !checked.report.has_errors(),
        "nested contextual fixture diagnostics: {:#?}",
        checked.report.diagnostics
    );
    let program = checked.program.expect("fixture has a checked program");
    let call = program
        .calls
        .iter()
        .find(|call| call.function == "lane_row")
        .expect("fixture has the outer lane_row occurrence");
    assert!(
        call.syntax_discriminated_result,
        "the VariableRow syntax must select the exact lane_row arm"
    );
    assert!(
        type_is_recursively_closed(&call.result.ty),
        "nested List/map outputs must be instantiated under the outer occurrence: {:#?}",
        call.result.ty
    );
}

#[test]
#[ignore = "large NovyWave checked-database determinism gate"]
fn novywave_checked_database_is_seed_free_and_deterministic() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/novywave");
    let units = [
        "hold.bn",
        "Bridge/NovyBridge.bn",
        "Generated/Assets.bn",
        "Generated/NovyReference.bn",
        "Model/NovyModel.bn",
        "Theme/NovyTheme.bn",
        "RUN.bn",
        "View/NovyView.bn",
    ]
    .into_iter()
    .map(|path| {
        (
            path.to_owned(),
            std::fs::read_to_string(root.join(path)).expect("NovyWave source unit"),
        )
    })
    .collect::<Vec<_>>();
    let parsed = boon_parser::parse_project("RUN.bn", units).expect("NovyWave project parses");
    let run = || {
        let (checker, init_profile) = CheckedProgramDatabase::new_profiled(&parsed);
        let (output, profile) =
            checker.finish_program_profiled(CheckOutputOwnership::DiagnosticsOwned, init_profile);
        (output, profile.work_counters)
    };
    let (baseline, baseline_work) = run();
    let (repeated, repeated_work) = run();
    assert!(
        !baseline.report.has_errors(),
        "seed-free NovyWave diagnostics: {:#?}",
        baseline.report.diagnostics
    );
    assert!(
        baseline.program.is_some(),
        "NovyWave must remain executable"
    );
    let checked = baseline.program.as_ref().expect("checked NovyWave");
    let selected_signal_defaults = checked
        .lowering_metadata
        .named_value_type_table
        .entries
        .iter()
        .find(|entry| entry.path == "store.selected_signal_defaults")
        .expect("selected_signal_defaults checked named value");
    let Type::List(item) = &selected_signal_defaults.flow_type.ty else {
        panic!(
            "selected_signal_defaults must remain a list: {:#?}",
            selected_signal_defaults.flow_type.ty
        );
    };
    let selected_item_actual = checked
        .calls
        .iter()
        .find(|call| call.function == "new_selected_visible_item")
        .and_then(|call| {
            call.entries.iter().find_map(|entry| match entry {
                CheckedCallEntry::Input { name, value, .. } if name == "row" => checked
                    .expressions
                    .get(value.0 as usize)
                    .map(|expression| &expression.flow_type.ty),
                _ => None,
            })
        });
    assert!(
        type_is_recursively_closed(item),
        "the concrete projected-discriminant map occurrence must seal its item:\nitem={item:#?}\nactual={selected_item_actual:#?}"
    );
    assert_eq!(
        repeated, baseline,
        "seed-free checked output must be stable"
    );
    assert_eq!(
        repeated_work, baseline_work,
        "NovyWave timer-free typecheck work must be stable"
    );
    assert!(baseline_work.inference_rounds > 0);
    assert!(baseline_work.inference_expression_visits > 0);
    assert!(baseline_work.diagnostic_replay_requests > 0);
    assert!(baseline_work.inference_call_visits_are_fully_classified());
    assert!(baseline_work.diagnostic_replay_is_fully_accounted());
}

#[test]
fn editor_owned_success_retains_hints_while_transferring_lowering_tables() {
    let parsed = boon_parser::parse_source(
        "editor-owned-success.bn",
        r#"
input: 41
output: increment(value: input)

FUNCTION increment(value) {
    value + 1
}
"#,
    )
    .expect("editor ownership fixture parses");

    let report_owned = check_program(&parsed);
    let editor_owned = check_editor_program_profiled(&parsed).0;
    assert_eq!(
        editor_owned.report.diagnostics,
        report_owned.report.diagnostics
    );
    assert_eq!(
        editor_owned.report.type_hint_table,
        report_owned.report.type_hint_table
    );
    assert_eq!(
        editor_owned.report.resolved_constant_table,
        report_owned.report.resolved_constant_table
    );
    assert_eq!(editor_owned.program.as_ref(), report_owned.program.as_ref());

    let checked = editor_owned.program.expect("valid editor program");
    assert!(!checked.lowering_metadata.expr_type_table.entries.is_empty());
    assert!(editor_owned.report.expr_type_table.entries.is_empty());
    assert!(editor_owned.report.function_type_table.entries.is_empty());
    assert!(
        editor_owned
            .report
            .named_value_type_table
            .entries
            .is_empty()
    );
    assert!(editor_owned.report.render_slot_table.slots.is_empty());
}

#[test]
fn diagnostics_owned_success_defers_but_exactly_reprojects_type_hints() {
    let parsed = boon_parser::parse_source(
        "diagnostics-owned-success.bn",
        r#"
input: 41
output: increment(value: input)

FUNCTION increment(value) {
    value + 1
}
"#,
    )
    .expect("diagnostics ownership fixture parses");

    let report_owned = check_program(&parsed);
    let diagnostics_owned = check_diagnostics_program_profiled(&parsed).0;
    assert_eq!(
        diagnostics_owned.report.diagnostics,
        report_owned.report.diagnostics
    );
    assert!(diagnostics_owned.report.type_hint_table.entries.is_empty());
    assert_eq!(
        project_type_hints(&parsed, &diagnostics_owned),
        report_owned.report.type_hint_table
    );
    assert_eq!(
        diagnostics_owned.program.as_ref(),
        report_owned.program.as_ref()
    );
}

#[test]
fn runtime_owned_error_path_preserves_complete_diagnostics() {
    let parsed = boon_parser::parse_source(
        "runtime-owned-errors.bn",
        r#"
too_wide: BITS[3] { 2u1000 }
unknown: missing_value
"#,
    )
    .expect("runtime diagnostic fixture parses");

    let editor = check_program(&parsed);
    let runtime = check_runtime_program_profiled(&parsed).0;
    assert!(editor.program.is_none());
    assert!(runtime.program.is_none());
    assert!(editor.report.has_errors());
    assert_eq!(runtime.report.diagnostics, editor.report.diagnostics);
    assert_eq!(
        runtime.report.render_slot_failure_count,
        editor.report.render_slot_failure_count
    );
}
