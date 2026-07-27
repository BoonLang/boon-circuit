use super::*;

#[test]
fn program_capability_profiles_have_stable_names_and_public_default() {
    #[derive(serde::Deserialize)]
    struct ProfileConfig {
        profile: ProgramCapabilityProfile,
    }

    assert_eq!(
        ProgramCapabilityProfile::default(),
        ProgramCapabilityProfile::PublicClient
    );
    assert_eq!(
        ProgramCapabilityProfile::PublicClient.name(),
        "public_client"
    );
    assert_eq!(
        ProgramCapabilityProfile::TrustedServer.name(),
        "trusted_server"
    );
    assert_eq!(
        toml::from_str::<ProfileConfig>("profile = \"trusted_server\"")
            .unwrap()
            .profile,
        ProgramCapabilityProfile::TrustedServer
    );
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

#[test]
fn runtime_route_identity_is_opaque_in_diagnostics() {
    let row = OwnerInstanceRow {
        list: ListId(0x41),
        key: 0x4242,
        generation: 0x4343,
    };
    let owner = OwnerInstanceRoute::new(PlanStaticOwnerId(0x44), [row]).unwrap();
    let route = SourceRouteToken::new(0x45, owner.clone(), SourceId(0x46), 0x47).unwrap();

    assert_eq!(format!("{row:?}"), "OwnerInstanceRow(..)");
    assert_eq!(format!("{owner:?}"), "OwnerInstanceRoute(..)");
    assert_eq!(format!("{route:?}"), "SourceRouteToken(..)");

    let diagnostic = format!("{row:?}\n{owner:?}\n{route:?}");
    for hidden in ["4242", "4343", "0x44", "0x45", "0x46", "0x47"] {
        assert!(!diagnostic.contains(hidden), "leaked `{hidden}`");
    }
}

#[test]
fn older_document_nodes_default_typed_focus_metadata_to_absent() {
    let node = DocumentNode::new("input", DocumentNodeKind::TextInput);
    let serialized = toml::to_string(&node).unwrap();
    assert!(!serialized.contains("text_input_id"));
    assert!(!serialized.contains("activation_focus"));
    let decoded: DocumentNode = toml::from_str(&serialized).unwrap();
    assert_eq!(decoded.text_input_id, None);
    assert_eq!(decoded.activation_focus, None);
}

#[test]
fn typed_focus_patch_has_a_stable_tagged_round_trip() {
    let patch = DocumentPatch::SetTextInputFocus {
        id: DocumentNodeId("diagnostic".to_owned()),
        text_input_id: None,
        activation_focus: Some(TextInputFocusRequest {
            input_id: TextInputId("profile-source".to_owned()),
            line: 8,
            column: 3,
        }),
    };
    let serialized = toml::to_string(&patch).unwrap();
    assert!(serialized.contains("kind = \"set_text_input_focus\""));
    assert!(serialized.contains("profile-source"));
    assert_eq!(toml::from_str::<DocumentPatch>(&serialized).unwrap(), patch);
}

#[test]
fn embedded_program_artifact_is_versioned_redacted_and_derived_from_the_full_bundle() {
    let descriptor = EmbeddedProgramDescriptor {
        source: "scene: Helper/render()\n".to_owned(),
        revision: 7,
        support_sources: vec![EmbeddedProgramSourceUnit {
            path: "Helper.bn".to_owned(),
            source: "FUNCTION render() { Scene/Element/text(element: [] style: [] text: TEXT { Secret child }) }\n"
                .to_owned(),
        }],
        bootstrap_source: "scene: Helper/render()\n".to_owned(),
        bootstrap_support_sources: vec![EmbeddedProgramSourceUnit {
            path: "Helper.bn".to_owned(),
            source: "FUNCTION render() { Scene/Element/text(element: [] style: [] text: TEXT { Secret starter }) }\n"
                .to_owned(),
        }],
        bootstrap_revision: 3,
        capability_profile: ProgramCapabilityProfile::PublicClient,
        ..EmbeddedProgramDescriptor::default()
    };
    let digest = descriptor.source_bundle_digest_v1().unwrap().unwrap();
    let bootstrap_digest = descriptor
        .bootstrap_source_bundle_digest_v1()
        .unwrap()
        .unwrap();
    assert_ne!(digest, bootstrap_digest);
    let artifact = toml::to_string(&descriptor).unwrap();

    assert!(artifact.contains(EMBEDDED_PROGRAM_ARTIFACT_FORMAT));
    assert!(artifact.contains(&digest.to_string()));
    assert!(artifact.contains(&bootstrap_digest.to_string()));
    assert!(artifact.contains("entrypoint = \"RUN.bn\""));
    assert!(artifact.contains("path = \"Helper.bn\""));
    assert!(!artifact.contains("Secret child"));
    assert!(!artifact.contains("Secret starter"));
    assert!(!artifact.contains("source_digest ="));
    assert!(!artifact.contains("bootstrap_source_digest"));
    assert!(toml::from_str::<EmbeddedProgramDescriptor>(&artifact).is_err());
}

#[test]
fn embedded_program_input_accepts_source_bytes_and_rejects_all_caller_digests() {
    let input = r#"
source = "scene: []"
revision = 1
capability_profile = "public_client"
"#;
    let descriptor: EmbeddedProgramDescriptor = toml::from_str(input).unwrap();
    assert_eq!(descriptor.source, "scene: []");

    for legacy in [
        "source_digest = \"stale\"\n",
        "bootstrap_source_digest = \"stale\"\n",
        "source_bundle_digest_v1 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
        "bootstrap_source_bundle_digest_v1 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
    ] {
        assert!(
            toml::from_str::<EmbeddedProgramDescriptor>(&format!("{input}{legacy}")).is_err(),
            "accepted caller-controlled identity field {legacy}"
        );
    }

    let support_legacy = r#"
source = "scene: []"
revision = 1
capability_profile = "public_client"

[[support_sources]]
path = "Helper.bn"
source = "FUNCTION helper() { [] }"
source_digest = "stale"
"#;
    assert!(toml::from_str::<EmbeddedProgramDescriptor>(support_legacy).is_err());
    let support_canonical = support_legacy.replace(
        "source_digest = \"stale\"",
        "source_bundle_digest_v1 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
    );
    assert!(toml::from_str::<EmbeddedProgramDescriptor>(&support_canonical).is_err());
}
