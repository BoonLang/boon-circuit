// Included by `../machine_plan_backend_tests.rs`; kept in the parent test module for private invariant access.

#[test]
fn cells_unscoped_record_literal_initial_rows_get_typed_field_ids() {
    let parsed = parse_cells_project_for_plan_test();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let defaults_id = debug_entry_id(&plan.debug_map.list_slots, "list", "cells_default_values");
    let list_slot = plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id.0 == defaults_id)
        .expect("Cells default values list slot should exist");
    assert_eq!(list_slot.initial_rows.len(), 5);
    let first_fields = &list_slot.initial_rows[0].fields;
    assert_eq!(first_fields.len(), 3);
    for field in first_fields {
        assert!(
            field.field_id.is_some(),
            "unscoped static list field `{}` should receive a typed synthetic field id",
            field.name
        );
    }
    let debug_labels = first_fields
        .iter()
        .map(|field| {
            let id = field.field_id.expect("field id checked above");
            plan.debug_map
                .fields
                .iter()
                .find(|entry| entry.id == format!("field:{}", id.0))
                .map(|entry| entry.label.clone())
                .expect("synthetic field id should be debuggable")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        debug_labels,
        vec![
            "cells_default_values.address".to_owned(),
            "cells_default_values.field".to_owned(),
            "cells_default_values.value".to_owned()
        ]
    );
    assert!(
        verify_plan(&plan)
            .unwrap()
            .checks
            .iter()
            .any(|check| check.id == "list-initial-row-fields-resolve" && check.pass),
        "Cells static list initial row refs should verify"
    );
}


#[test]
fn cells_range_list_preserves_typed_bounds() {
    let parsed = parse_cells_project_for_plan_test();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let cells_id = debug_entry_id(&plan.debug_map.list_slots, "list", "cells");
    let list_slot = plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id.0 == cells_id)
        .expect("Cells range list slot should exist");
    assert_eq!(list_slot.initializer_kind, ListInitializerKind::Range);
    assert_eq!(
        list_slot.range,
        Some(PlanRangeInitializer { from: 0, to: 2599 })
    );
    let cells_index_id = debug_entry_id(&plan.debug_map.fields, "field", "cells.index");
    let address_id = debug_entry_id(&plan.debug_map.fields, "field", "cell.address");
    let address_op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .find(|op| matches!(op.output, Some(ValueRef::Field(field_id)) if field_id.0 == address_id))
        .expect("cell.address should have a derived op");
    assert!(
        address_op
            .inputs
            .contains(&ValueRef::Field(FieldId(cells_index_id))),
        "cell.address should depend on the typed synthetic range row index"
    );
    assert!(
        matches!(
            &address_op.kind,
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::RowExpression {
                    expression: PlanRowExpression::TextConcat { .. }
                }),
                ..
            }
        ),
        "cell.address should lower to an executable generic row expression"
    );
    assert!(
        verify_plan(&plan)
            .unwrap()
            .checks
            .iter()
            .any(|check| check.id == "list-range-bounds-resolve" && check.pass),
        "Cells range bounds should verify"
    );
}


#[test]
fn cells_display_text_when_lowers_to_cpu_supported_row_select() {
    let parsed = parse_cells_project_for_plan_test();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

    let display_text_id = debug_entry_id(&plan.debug_map.fields, "field", "cell.display_text");
    let display_text = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            matches!(
                op.output,
                Some(ValueRef::Field(field_id)) if field_id.0 == display_text_id
            )
        })
        .expect("cell.display_text derived op should lower");

    assert!(
        matches!(
            &display_text.kind,
            PlanOpKind::DerivedValue {
                derived_kind: PlanDerivedKind::Pure,
                expression: Some(PlanDerivedExpression::RowExpression {
                    expression: PlanRowExpression::Select { .. }
                }),
                ..
            }
        ),
        "cell.display_text WHEN expression should lower as a generic row select"
    );
    assert!(
        cpu_plan_executor_supports_whole_plan_op(
            &plan.storage_layout.scalar_slots,
            &plan.storage_layout.list_slots,
            &plan.constants,
            display_text,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        ),
        "cell.display_text row select should be executable by the generic CPU PlanExecutor"
    );
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "Cells MachinePlan should be CPU-complete once row WHEN expressions lower generically"
    );
    assert_eq!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count,
        0
    );
}


