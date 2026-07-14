use super::*;

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

#[test]
fn sensitive_text_input_artifacts_are_fixed_redactions() {
    const SENTINEL: &str = "document-SENTINEL-82be7a";
    let mut node = DocumentNode::new("password", DocumentNodeKind::TextInput);
    node.text = Some(TextValue {
        text: SENTINEL.to_owned(),
    });
    node.style
        .insert(SENSITIVE_INPUT_STYLE_KEY.to_owned(), StyleValue::Bool(true));
    node.style
        .insert("value".to_owned(), StyleValue::Text(SENTINEL.to_owned()));
    node.style
        .insert("caret_column".to_owned(), StyleValue::Number(123.0));

    let serialized = toml::to_string(&node).unwrap();
    let debug = format!("{node:?}");
    for artifact in [&serialized, &debug] {
        assert!(!artifact.contains(SENTINEL));
        assert!(!artifact.contains("82be7a"));
        assert!(!artifact.contains("123.0"));
        assert!(artifact.contains(SENSITIVE_INPUT_REDACTED_VALUE));
    }
    assert_eq!(
        node.presentation_text(true).as_deref(),
        Some(SENSITIVE_INPUT_REDACTED_GLYPHS)
    );
    assert_eq!(
        node.presentation_text(false).as_deref(),
        Some(SENSITIVE_INPUT_REDACTED_GLYPHS)
    );
}
