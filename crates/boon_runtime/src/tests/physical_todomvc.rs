// Included by `../tests.rs`; kept in the parent test module for private invariant access.

#[test]
fn cells_manifest_source_is_generic_boon_without_spreadsheet_shortcuts() {
    let source = cells_project_source_for_test();
    for forbidden in ["Formula", "Grid", "List/table", "EXAMPLE", "#"] {
        assert!(
            !source.contains(forbidden),
            "Cells manifest-backed source must not contain `{forbidden}`"
        );
    }
    assert!(
        source.contains("List/range(from: 0, to: 2599)"),
        "Cells should generate its official 26x100 model from a generic range"
    );
    assert!(
        source.contains("List/chunk(cells, size: 26"),
        "Cells should derive sheet rows with generic List/chunk"
    );
    let parsed = parse_source("examples/cells.bn", &source).unwrap();
    let ir = lower(&parsed).unwrap();
    assert_eq!(cells_range_from_ir(&ir), Some((0, 2599)));
    assert!(
        parsed.operators.iter().all(|operator| {
            !matches!(
                operator.as_str(),
                "Formula/eval" | "Grid/cells" | "List/table"
            )
        }),
        "Cells should not lower through spreadsheet-specific operators"
    );
}

fn physical_todomvc_project_root_for_test() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/todo_mvc_physical")
}


#[test]
fn physical_todomvc_source_preserves_original_assets_and_declares_theme_divergence() {
    let original_root = Path::new(
        "/home/martinkavik/repos/boon/playground/frontend/src/examples/todo_mvc_physical",
    );
    assert!(
        original_root.exists(),
        "original physical TodoMVC checkout is required for source preservation proof"
    );
    let migrated_root = physical_todomvc_project_root_for_test();
    for relative in [
        "Generated/Assets.bn",
        "assets/icons/checkbox_active.svg",
        "assets/icons/checkbox_completed.svg",
    ] {
        let original = std::fs::read_to_string(original_root.join(relative)).unwrap();
        let migrated = std::fs::read_to_string(migrated_root.join(relative)).unwrap();
        let expected = if relative.ends_with(".bn") {
            original.replace("LINK", "SOURCE")
        } else {
            original
        };
        assert_eq!(
            migrated, expected,
            "{relative} changed beyond LINK to SOURCE"
        );
    }
    let build_source = std::fs::read_to_string(migrated_root.join("BUILD.bn")).unwrap();
    assert!(
        build_source.contains("File/read_bytes()"),
        "BUILD.bn should read icon payloads through BYTES"
    );
    assert!(
        build_source.contains("Bytes/to_text(encoding: Utf8)"),
        "BUILD.bn should decode icon payloads through an explicit BYTES/TEXT boundary"
    );
    for relative in [
        "Theme/Glassmorphism.bn",
        "Theme/Neobrutalism.bn",
        "Theme/Neumorphism.bn",
        "Theme/Professional.bn",
    ] {
        let migrated = std::fs::read_to_string(migrated_root.join(relative)).unwrap();
        assert!(
            migrated.contains("CheckboxGlyph =>"),
            "{relative} should declare the intentional themed checkbox glyph material"
        );
        assert!(
            migrated.contains("checked_border") && migrated.contains("check_color"),
            "{relative} should declare checked checkbox colors for the themed glyph material"
        );
        assert!(
            !migrated.contains("LINK"),
            "{relative} should keep the migration-wide SOURCE spelling"
        );
    }
    assert!(
        migrated_root.join("Theme/Classic.bn").exists(),
        "Classic is a Boon Circuit scene theme added during migration"
    );
    for relative in ["RUN.bn", "Theme/Theme.bn"] {
        let migrated = std::fs::read_to_string(migrated_root.join(relative)).unwrap();
        assert!(
            migrated.contains("Classic"),
            "{relative} should document the intentional Classic theme divergence"
        );
        assert!(
            !migrated.contains("LINK"),
            "{relative} should keep the migration-wide SOURCE spelling"
        );
    }
}


#[test]
fn physical_todomvc_build_assets_match_generated_file() {
    let root = physical_todomvc_project_root_for_test();
    let temp_root =
        std::env::temp_dir().join(format!("boon-physical-build-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_root);
    copy_dir_for_test(&root, &temp_root);
    std::fs::write(temp_root.join("Generated/Assets.bn"), "").unwrap();

    let result = run_project_build_file(&temp_root, Path::new("BUILD.bn"), true).unwrap();
    assert_eq!(result.status, "pass");
    assert_eq!(result.output_file, "./Generated/Assets.bn");
    assert_eq!(result.output_binding, "icon");
    assert_eq!(
        result.input_files,
        vec![
            "assets/icons/checkbox_active.svg".to_owned(),
            "assets/icons/checkbox_completed.svg".to_owned()
        ]
    );
    assert_eq!(result.input_byte_reads.len(), result.input_files.len());
    for read in &result.input_byte_reads {
        assert!(
            result.input_files.iter().any(|path| path == &read.path),
            "byte read evidence should point at an input asset: {read:?}"
        );
        assert_eq!(read.decoded_as, "Utf8");
        assert_eq!(read.sha256.len(), 64);
        assert!(read.byte_len > 0);
    }
    assert!(
        result
            .operator_evidence
            .iter()
            .any(|operator| operator == "File/read_bytes")
    );
    assert!(
        result
            .operator_evidence
            .iter()
            .any(|operator| operator == "Bytes/to_text")
    );
    assert!(
        !result
            .operator_evidence
            .iter()
            .any(|operator| operator == "File/read_text")
    );
    assert_eq!(result.written_files, vec!["Generated/Assets.bn".to_owned()]);

    let generated = std::fs::read_to_string(temp_root.join("Generated/Assets.bn")).unwrap();
    let expected = std::fs::read_to_string(root.join("Generated/Assets.bn")).unwrap();
    assert_eq!(generated, expected);
    assert_eq!(result.output_sha256, sha256_bytes(expected.as_bytes()));

    let verified = generated_output_for_project_build_file(&root, Path::new("BUILD.bn"))
        .expect("checked generated assets should match BUILD.bn output");
    assert_eq!(verified, expected);
    let _ = std::fs::remove_dir_all(&temp_root);
}