#[test]
fn cells_formula_byte_scan_offsets_lower_as_numeric_infix() {
    fn expression_contains_function(expression: &PlanRowExpression, function_name: &str) -> bool {
        match expression {
            PlanRowExpression::TextToBytes { input, encoding } => {
                function_name == "Text/to_bytes"
                    || expression_contains_function(input, function_name)
                    || encoding.as_deref().is_some_and(|encoding| {
                        expression_contains_function(encoding, function_name)
                    })
            }
            PlanRowExpression::BytesToText { input, encoding } => {
                function_name == "Bytes/to_text"
                    || expression_contains_function(input, function_name)
                    || encoding.as_deref().is_some_and(|encoding| {
                        expression_contains_function(encoding, function_name)
                    })
            }
            PlanRowExpression::BytesToHex { input } => {
                function_name == "Bytes/to_hex"
                    || expression_contains_function(input, function_name)
            }
            PlanRowExpression::BytesToBase64 { input } => {
                function_name == "Bytes/to_base64"
                    || expression_contains_function(input, function_name)
            }
            PlanRowExpression::BytesFromHex { input } => {
                function_name == "Bytes/from_hex"
                    || expression_contains_function(input, function_name)
            }
            PlanRowExpression::BytesFromBase64 { input } => {
                function_name == "Bytes/from_base64"
                    || expression_contains_function(input, function_name)
            }
            PlanRowExpression::BytesFind { input, needle } => {
                function_name == "Bytes/find"
                    || expression_contains_function(input, function_name)
                    || expression_contains_function(needle, function_name)
            }
            PlanRowExpression::BytesStartsWith { input, prefix } => {
                function_name == "Bytes/starts_with"
                    || expression_contains_function(input, function_name)
                    || expression_contains_function(prefix, function_name)
            }
            PlanRowExpression::BytesEndsWith { input, suffix } => {
                function_name == "Bytes/ends_with"
                    || expression_contains_function(input, function_name)
                    || expression_contains_function(suffix, function_name)
            }
            PlanRowExpression::BytesIsEmpty { input } => {
                function_name == "Bytes/is_empty"
                    || expression_contains_function(input, function_name)
            }
            PlanRowExpression::BytesConcat { left, right } => {
                function_name == "Bytes/concat"
                    || expression_contains_function(left, function_name)
                    || expression_contains_function(right, function_name)
            }
            PlanRowExpression::BytesEqual { left, right } => {
                function_name == "Bytes/equal"
                    || expression_contains_function(left, function_name)
                    || expression_contains_function(right, function_name)
            }
            PlanRowExpression::BuiltinCall {
                function,
                input,
                args,
            } => {
                function == function_name
                    || input
                        .as_deref()
                        .is_some_and(|input| expression_contains_function(input, function_name))
                    || args
                        .iter()
                        .any(|arg| expression_contains_function(&arg.value, function_name))
            }
            PlanRowExpression::TextTrim { input }
            | PlanRowExpression::TextIsEmpty { input }
            | PlanRowExpression::TextLength { input }
            | PlanRowExpression::TextToNumber { input }
            | PlanRowExpression::ObjectField { object: input, .. }
            | PlanRowExpression::ListSum { input } => {
                expression_contains_function(input, function_name)
            }
            PlanRowExpression::Object { fields } => fields
                .iter()
                .any(|field| expression_contains_function(&field.value, function_name)),
            PlanRowExpression::TextStartsWith { input, prefix } => {
                expression_contains_function(input, function_name)
                    || expression_contains_function(prefix, function_name)
            }
            PlanRowExpression::TextSubstring {
                input,
                start,
                length,
            } => {
                expression_contains_function(input, function_name)
                    || expression_contains_function(start, function_name)
                    || expression_contains_function(length, function_name)
            }
            PlanRowExpression::BytesLength { input } => {
                function_name == "Bytes/length"
                    || expression_contains_function(input, function_name)
            }
            PlanRowExpression::BytesGet { input, index } => {
                function_name == "Bytes/get"
                    || expression_contains_function(input, function_name)
                    || expression_contains_function(index, function_name)
            }
            PlanRowExpression::BytesSlice {
                input,
                offset,
                byte_count,
            } => {
                expression_contains_function(input, function_name)
                    || expression_contains_function(offset, function_name)
                    || expression_contains_function(byte_count, function_name)
            }
            PlanRowExpression::BytesTake { input, byte_count }
            | PlanRowExpression::BytesDrop { input, byte_count } => {
                expression_contains_function(input, function_name)
                    || expression_contains_function(byte_count, function_name)
            }
            PlanRowExpression::BytesZeros { byte_count } => {
                expression_contains_function(byte_count, function_name)
            }
            PlanRowExpression::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            }
            | PlanRowExpression::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                (matches!(
                    expression,
                    PlanRowExpression::BytesReadUnsigned { .. }
                        if function_name == "Bytes/read_unsigned"
                ) || matches!(
                    expression,
                    PlanRowExpression::BytesReadSigned { .. }
                        if function_name == "Bytes/read_signed"
                )) || expression_contains_function(input, function_name)
                    || expression_contains_function(offset, function_name)
                    || expression_contains_function(byte_count, function_name)
                    || expression_contains_function(endian, function_name)
            }
            PlanRowExpression::BytesSet {
                input,
                index,
                value,
            } => {
                function_name == "Bytes/set"
                    || expression_contains_function(input, function_name)
                    || expression_contains_function(index, function_name)
                    || expression_contains_function(value, function_name)
            }
            PlanRowExpression::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                function_name == "Bytes/write_unsigned"
                    || expression_contains_function(input, function_name)
                    || expression_contains_function(offset, function_name)
                    || expression_contains_function(byte_count, function_name)
                    || expression_contains_function(endian, function_name)
                    || expression_contains_function(value, function_name)
            }
            PlanRowExpression::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                function_name == "Bytes/write_signed"
                    || expression_contains_function(input, function_name)
                    || expression_contains_function(offset, function_name)
                    || expression_contains_function(byte_count, function_name)
                    || expression_contains_function(endian, function_name)
                    || expression_contains_function(value, function_name)
            }
            PlanRowExpression::NumberInfix { left, right, .. } => {
                expression_contains_function(left, function_name)
                    || expression_contains_function(right, function_name)
            }
            PlanRowExpression::TextConcat { parts } => parts
                .iter()
                .any(|part| expression_contains_function(part, function_name)),
            PlanRowExpression::ListGetField { index, .. } => {
                expression_contains_function(index, function_name)
            }
            PlanRowExpression::ListFindValue {
                value, fallback, ..
            } => {
                expression_contains_function(value, function_name)
                    || fallback.as_deref().is_some_and(|fallback| {
                        expression_contains_function(fallback, function_name)
                    })
            }
            PlanRowExpression::ListRange { from, to } => {
                expression_contains_function(from, function_name)
                    || expression_contains_function(to, function_name)
            }
            PlanRowExpression::ListLiteral { items } => items
                .iter()
                .any(|item| expression_contains_function(item, function_name)),
            PlanRowExpression::ListMap { input, value, .. } => {
                expression_contains_function(input, function_name)
                    || expression_contains_function(value, function_name)
            }
            PlanRowExpression::Select { input, arms } => {
                expression_contains_function(input, function_name)
                    || arms
                        .iter()
                        .any(|arm| expression_contains_function(&arm.value, function_name))
            }
            PlanRowExpression::Field { .. }
            | PlanRowExpression::Constant { .. }
            | PlanRowExpression::ListRef { .. }
            | PlanRowExpression::ListMapItem { .. } => false,
        }
    }

    fn is_direct_bytes_find_result(expression: &PlanRowExpression) -> bool {
        match expression {
            PlanRowExpression::BytesFind { .. } => true,
            PlanRowExpression::BuiltinCall { function, .. } => function == "Bytes/find",
            _ => false,
        }
    }

    fn contains_direct_bytes_find_text_concat(expression: &PlanRowExpression) -> bool {
        match expression {
            PlanRowExpression::TextConcat { parts } => {
                parts.iter().any(is_direct_bytes_find_result)
            }
            PlanRowExpression::TextTrim { input }
            | PlanRowExpression::TextIsEmpty { input }
            | PlanRowExpression::TextLength { input }
            | PlanRowExpression::TextToNumber { input }
            | PlanRowExpression::ObjectField { object: input, .. }
            | PlanRowExpression::ListSum { input } => contains_direct_bytes_find_text_concat(input),
            PlanRowExpression::Object { fields } => fields
                .iter()
                .any(|field| contains_direct_bytes_find_text_concat(&field.value)),
            PlanRowExpression::TextStartsWith { input, prefix } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(prefix)
            }
            PlanRowExpression::TextSubstring {
                input,
                start,
                length,
            } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(start)
                    || contains_direct_bytes_find_text_concat(length)
            }
            PlanRowExpression::TextToBytes { input, encoding } => {
                contains_direct_bytes_find_text_concat(input)
                    || encoding
                        .as_deref()
                        .is_some_and(contains_direct_bytes_find_text_concat)
            }
            PlanRowExpression::BytesToText { input, encoding } => {
                contains_direct_bytes_find_text_concat(input)
                    || encoding
                        .as_deref()
                        .is_some_and(contains_direct_bytes_find_text_concat)
            }
            PlanRowExpression::BytesToHex { input }
            | PlanRowExpression::BytesToBase64 { input }
            | PlanRowExpression::BytesFromHex { input }
            | PlanRowExpression::BytesFromBase64 { input } => {
                contains_direct_bytes_find_text_concat(input)
            }
            PlanRowExpression::BytesLength { input } => {
                contains_direct_bytes_find_text_concat(input)
            }
            PlanRowExpression::BytesGet { input, index } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(index)
            }
            PlanRowExpression::BytesSlice {
                input,
                offset,
                byte_count,
            } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(offset)
                    || contains_direct_bytes_find_text_concat(byte_count)
            }
            PlanRowExpression::BytesTake { input, byte_count }
            | PlanRowExpression::BytesDrop { input, byte_count } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(byte_count)
            }
            PlanRowExpression::BytesZeros { byte_count } => {
                contains_direct_bytes_find_text_concat(byte_count)
            }
            PlanRowExpression::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            }
            | PlanRowExpression::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(offset)
                    || contains_direct_bytes_find_text_concat(byte_count)
                    || contains_direct_bytes_find_text_concat(endian)
            }
            PlanRowExpression::BytesSet {
                input,
                index,
                value,
            } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(index)
                    || contains_direct_bytes_find_text_concat(value)
            }
            PlanRowExpression::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            }
            | PlanRowExpression::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(offset)
                    || contains_direct_bytes_find_text_concat(byte_count)
                    || contains_direct_bytes_find_text_concat(endian)
                    || contains_direct_bytes_find_text_concat(value)
            }
            PlanRowExpression::BytesFind { input, needle } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(needle)
            }
            PlanRowExpression::BytesStartsWith { input, prefix } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(prefix)
            }
            PlanRowExpression::BytesEndsWith { input, suffix } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(suffix)
            }
            PlanRowExpression::BytesIsEmpty { input } => {
                contains_direct_bytes_find_text_concat(input)
            }
            PlanRowExpression::BytesConcat { left, right } => {
                contains_direct_bytes_find_text_concat(left)
                    || contains_direct_bytes_find_text_concat(right)
            }
            PlanRowExpression::BytesEqual { left, right } => {
                contains_direct_bytes_find_text_concat(left)
                    || contains_direct_bytes_find_text_concat(right)
            }
            PlanRowExpression::NumberInfix { left, right, .. } => {
                contains_direct_bytes_find_text_concat(left)
                    || contains_direct_bytes_find_text_concat(right)
            }
            PlanRowExpression::ListGetField { index, .. } => {
                contains_direct_bytes_find_text_concat(index)
            }
            PlanRowExpression::ListFindValue {
                value, fallback, ..
            } => {
                contains_direct_bytes_find_text_concat(value)
                    || fallback
                        .as_deref()
                        .is_some_and(contains_direct_bytes_find_text_concat)
            }
            PlanRowExpression::ListRange { from, to } => {
                contains_direct_bytes_find_text_concat(from)
                    || contains_direct_bytes_find_text_concat(to)
            }
            PlanRowExpression::ListLiteral { items } => {
                items.iter().any(contains_direct_bytes_find_text_concat)
            }
            PlanRowExpression::ListMap { input, value, .. } => {
                contains_direct_bytes_find_text_concat(input)
                    || contains_direct_bytes_find_text_concat(value)
            }
            PlanRowExpression::BuiltinCall { input, args, .. } => {
                input
                    .as_deref()
                    .is_some_and(contains_direct_bytes_find_text_concat)
                    || args
                        .iter()
                        .any(|arg| contains_direct_bytes_find_text_concat(&arg.value))
            }
            PlanRowExpression::Select { input, arms } => {
                contains_direct_bytes_find_text_concat(input)
                    || arms
                        .iter()
                        .any(|arm| contains_direct_bytes_find_text_concat(&arm.value))
            }
            PlanRowExpression::Field { .. }
            | PlanRowExpression::Constant { .. }
            | PlanRowExpression::ListRef { .. }
            | PlanRowExpression::ListMapItem { .. } => false,
        }
    }

    fn contains_bytes_find_numeric_plus(expression: &PlanRowExpression) -> bool {
        match expression {
            PlanRowExpression::NumberInfix { op, left, right } if op == "+" => {
                expression_contains_function(left, "Bytes/find")
                    || expression_contains_function(right, "Bytes/find")
                    || contains_bytes_find_numeric_plus(left)
                    || contains_bytes_find_numeric_plus(right)
            }
            PlanRowExpression::TextTrim { input }
            | PlanRowExpression::TextIsEmpty { input }
            | PlanRowExpression::TextLength { input }
            | PlanRowExpression::TextToNumber { input }
            | PlanRowExpression::ObjectField { object: input, .. }
            | PlanRowExpression::ListSum { input } => contains_bytes_find_numeric_plus(input),
            PlanRowExpression::Object { fields } => fields
                .iter()
                .any(|field| contains_bytes_find_numeric_plus(&field.value)),
            PlanRowExpression::TextStartsWith { input, prefix } => {
                contains_bytes_find_numeric_plus(input) || contains_bytes_find_numeric_plus(prefix)
            }
            PlanRowExpression::TextSubstring {
                input,
                start,
                length,
            } => {
                contains_bytes_find_numeric_plus(input)
                    || contains_bytes_find_numeric_plus(start)
                    || contains_bytes_find_numeric_plus(length)
            }
            PlanRowExpression::TextToBytes { input, encoding } => {
                contains_bytes_find_numeric_plus(input)
                    || encoding
                        .as_deref()
                        .is_some_and(contains_bytes_find_numeric_plus)
            }
            PlanRowExpression::BytesToText { input, encoding } => {
                contains_bytes_find_numeric_plus(input)
                    || encoding
                        .as_deref()
                        .is_some_and(contains_bytes_find_numeric_plus)
            }
            PlanRowExpression::BytesToHex { input }
            | PlanRowExpression::BytesToBase64 { input }
            | PlanRowExpression::BytesFromHex { input }
            | PlanRowExpression::BytesFromBase64 { input } => {
                contains_bytes_find_numeric_plus(input)
            }
            PlanRowExpression::BytesLength { input } => contains_bytes_find_numeric_plus(input),
            PlanRowExpression::BytesGet { input, index } => {
                contains_bytes_find_numeric_plus(input) || contains_bytes_find_numeric_plus(index)
            }
            PlanRowExpression::BytesSlice {
                input,
                offset,
                byte_count,
            } => {
                contains_bytes_find_numeric_plus(input)
                    || contains_bytes_find_numeric_plus(offset)
                    || contains_bytes_find_numeric_plus(byte_count)
            }
            PlanRowExpression::BytesTake { input, byte_count }
            | PlanRowExpression::BytesDrop { input, byte_count } => {
                contains_bytes_find_numeric_plus(input)
                    || contains_bytes_find_numeric_plus(byte_count)
            }
            PlanRowExpression::BytesZeros { byte_count } => {
                contains_bytes_find_numeric_plus(byte_count)
            }
            PlanRowExpression::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            }
            | PlanRowExpression::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                contains_bytes_find_numeric_plus(input)
                    || contains_bytes_find_numeric_plus(offset)
                    || contains_bytes_find_numeric_plus(byte_count)
                    || contains_bytes_find_numeric_plus(endian)
            }
            PlanRowExpression::BytesSet {
                input,
                index,
                value,
            } => {
                contains_bytes_find_numeric_plus(input)
                    || contains_bytes_find_numeric_plus(index)
                    || contains_bytes_find_numeric_plus(value)
            }
            PlanRowExpression::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            }
            | PlanRowExpression::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                contains_bytes_find_numeric_plus(input)
                    || contains_bytes_find_numeric_plus(offset)
                    || contains_bytes_find_numeric_plus(byte_count)
                    || contains_bytes_find_numeric_plus(endian)
                    || contains_bytes_find_numeric_plus(value)
            }
            PlanRowExpression::BytesFind { input, needle } => {
                contains_bytes_find_numeric_plus(input) || contains_bytes_find_numeric_plus(needle)
            }
            PlanRowExpression::BytesStartsWith { input, prefix } => {
                contains_bytes_find_numeric_plus(input) || contains_bytes_find_numeric_plus(prefix)
            }
            PlanRowExpression::BytesEndsWith { input, suffix } => {
                contains_bytes_find_numeric_plus(input) || contains_bytes_find_numeric_plus(suffix)
            }
            PlanRowExpression::BytesIsEmpty { input } => contains_bytes_find_numeric_plus(input),
            PlanRowExpression::BytesConcat { left, right } => {
                contains_bytes_find_numeric_plus(left) || contains_bytes_find_numeric_plus(right)
            }
            PlanRowExpression::BytesEqual { left, right } => {
                contains_bytes_find_numeric_plus(left) || contains_bytes_find_numeric_plus(right)
            }
            PlanRowExpression::NumberInfix { left, right, .. } => {
                contains_bytes_find_numeric_plus(left) || contains_bytes_find_numeric_plus(right)
            }
            PlanRowExpression::TextConcat { parts } => {
                parts.iter().any(contains_bytes_find_numeric_plus)
            }
            PlanRowExpression::ListGetField { index, .. } => {
                contains_bytes_find_numeric_plus(index)
            }
            PlanRowExpression::ListFindValue {
                value, fallback, ..
            } => {
                contains_bytes_find_numeric_plus(value)
                    || fallback
                        .as_deref()
                        .is_some_and(contains_bytes_find_numeric_plus)
            }
            PlanRowExpression::ListRange { from, to } => {
                contains_bytes_find_numeric_plus(from) || contains_bytes_find_numeric_plus(to)
            }
            PlanRowExpression::ListLiteral { items } => {
                items.iter().any(contains_bytes_find_numeric_plus)
            }
            PlanRowExpression::ListMap { input, value, .. } => {
                contains_bytes_find_numeric_plus(input) || contains_bytes_find_numeric_plus(value)
            }
            PlanRowExpression::BuiltinCall { input, args, .. } => {
                input
                    .as_deref()
                    .is_some_and(contains_bytes_find_numeric_plus)
                    || args
                        .iter()
                        .any(|arg| contains_bytes_find_numeric_plus(&arg.value))
            }
            PlanRowExpression::Select { input, arms } => {
                contains_bytes_find_numeric_plus(input)
                    || arms
                        .iter()
                        .any(|arm| contains_bytes_find_numeric_plus(&arm.value))
            }
            PlanRowExpression::Field { .. }
            | PlanRowExpression::Constant { .. }
            | PlanRowExpression::ListRef { .. }
            | PlanRowExpression::ListMapItem { .. } => false,
        }
    }

    fn contains_typed_bytes_slice(expression: &PlanRowExpression) -> bool {
        match expression {
            PlanRowExpression::BytesSlice {
                input, byte_count, ..
            } => {
                matches!(byte_count.as_ref(), PlanRowExpression::NumberInfix { .. })
                    || contains_typed_bytes_slice(input)
                    || contains_typed_bytes_slice(byte_count)
            }
            PlanRowExpression::TextTrim { input }
            | PlanRowExpression::TextIsEmpty { input }
            | PlanRowExpression::TextLength { input }
            | PlanRowExpression::TextToNumber { input }
            | PlanRowExpression::ObjectField { object: input, .. }
            | PlanRowExpression::ListSum { input } => contains_typed_bytes_slice(input),
            PlanRowExpression::Object { fields } => fields
                .iter()
                .any(|field| contains_typed_bytes_slice(&field.value)),
            PlanRowExpression::TextStartsWith { input, prefix } => {
                contains_typed_bytes_slice(input) || contains_typed_bytes_slice(prefix)
            }
            PlanRowExpression::TextSubstring {
                input,
                start,
                length,
            } => {
                contains_typed_bytes_slice(input)
                    || contains_typed_bytes_slice(start)
                    || contains_typed_bytes_slice(length)
            }
            PlanRowExpression::TextToBytes { input, encoding } => {
                contains_typed_bytes_slice(input)
                    || encoding.as_deref().is_some_and(contains_typed_bytes_slice)
            }
            PlanRowExpression::BytesToText { input, encoding } => {
                contains_typed_bytes_slice(input)
                    || encoding.as_deref().is_some_and(contains_typed_bytes_slice)
            }
            PlanRowExpression::BytesToHex { input }
            | PlanRowExpression::BytesToBase64 { input }
            | PlanRowExpression::BytesFromHex { input }
            | PlanRowExpression::BytesFromBase64 { input } => contains_typed_bytes_slice(input),
            PlanRowExpression::BytesLength { input } => contains_typed_bytes_slice(input),
            PlanRowExpression::BytesGet { input, index } => {
                contains_typed_bytes_slice(input) || contains_typed_bytes_slice(index)
            }
            PlanRowExpression::BytesTake { input, byte_count }
            | PlanRowExpression::BytesDrop { input, byte_count } => {
                contains_typed_bytes_slice(input) || contains_typed_bytes_slice(byte_count)
            }
            PlanRowExpression::BytesZeros { byte_count } => contains_typed_bytes_slice(byte_count),
            PlanRowExpression::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            }
            | PlanRowExpression::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                contains_typed_bytes_slice(input)
                    || contains_typed_bytes_slice(offset)
                    || contains_typed_bytes_slice(byte_count)
                    || contains_typed_bytes_slice(endian)
            }
            PlanRowExpression::BytesSet {
                input,
                index,
                value,
            } => {
                contains_typed_bytes_slice(input)
                    || contains_typed_bytes_slice(index)
                    || contains_typed_bytes_slice(value)
            }
            PlanRowExpression::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            }
            | PlanRowExpression::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                contains_typed_bytes_slice(input)
                    || contains_typed_bytes_slice(offset)
                    || contains_typed_bytes_slice(byte_count)
                    || contains_typed_bytes_slice(endian)
                    || contains_typed_bytes_slice(value)
            }
            PlanRowExpression::BytesFind { input, needle } => {
                contains_typed_bytes_slice(input) || contains_typed_bytes_slice(needle)
            }
            PlanRowExpression::BytesStartsWith { input, prefix } => {
                contains_typed_bytes_slice(input) || contains_typed_bytes_slice(prefix)
            }
            PlanRowExpression::BytesEndsWith { input, suffix } => {
                contains_typed_bytes_slice(input) || contains_typed_bytes_slice(suffix)
            }
            PlanRowExpression::BytesIsEmpty { input } => contains_typed_bytes_slice(input),
            PlanRowExpression::BytesConcat { left, right } => {
                contains_typed_bytes_slice(left) || contains_typed_bytes_slice(right)
            }
            PlanRowExpression::BytesEqual { left, right } => {
                contains_typed_bytes_slice(left) || contains_typed_bytes_slice(right)
            }
            PlanRowExpression::NumberInfix { left, right, .. } => {
                contains_typed_bytes_slice(left) || contains_typed_bytes_slice(right)
            }
            PlanRowExpression::TextConcat { parts } => parts.iter().any(contains_typed_bytes_slice),
            PlanRowExpression::ListGetField { index, .. } => contains_typed_bytes_slice(index),
            PlanRowExpression::ListFindValue {
                value, fallback, ..
            } => {
                contains_typed_bytes_slice(value)
                    || fallback.as_deref().is_some_and(contains_typed_bytes_slice)
            }
            PlanRowExpression::ListRange { from, to } => {
                contains_typed_bytes_slice(from) || contains_typed_bytes_slice(to)
            }
            PlanRowExpression::ListLiteral { items } => {
                items.iter().any(contains_typed_bytes_slice)
            }
            PlanRowExpression::ListMap { input, value, .. } => {
                contains_typed_bytes_slice(input) || contains_typed_bytes_slice(value)
            }
            PlanRowExpression::BuiltinCall { input, args, .. } => {
                input.as_deref().is_some_and(contains_typed_bytes_slice)
                    || args
                        .iter()
                        .any(|arg| contains_typed_bytes_slice(&arg.value))
            }
            PlanRowExpression::Select { input, arms } => {
                contains_typed_bytes_slice(input)
                    || arms
                        .iter()
                        .any(|arm| contains_typed_bytes_slice(&arm.value))
            }
            PlanRowExpression::Field { .. }
            | PlanRowExpression::Constant { .. }
            | PlanRowExpression::ListRef { .. }
            | PlanRowExpression::ListMapItem { .. } => false,
        }
    }

    fn contains_typed_byte_scanner_ops(expression: &PlanRowExpression) -> bool {
        match expression {
            PlanRowExpression::TextToBytes { .. }
            | PlanRowExpression::BytesToText { .. }
            | PlanRowExpression::BytesToHex { .. }
            | PlanRowExpression::BytesToBase64 { .. }
            | PlanRowExpression::BytesFromHex { .. }
            | PlanRowExpression::BytesFromBase64 { .. }
            | PlanRowExpression::BytesFind { .. }
            | PlanRowExpression::BytesStartsWith { .. }
            | PlanRowExpression::BytesEndsWith { .. }
            | PlanRowExpression::BytesIsEmpty { .. }
            | PlanRowExpression::BytesConcat { .. }
            | PlanRowExpression::BytesEqual { .. }
            | PlanRowExpression::BytesTake { .. }
            | PlanRowExpression::BytesDrop { .. }
            | PlanRowExpression::BytesZeros { .. }
            | PlanRowExpression::BytesReadUnsigned { .. }
            | PlanRowExpression::BytesReadSigned { .. }
            | PlanRowExpression::BytesSet { .. }
            | PlanRowExpression::BytesWriteUnsigned { .. }
            | PlanRowExpression::BytesWriteSigned { .. } => true,
            PlanRowExpression::TextTrim { input }
            | PlanRowExpression::TextIsEmpty { input }
            | PlanRowExpression::TextLength { input }
            | PlanRowExpression::TextToNumber { input }
            | PlanRowExpression::ObjectField { object: input, .. }
            | PlanRowExpression::ListSum { input } => contains_typed_byte_scanner_ops(input),
            PlanRowExpression::Object { fields } => fields
                .iter()
                .any(|field| contains_typed_byte_scanner_ops(&field.value)),
            PlanRowExpression::TextStartsWith { input, prefix } => {
                contains_typed_byte_scanner_ops(input) || contains_typed_byte_scanner_ops(prefix)
            }
            PlanRowExpression::TextSubstring {
                input,
                start,
                length,
            } => {
                contains_typed_byte_scanner_ops(input)
                    || contains_typed_byte_scanner_ops(start)
                    || contains_typed_byte_scanner_ops(length)
            }
            PlanRowExpression::BytesSlice {
                input,
                offset,
                byte_count,
            } => {
                contains_typed_byte_scanner_ops(input)
                    || contains_typed_byte_scanner_ops(offset)
                    || contains_typed_byte_scanner_ops(byte_count)
            }
            PlanRowExpression::BytesLength { input } => contains_typed_byte_scanner_ops(input),
            PlanRowExpression::BytesGet { input, index } => {
                contains_typed_byte_scanner_ops(input) || contains_typed_byte_scanner_ops(index)
            }
            PlanRowExpression::NumberInfix { left, right, .. } => {
                contains_typed_byte_scanner_ops(left) || contains_typed_byte_scanner_ops(right)
            }
            PlanRowExpression::TextConcat { parts } => {
                parts.iter().any(contains_typed_byte_scanner_ops)
            }
            PlanRowExpression::ListGetField { index, .. } => contains_typed_byte_scanner_ops(index),
            PlanRowExpression::ListFindValue {
                value, fallback, ..
            } => {
                contains_typed_byte_scanner_ops(value)
                    || fallback
                        .as_deref()
                        .is_some_and(contains_typed_byte_scanner_ops)
            }
            PlanRowExpression::ListRange { from, to } => {
                contains_typed_byte_scanner_ops(from) || contains_typed_byte_scanner_ops(to)
            }
            PlanRowExpression::ListLiteral { items } => {
                items.iter().any(contains_typed_byte_scanner_ops)
            }
            PlanRowExpression::ListMap { input, value, .. } => {
                contains_typed_byte_scanner_ops(input) || contains_typed_byte_scanner_ops(value)
            }
            PlanRowExpression::BuiltinCall { input, args, .. } => {
                input
                    .as_deref()
                    .is_some_and(contains_typed_byte_scanner_ops)
                    || args
                        .iter()
                        .any(|arg| contains_typed_byte_scanner_ops(&arg.value))
            }
            PlanRowExpression::Select { input, arms } => {
                contains_typed_byte_scanner_ops(input)
                    || arms
                        .iter()
                        .any(|arm| contains_typed_byte_scanner_ops(&arm.value))
            }
            PlanRowExpression::Field { .. }
            | PlanRowExpression::Constant { .. }
            | PlanRowExpression::ListRef { .. }
            | PlanRowExpression::ListMapItem { .. } => false,
        }
    }

    fn matches_generic_row_builtin_function(
        expression: &PlanRowExpression,
        function_name: &str,
    ) -> bool {
        match expression {
            PlanRowExpression::BuiltinCall {
                function,
                input,
                args,
            } => {
                function == function_name
                    || input.as_deref().is_some_and(|input| {
                        matches_generic_row_builtin_function(input, function_name)
                    })
                    || args
                        .iter()
                        .any(|arg| matches_generic_row_builtin_function(&arg.value, function_name))
            }
            PlanRowExpression::TextTrim { input }
            | PlanRowExpression::TextIsEmpty { input }
            | PlanRowExpression::TextLength { input }
            | PlanRowExpression::TextToNumber { input }
            | PlanRowExpression::ObjectField { object: input, .. }
            | PlanRowExpression::ListSum { input } => {
                matches_generic_row_builtin_function(input, function_name)
            }
            PlanRowExpression::Object { fields } => fields
                .iter()
                .any(|field| matches_generic_row_builtin_function(&field.value, function_name)),
            PlanRowExpression::TextStartsWith { input, prefix } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(prefix, function_name)
            }
            PlanRowExpression::TextSubstring {
                input,
                start,
                length,
            } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(start, function_name)
                    || matches_generic_row_builtin_function(length, function_name)
            }
            PlanRowExpression::TextToBytes { input, encoding } => {
                matches_generic_row_builtin_function(input, function_name)
                    || encoding.as_deref().is_some_and(|encoding| {
                        matches_generic_row_builtin_function(encoding, function_name)
                    })
            }
            PlanRowExpression::BytesToText { input, encoding } => {
                matches_generic_row_builtin_function(input, function_name)
                    || encoding.as_deref().is_some_and(|encoding| {
                        matches_generic_row_builtin_function(encoding, function_name)
                    })
            }
            PlanRowExpression::BytesToHex { input }
            | PlanRowExpression::BytesToBase64 { input }
            | PlanRowExpression::BytesFromHex { input }
            | PlanRowExpression::BytesFromBase64 { input } => {
                matches_generic_row_builtin_function(input, function_name)
            }
            PlanRowExpression::BytesLength { input } => {
                matches_generic_row_builtin_function(input, function_name)
            }
            PlanRowExpression::BytesGet { input, index } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(index, function_name)
            }
            PlanRowExpression::BytesSlice {
                input,
                offset,
                byte_count,
            } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(offset, function_name)
                    || matches_generic_row_builtin_function(byte_count, function_name)
            }
            PlanRowExpression::BytesTake { input, byte_count }
            | PlanRowExpression::BytesDrop { input, byte_count } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(byte_count, function_name)
            }
            PlanRowExpression::BytesZeros { byte_count } => {
                matches_generic_row_builtin_function(byte_count, function_name)
            }
            PlanRowExpression::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            }
            | PlanRowExpression::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(offset, function_name)
                    || matches_generic_row_builtin_function(byte_count, function_name)
                    || matches_generic_row_builtin_function(endian, function_name)
            }
            PlanRowExpression::BytesSet {
                input,
                index,
                value,
            } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(index, function_name)
                    || matches_generic_row_builtin_function(value, function_name)
            }
            PlanRowExpression::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            }
            | PlanRowExpression::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(offset, function_name)
                    || matches_generic_row_builtin_function(byte_count, function_name)
                    || matches_generic_row_builtin_function(endian, function_name)
                    || matches_generic_row_builtin_function(value, function_name)
            }
            PlanRowExpression::BytesFind { input, needle } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(needle, function_name)
            }
            PlanRowExpression::BytesStartsWith { input, prefix } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(prefix, function_name)
            }
            PlanRowExpression::BytesEndsWith { input, suffix } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(suffix, function_name)
            }
            PlanRowExpression::BytesIsEmpty { input } => {
                matches_generic_row_builtin_function(input, function_name)
            }
            PlanRowExpression::BytesConcat { left, right } => {
                matches_generic_row_builtin_function(left, function_name)
                    || matches_generic_row_builtin_function(right, function_name)
            }
            PlanRowExpression::BytesEqual { left, right } => {
                matches_generic_row_builtin_function(left, function_name)
                    || matches_generic_row_builtin_function(right, function_name)
            }
            PlanRowExpression::NumberInfix { left, right, .. } => {
                matches_generic_row_builtin_function(left, function_name)
                    || matches_generic_row_builtin_function(right, function_name)
            }
            PlanRowExpression::TextConcat { parts } => parts
                .iter()
                .any(|part| matches_generic_row_builtin_function(part, function_name)),
            PlanRowExpression::ListGetField { index, .. } => {
                matches_generic_row_builtin_function(index, function_name)
            }
            PlanRowExpression::ListFindValue {
                value, fallback, ..
            } => {
                matches_generic_row_builtin_function(value, function_name)
                    || fallback.as_deref().is_some_and(|fallback| {
                        matches_generic_row_builtin_function(fallback, function_name)
                    })
            }
            PlanRowExpression::ListRange { from, to } => {
                matches_generic_row_builtin_function(from, function_name)
                    || matches_generic_row_builtin_function(to, function_name)
            }
            PlanRowExpression::ListLiteral { items } => items
                .iter()
                .any(|item| matches_generic_row_builtin_function(item, function_name)),
            PlanRowExpression::ListMap { input, value, .. } => {
                matches_generic_row_builtin_function(input, function_name)
                    || matches_generic_row_builtin_function(value, function_name)
            }
            PlanRowExpression::Select { input, arms } => {
                matches_generic_row_builtin_function(input, function_name)
                    || arms
                        .iter()
                        .any(|arm| matches_generic_row_builtin_function(&arm.value, function_name))
            }
            PlanRowExpression::Field { .. }
            | PlanRowExpression::Constant { .. }
            | PlanRowExpression::ListRef { .. }
            | PlanRowExpression::ListMapItem { .. } => false,
        }
    }

    fn replace_first_typed_byte_scanner_with_generic(expression: &mut PlanRowExpression) -> bool {
        match expression {
            PlanRowExpression::TextToBytes { input, encoding } => {
                let input = (**input).clone();
                let args = encoding
                    .as_deref()
                    .map(|encoding| PlanRowCallArg {
                        name: Some("encoding".to_owned()),
                        value: encoding.clone(),
                    })
                    .into_iter()
                    .collect();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Text/to_bytes".to_owned(),
                    input: Some(Box::new(input)),
                    args,
                };
                true
            }
            PlanRowExpression::BytesToText { input, encoding } => {
                let input = (**input).clone();
                let args = encoding
                    .as_deref()
                    .map(|encoding| PlanRowCallArg {
                        name: Some("encoding".to_owned()),
                        value: encoding.clone(),
                    })
                    .into_iter()
                    .collect();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/to_text".to_owned(),
                    input: Some(Box::new(input)),
                    args,
                };
                true
            }
            PlanRowExpression::BytesToHex { input } => {
                let input = (**input).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/to_hex".to_owned(),
                    input: Some(Box::new(input)),
                    args: Vec::new(),
                };
                true
            }
            PlanRowExpression::BytesToBase64 { input } => {
                let input = (**input).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/to_base64".to_owned(),
                    input: Some(Box::new(input)),
                    args: Vec::new(),
                };
                true
            }
            PlanRowExpression::BytesFromHex { input } => {
                let input = (**input).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/from_hex".to_owned(),
                    input: Some(Box::new(input)),
                    args: Vec::new(),
                };
                true
            }
            PlanRowExpression::BytesFromBase64 { input } => {
                let input = (**input).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/from_base64".to_owned(),
                    input: Some(Box::new(input)),
                    args: Vec::new(),
                };
                true
            }
            PlanRowExpression::BytesSlice {
                input,
                offset,
                byte_count,
            } => {
                let input = (**input).clone();
                let offset = (**offset).clone();
                let byte_count = (**byte_count).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/slice".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![
                        PlanRowCallArg {
                            name: Some("offset".to_owned()),
                            value: offset,
                        },
                        PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        },
                    ],
                };
                true
            }
            PlanRowExpression::BytesTake { input, byte_count } => {
                let input = (**input).clone();
                let byte_count = (**byte_count).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/take".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![PlanRowCallArg {
                        name: Some("byte_count".to_owned()),
                        value: byte_count,
                    }],
                };
                true
            }
            PlanRowExpression::BytesDrop { input, byte_count } => {
                let input = (**input).clone();
                let byte_count = (**byte_count).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/drop".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![PlanRowCallArg {
                        name: Some("byte_count".to_owned()),
                        value: byte_count,
                    }],
                };
                true
            }
            PlanRowExpression::BytesZeros { byte_count } => {
                let byte_count = (**byte_count).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/zeros".to_owned(),
                    input: None,
                    args: vec![PlanRowCallArg {
                        name: Some("byte_count".to_owned()),
                        value: byte_count,
                    }],
                };
                true
            }
            PlanRowExpression::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                let input = (**input).clone();
                let offset = (**offset).clone();
                let byte_count = (**byte_count).clone();
                let endian = (**endian).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/read_unsigned".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![
                        PlanRowCallArg {
                            name: Some("offset".to_owned()),
                            value: offset,
                        },
                        PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        },
                        PlanRowCallArg {
                            name: Some("endian".to_owned()),
                            value: endian,
                        },
                    ],
                };
                true
            }
            PlanRowExpression::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                let input = (**input).clone();
                let offset = (**offset).clone();
                let byte_count = (**byte_count).clone();
                let endian = (**endian).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/read_signed".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![
                        PlanRowCallArg {
                            name: Some("offset".to_owned()),
                            value: offset,
                        },
                        PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        },
                        PlanRowCallArg {
                            name: Some("endian".to_owned()),
                            value: endian,
                        },
                    ],
                };
                true
            }
            PlanRowExpression::BytesSet {
                input,
                index,
                value,
            } => {
                let input = (**input).clone();
                let index = (**index).clone();
                let value = (**value).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/set".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![
                        PlanRowCallArg {
                            name: Some("index".to_owned()),
                            value: index,
                        },
                        PlanRowCallArg {
                            name: Some("value".to_owned()),
                            value,
                        },
                    ],
                };
                true
            }
            PlanRowExpression::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                let input = (**input).clone();
                let offset = (**offset).clone();
                let byte_count = (**byte_count).clone();
                let endian = (**endian).clone();
                let value = (**value).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/write_unsigned".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![
                        PlanRowCallArg {
                            name: Some("offset".to_owned()),
                            value: offset,
                        },
                        PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        },
                        PlanRowCallArg {
                            name: Some("endian".to_owned()),
                            value: endian,
                        },
                        PlanRowCallArg {
                            name: Some("value".to_owned()),
                            value,
                        },
                    ],
                };
                true
            }
            PlanRowExpression::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                let input = (**input).clone();
                let offset = (**offset).clone();
                let byte_count = (**byte_count).clone();
                let endian = (**endian).clone();
                let value = (**value).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/write_signed".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![
                        PlanRowCallArg {
                            name: Some("offset".to_owned()),
                            value: offset,
                        },
                        PlanRowCallArg {
                            name: Some("byte_count".to_owned()),
                            value: byte_count,
                        },
                        PlanRowCallArg {
                            name: Some("endian".to_owned()),
                            value: endian,
                        },
                        PlanRowCallArg {
                            name: Some("value".to_owned()),
                            value,
                        },
                    ],
                };
                true
            }
            PlanRowExpression::BytesFind { input, needle } => {
                let input = (**input).clone();
                let needle = (**needle).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/find".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![PlanRowCallArg {
                        name: Some("needle".to_owned()),
                        value: needle,
                    }],
                };
                true
            }
            PlanRowExpression::BytesStartsWith { input, prefix } => {
                let input = (**input).clone();
                let prefix = (**prefix).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/starts_with".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![PlanRowCallArg {
                        name: Some("prefix".to_owned()),
                        value: prefix,
                    }],
                };
                true
            }
            PlanRowExpression::BytesEndsWith { input, suffix } => {
                let input = (**input).clone();
                let suffix = (**suffix).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/ends_with".to_owned(),
                    input: Some(Box::new(input)),
                    args: vec![PlanRowCallArg {
                        name: Some("suffix".to_owned()),
                        value: suffix,
                    }],
                };
                true
            }
            PlanRowExpression::BytesIsEmpty { input } => {
                let input = (**input).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/is_empty".to_owned(),
                    input: Some(Box::new(input)),
                    args: Vec::new(),
                };
                true
            }
            PlanRowExpression::BytesConcat { left, right } => {
                let left = (**left).clone();
                let right = (**right).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/concat".to_owned(),
                    input: Some(Box::new(left)),
                    args: vec![PlanRowCallArg {
                        name: Some("with".to_owned()),
                        value: right,
                    }],
                };
                true
            }
            PlanRowExpression::BytesEqual { left, right } => {
                let left = (**left).clone();
                let right = (**right).clone();
                *expression = PlanRowExpression::BuiltinCall {
                    function: "Bytes/equal".to_owned(),
                    input: Some(Box::new(left)),
                    args: vec![PlanRowCallArg {
                        name: Some("with".to_owned()),
                        value: right,
                    }],
                };
                true
            }
            PlanRowExpression::TextTrim { input }
            | PlanRowExpression::TextIsEmpty { input }
            | PlanRowExpression::TextLength { input }
            | PlanRowExpression::TextToNumber { input }
            | PlanRowExpression::ObjectField { object: input, .. }
            | PlanRowExpression::ListSum { input } => {
                replace_first_typed_byte_scanner_with_generic(input)
            }
            PlanRowExpression::Object { fields } => fields
                .iter_mut()
                .any(|field| replace_first_typed_byte_scanner_with_generic(&mut field.value)),
            PlanRowExpression::TextStartsWith { input, prefix } => {
                replace_first_typed_byte_scanner_with_generic(input)
                    || replace_first_typed_byte_scanner_with_generic(prefix)
            }
            PlanRowExpression::TextSubstring {
                input,
                start,
                length,
            } => {
                replace_first_typed_byte_scanner_with_generic(input)
                    || replace_first_typed_byte_scanner_with_generic(start)
                    || replace_first_typed_byte_scanner_with_generic(length)
            }
            PlanRowExpression::BytesLength { input } => {
                replace_first_typed_byte_scanner_with_generic(input)
            }
            PlanRowExpression::BytesGet { input, index } => {
                replace_first_typed_byte_scanner_with_generic(input)
                    || replace_first_typed_byte_scanner_with_generic(index)
            }
            PlanRowExpression::NumberInfix { left, right, .. } => {
                replace_first_typed_byte_scanner_with_generic(left)
                    || replace_first_typed_byte_scanner_with_generic(right)
            }
            PlanRowExpression::TextConcat { parts } => parts
                .iter_mut()
                .any(replace_first_typed_byte_scanner_with_generic),
            PlanRowExpression::ListGetField { index, .. } => {
                replace_first_typed_byte_scanner_with_generic(index)
            }
            PlanRowExpression::ListFindValue {
                value, fallback, ..
            } => {
                replace_first_typed_byte_scanner_with_generic(value)
                    || fallback
                        .as_deref_mut()
                        .is_some_and(replace_first_typed_byte_scanner_with_generic)
            }
            PlanRowExpression::ListRange { from, to } => {
                replace_first_typed_byte_scanner_with_generic(from)
                    || replace_first_typed_byte_scanner_with_generic(to)
            }
            PlanRowExpression::ListLiteral { items } => items
                .iter_mut()
                .any(replace_first_typed_byte_scanner_with_generic),
            PlanRowExpression::ListMap { input, value, .. } => {
                replace_first_typed_byte_scanner_with_generic(input)
                    || replace_first_typed_byte_scanner_with_generic(value)
            }
            PlanRowExpression::BuiltinCall { input, args, .. } => {
                input
                    .as_deref_mut()
                    .is_some_and(replace_first_typed_byte_scanner_with_generic)
                    || args
                        .iter_mut()
                        .any(|arg| replace_first_typed_byte_scanner_with_generic(&mut arg.value))
            }
            PlanRowExpression::Select { input, arms } => {
                replace_first_typed_byte_scanner_with_generic(input)
                    || arms
                        .iter_mut()
                        .any(|arm| replace_first_typed_byte_scanner_with_generic(&mut arm.value))
            }
            PlanRowExpression::Field { .. }
            | PlanRowExpression::Constant { .. }
            | PlanRowExpression::ListRef { .. }
            | PlanRowExpression::ListMapItem { .. } => false,
        }
    }

    let parsed = parse_cells_project_for_plan_test();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let value_id = debug_entry_id(&plan.debug_map.fields, "field", "cell.value");
    let value_expression = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .find_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::RowExpression { expression }),
                ..
            } if matches!(op.output, Some(ValueRef::Field(field_id)) if field_id.0 == value_id) => {
                Some(expression)
            }
            _ => None,
        })
        .expect("cell.value should lower to a row expression");

    assert!(
        contains_bytes_find_numeric_plus(value_expression),
        "Cells formula parser offsets such as `index + 1` should lower as numeric infix"
    );
    assert!(
        !contains_direct_bytes_find_text_concat(value_expression),
        "Bytes/find parser offsets must not lower through text concatenation"
    );
    assert!(
        contains_typed_bytes_slice(value_expression),
        "Cells formula byte scanning should lower Bytes/slice as a typed row expression with a dynamic numeric byte_count"
    );
    assert!(
        !expression_contains_function(value_expression, "Bytes/slice"),
        "Cells formula byte scanning must not leave Bytes/slice as a generic row builtin call"
    );
    assert!(
        contains_typed_byte_scanner_ops(value_expression),
        "Cells formula byte scanning should lower text-to-bytes and byte scanner calls as typed row expressions"
    );
    for function in ["Text/to_bytes", "Bytes/find", "Bytes/starts_with"] {
        assert!(
            !matches_generic_row_builtin_function(value_expression, function),
            "Cells formula byte scanning must not leave {function} as a generic row builtin call"
        );
    }

    let mut tampered = plan.clone();
    let replaced = tampered
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter_mut())
        .any(|op| match &mut op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::RowExpression { expression }),
                ..
            } if matches!(op.output, Some(ValueRef::Field(field_id)) if field_id.0 == value_id) => {
                replace_first_typed_byte_scanner_with_generic(expression)
            }
            _ => false,
        });
    assert!(
        replaced,
        "Cells should contain a typed row byte scanner to tamper"
    );
    let tampered_verification = verify_plan(&tampered).unwrap();
    assert_eq!(tampered_verification.status, "fail");
    assert!(
        tampered_verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "generic row byte scanner BuiltinCall must not remain verifier-admissible: {tampered_verification:#?}"
    );
}


