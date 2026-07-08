use super::*;

#[test]
fn typed_style_payloads_round_trip_as_typed_objects() {
    let rich_text = StyleValue::RichTextSpans(vec![StyleRichTextSpan {
        text: "SOURCE".to_owned(),
        source_text: Some("SOURCE".to_owned()),
        color: Some("#ff0000".to_owned()),
        font_style: Some("italic".to_owned()),
        font_weight: Some("bold".to_owned()),
    }]);
    let hints = StyleValue::EditorTypeHints(vec![StyleEditorTypeHint {
        line: 2,
        start: 4,
        end: 8,
        anchor_column: 12,
        category: "return".to_owned(),
        compact_label: "TEXT".to_owned(),
        detail_label: "TEXT value".to_owned(),
    }]);

    for value in [rich_text, hints] {
        let encoded = serde_json::to_value(&value).expect("style value should serialize");
        assert!(
            encoded.get("kind").is_some(),
            "typed style payloads must use tagged objects"
        );
        let decoded: StyleValue =
            serde_json::from_value(encoded).expect("typed style value should deserialize");
        assert_eq!(decoded, value);
    }
}

#[test]
fn typed_ui_style_changes_lower_to_compatible_style_patches() {
    let node = DocumentNodeId("node".to_owned());
    let typed_changes = vec![
        UiSemanticChange::SetLayoutStyle {
            id: node.clone(),
            patch: LayoutStylePatch {
                patch: BTreeMap::from([("width".to_owned(), Some(StyleValue::Number(120.0)))]),
            },
        },
        UiSemanticChange::SetPaintStyle {
            id: node.clone(),
            patch: PaintStylePatch {
                patch: BTreeMap::from([(
                    "background".to_owned(),
                    Some(StyleValue::Text("#fff".to_owned())),
                )]),
            },
        },
        UiSemanticChange::SetTextStyle {
            id: node.clone(),
            patch: TextStylePatch {
                patch: BTreeMap::from([(
                    "font_weight".to_owned(),
                    Some(StyleValue::Text("bold".to_owned())),
                )]),
            },
        },
        UiSemanticChange::SetMaterialStyle {
            id: node.clone(),
            patch: MaterialStylePatch {
                patch: BTreeMap::from([(
                    "material".to_owned(),
                    Some(StyleValue::Text("glass".to_owned())),
                )]),
            },
        },
    ];
    let batch: ChangeBatch<DocumentPatch> = ChangeBatch {
        epoch: 11,
        changes: typed_changes,
    }
    .into();

    assert_eq!(batch.epoch, 11);
    assert_eq!(batch.changes.len(), 4);
    for patch in batch.changes {
        assert!(
            matches!(patch, DocumentPatch::SetStyle { id, .. } if id == node),
            "typed style semantic changes should preserve compatible SetStyle lowering"
        );
    }
}