#[test]
fn cells_row_lookup_field_ids_must_belong_to_referenced_list() {
    fn tamper_first_row_lookup(expression: &mut PlanRowExpression, invalid: FieldId) -> bool {
        match expression {
            PlanRowExpression::ListFindValue { target, .. } => {
                *target = invalid;
                true
            }
            PlanRowExpression::ListGetField { field, .. } => {
                *field = invalid;
                true
            }
            PlanRowExpression::TextTrim { input }
            | PlanRowExpression::TextIsEmpty { input }
            | PlanRowExpression::TextLength { input }
            | PlanRowExpression::TextToNumber { input }
            | PlanRowExpression::ObjectField { object: input, .. }
            | PlanRowExpression::ListSum { input } => tamper_first_row_lookup(input, invalid),
            PlanRowExpression::Object { fields } => fields
                .iter_mut()
                .any(|field| tamper_first_row_lookup(&mut field.value, invalid)),
            PlanRowExpression::TextStartsWith { input, prefix } => {
                tamper_first_row_lookup(input, invalid) || tamper_first_row_lookup(prefix, invalid)
            }
            PlanRowExpression::TextSubstring {
                input,
                start,
                length,
            } => {
                tamper_first_row_lookup(input, invalid)
                    || tamper_first_row_lookup(start, invalid)
                    || tamper_first_row_lookup(length, invalid)
            }
            PlanRowExpression::TextToBytes { input, encoding } => {
                tamper_first_row_lookup(input, invalid)
                    || encoding
                        .as_deref_mut()
                        .is_some_and(|encoding| tamper_first_row_lookup(encoding, invalid))
            }
            PlanRowExpression::BytesToText { input, encoding } => {
                tamper_first_row_lookup(input, invalid)
                    || encoding
                        .as_deref_mut()
                        .is_some_and(|encoding| tamper_first_row_lookup(encoding, invalid))
            }
            PlanRowExpression::BytesToHex { input }
            | PlanRowExpression::BytesToBase64 { input }
            | PlanRowExpression::BytesFromHex { input }
            | PlanRowExpression::BytesFromBase64 { input } => {
                tamper_first_row_lookup(input, invalid)
            }
            PlanRowExpression::BytesLength { input } => tamper_first_row_lookup(input, invalid),
            PlanRowExpression::BytesGet { input, index } => {
                tamper_first_row_lookup(input, invalid) || tamper_first_row_lookup(index, invalid)
            }
            PlanRowExpression::BytesSlice {
                input,
                offset,
                byte_count,
            } => {
                tamper_first_row_lookup(input, invalid)
                    || tamper_first_row_lookup(offset, invalid)
                    || tamper_first_row_lookup(byte_count, invalid)
            }
            PlanRowExpression::BytesTake { input, byte_count }
            | PlanRowExpression::BytesDrop { input, byte_count } => {
                tamper_first_row_lookup(input, invalid)
                    || tamper_first_row_lookup(byte_count, invalid)
            }
            PlanRowExpression::BytesZeros { byte_count } => {
                tamper_first_row_lookup(byte_count, invalid)
            }
            PlanRowExpression::BytesReadUnsigned {
                input,
                offset,
                byte_count,
                endian,
            }
            | PlanRowExpression::BytesReadSigned {
                input,
                offset,
                byte_count,
                endian,
            } => {
                tamper_first_row_lookup(input, invalid)
                    || tamper_first_row_lookup(offset, invalid)
                    || tamper_first_row_lookup(byte_count, invalid)
                    || tamper_first_row_lookup(endian, invalid)
            }
            PlanRowExpression::BytesSet {
                input,
                index,
                value,
            } => {
                tamper_first_row_lookup(input, invalid)
                    || tamper_first_row_lookup(index, invalid)
                    || tamper_first_row_lookup(value, invalid)
            }
            PlanRowExpression::BytesWriteUnsigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            }
            | PlanRowExpression::BytesWriteSigned {
                input,
                offset,
                byte_count,
                endian,
                value,
            } => {
                tamper_first_row_lookup(input, invalid)
                    || tamper_first_row_lookup(offset, invalid)
                    || tamper_first_row_lookup(byte_count, invalid)
                    || tamper_first_row_lookup(endian, invalid)
                    || tamper_first_row_lookup(value, invalid)
            }
            PlanRowExpression::BytesFind { input, needle } => {
                tamper_first_row_lookup(input, invalid) || tamper_first_row_lookup(needle, invalid)
            }
            PlanRowExpression::BytesStartsWith { input, prefix } => {
                tamper_first_row_lookup(input, invalid) || tamper_first_row_lookup(prefix, invalid)
            }
            PlanRowExpression::BytesEndsWith { input, suffix } => {
                tamper_first_row_lookup(input, invalid) || tamper_first_row_lookup(suffix, invalid)
            }
            PlanRowExpression::BytesIsEmpty { input } => tamper_first_row_lookup(input, invalid),
            PlanRowExpression::BytesConcat { left, right } => {
                tamper_first_row_lookup(left, invalid) || tamper_first_row_lookup(right, invalid)
            }
            PlanRowExpression::BytesEqual { left, right } => {
                tamper_first_row_lookup(left, invalid) || tamper_first_row_lookup(right, invalid)
            }
            PlanRowExpression::NumberInfix { left, right, .. } => {
                tamper_first_row_lookup(left, invalid) || tamper_first_row_lookup(right, invalid)
            }
            PlanRowExpression::TextConcat { parts } => parts
                .iter_mut()
                .any(|part| tamper_first_row_lookup(part, invalid)),
            PlanRowExpression::ListRange { from, to } => {
                tamper_first_row_lookup(from, invalid) || tamper_first_row_lookup(to, invalid)
            }
            PlanRowExpression::ListLiteral { items } => items
                .iter_mut()
                .any(|item| tamper_first_row_lookup(item, invalid)),
            PlanRowExpression::ListMap { input, value, .. } => {
                tamper_first_row_lookup(input, invalid) || tamper_first_row_lookup(value, invalid)
            }
            PlanRowExpression::BuiltinCall { input, args, .. } => {
                input
                    .as_deref_mut()
                    .is_some_and(|input| tamper_first_row_lookup(input, invalid))
                    || args
                        .iter_mut()
                        .any(|arg| tamper_first_row_lookup(&mut arg.value, invalid))
            }
            PlanRowExpression::Select { input, arms } => {
                tamper_first_row_lookup(input, invalid)
                    || arms
                        .iter_mut()
                        .any(|arm| tamper_first_row_lookup(&mut arm.value, invalid))
            }
            PlanRowExpression::Field { .. }
            | PlanRowExpression::Constant { .. }
            | PlanRowExpression::ListRef { .. }
            | PlanRowExpression::ListMapItem { .. } => false,
        }
    }

    let parsed = parse_cells_project_for_plan_test();
    let ir = boon_ir::lower(&parsed).unwrap();
    let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

    let verification = verify_plan(&plan).unwrap();
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "row-expression-list-fields-resolve" && check.pass),
        "fresh Cells row lookup field ids should verify: {verification:#?}"
    );

    let invalid = FieldId(usize::MAX - 1);
    let tampered = plan
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter_mut())
        .any(|op| match &mut op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::RowExpression { expression }),
                ..
            } => tamper_first_row_lookup(expression, invalid),
            _ => false,
        });
    assert!(tampered, "Cells should contain a row lookup expression");

    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "fail");
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "row-expression-list-fields-resolve" && !check.pass),
        "tampered row lookup field id should fail membership verification: {verification:#?}"
    );
}


#[test]
fn cells_row_initial_fields_get_concrete_storage_types() {
    let parsed = parse_cells_project_for_plan_test();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

    for label in ["cell.editing_text", "cell.formula_text"] {
        let state_id = StateId(debug_entry_id(&plan.debug_map.state_slots, "state", label));
        let storage_type =
            plan_value_type_for_state_slots(&plan.storage_layout.scalar_slots, state_id);
        assert_eq!(
            storage_type,
            Some(&PlanValueType::Text),
            "{label} should keep row-initial explainability but execute as TEXT"
        );
        let initial_kind = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id == state_id)
            .map(|slot| slot.initial_value_kind);
        assert_eq!(initial_kind, Some(InitialValueKind::RowInitialField));
    }

    let verification = verify_plan(&plan).unwrap();
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "constant-refs-resolve-and-match-storage-types" && check.pass),
        "Cells row-initial SourcePayload(Text) writes should verify with concrete storage types: {verification:#?}"
    );
    assert_eq!(verification.status, "pass");
}
