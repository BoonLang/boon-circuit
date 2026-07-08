// Included by `../tests.rs`; kept in the parent test module for private renderer helper access.

#[test]
fn bundled_editor_font_applies_calt_ligature_substitutions() {
    let enabled = text_font_features("zero,calt");
    assert_ne!(
        shape_glyph_ids("--", disabled_editor_ligature_features()),
        shape_glyph_ids("--", enabled.clone()),
        "patched JetBrains Mono must substitute dash sequences through calt"
    );
    let raw_pipe = shape_glyph_ids("|>", disabled_editor_ligature_features());
    let shaped_pipe = shape_glyph_ids("|>", enabled);
    assert_eq!(raw_pipe.len(), 2);
    assert_eq!(shaped_pipe.len(), 2);
    assert_ne!(
        raw_pipe, shaped_pipe,
        "patched JetBrains Mono must substitute pipe-forward through calt"
    );
    assert_ne!(
        raw_pipe[0], shaped_pipe[0],
        "pipe-forward must replace the raw bar with the pipe ligature glyph"
    );
    assert_ne!(
        raw_pipe[1], shaped_pipe[1],
        "pipe-forward must replace the raw greater-than with an invisible filler glyph"
    );
}


#[test]
fn rich_editor_spans_shape_pipe_forward_inside_operator_span() {
    let raw_pipe = shape_glyph_ids("|>", disabled_editor_ligature_features());
    let rich_pipe = shape_rich_glyph_ids(&[
        ("0 ", [217, 225, 242, 255], Style::Normal, Weight::NORMAL),
        ("|>", [255, 159, 67, 255], Style::Normal, Weight::BOLD),
        (
            " HOLD",
            [210, 105, 30, 255],
            Style::Italic,
            Weight::EXTRA_BOLD,
        ),
    ]);
    assert!(
        !rich_pipe
            .windows(raw_pipe.len())
            .any(|window| window == raw_pipe)
    );
    assert!(
        rich_pipe.iter().any(|glyph_id| *glyph_id == 1563),
        "rich editor spans must shape |> to the bundled pipe-forward ligature glyph"
    );
}


#[test]
fn styled_editor_spans_keep_dash_ligatures_on_patched_jetbrains_variants() {
    let raw_dash = shape_glyph_ids("--", disabled_editor_ligature_features());
    let styled_dash = shape_rich_glyph_ids(&[(
        "-- comment",
        [119, 136, 153, 255],
        Style::Italic,
        Weight::NORMAL,
    )]);
    assert!(
        !styled_dash
            .windows(raw_dash.len())
            .any(|window| window == raw_dash)
    );
    assert!(
        styled_dash.iter().any(|glyph_id| *glyph_id == 876),
        "italic comment spans must shape -- through the bundled patched JetBrains variant"
    );
}


#[test]
fn styled_editor_punctuation_stays_monospace_across_weights() {
    let punctuation = shape_rich_glyphs(&[
        ("_([", [210, 105, 30, 255], Style::Normal, Weight::BOLD),
        (
            " (([]))",
            [210, 105, 30, 255],
            Style::Italic,
            Weight::EXTRA_BOLD,
        ),
    ]);
    assert_eq!(punctuation.len(), 10);
    let first_advance = punctuation
        .first()
        .map(|(_, advance)| *advance)
        .expect("punctuation should shape");
    assert!(
        punctuation
            .iter()
            .all(|(_, advance)| (*advance - first_advance).abs() < f32::EPSILON),
        "styled punctuation must stay on the bundled monospace JetBrains variants: {punctuation:?}"
    );
}


#[test]
fn asset_refs_are_stable_digest_identities_for_inline_svg_uploads() {
    let url = "data:image/svg+xml;utf8,%3Csvg%3E%3C/svg%3E".to_owned();
    let key = AssetTextureKey {
        asset_ref: RenderAssetRef::inline_svg_data_url(&url, 24, 32),
        url,
        width: 24,
        height: 32,
    };
    let same = key.asset_ref();
    let same_again = key.asset_ref();
    let different_size = AssetTextureKey {
        asset_ref: RenderAssetRef::inline_svg_data_url(&key.url, 25, 32),
        width: 25,
        ..key.clone()
    }
    .asset_ref();

    assert_eq!(same, same_again);
    assert_eq!(same.blob_ref, same_again.blob_ref);
    assert_eq!(same.blob_ref.sha256.len(), 64);
    assert!(same.blob_ref.id.starts_with("blob:sha256:"));
    assert_eq!(same.width, 24);
    assert_eq!(same.height, 32);
    assert_ne!(same.id, different_size.id);
    assert_eq!(same.blob_ref, different_size.blob_ref);
}


#[test]
fn button_text_runs_are_centered_by_default() {
    let mut style = StyleMap::new();
    style.insert("size".to_owned(), StyleValue::Number(16.0));
    style.insert("text_inset".to_owned(), StyleValue::Number(4.0));
    let frame = LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("button".to_owned()),
            kind: DocumentNodeKind::Button,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 120.0,
                height: 40.0,
            },
            text: Some("RUN".to_owned()),
            style,
            focused: false,
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let runs = text_runs(&frame, 320, 120);
    let run = runs.first().expect("button text should render");
    assert_eq!(run.align, TextAlign::Center);
    assert_eq!(run.vertical_align, TextVerticalAlign::Center);

    let mut font_system = editor_font_system();
    let buffer = shape_text_run(&mut font_system, run);
    let line_width = shaped_line_width(&buffer).expect("button label should shape");
    let left = text_left_for_width(run, line_width);
    let top = text_top_for_height(run);
    let paint_left = text_paint_left_for_width(run, line_width);
    let paint_top = text_paint_top_for_height(run);
    assert!(
        left > run.bounds.x + run.text_inset,
        "centered button text should not use the left inset"
    );
    assert_eq!(
        paint_left.fract(),
        0.0,
        "button text should paint on a whole-pixel x origin"
    );
    let line_box_top = text_top_for_parts(
        run.bounds,
        run.line_height,
        run.text_inset,
        run.vertical_align,
    );
    assert!((line_box_top - 30.0).abs() <= 0.5);
    assert!(
        (top - 30.0).abs() <= 0.5,
        "centered glyph paint should use the line-box top without an optical offset, top={top}"
    );
    assert_eq!(
        paint_top, 30.0,
        "centered glyph paint origin should snap to whole pixels"
    );
}


#[test]
fn quarter_turn_text_run_rasterizes_a_centered_rotated_mask() {
    let run = TextRun {
        node: DocumentNodeId("toggle-all".to_owned()),
        font_id: 5,
        paint_id: 3,
        bounds: Rect {
            x: 75.0,
            y: 130.0,
            width: 45.0,
            height: 65.0,
        },
        clip: None,
        text: "❯".to_owned(),
        rich_spans: Vec::new(),
        font_family: "Helvetica Neue, Helvetica, Arial, SansSerif".to_owned(),
        font_style: Style::Normal,
        font_weight: Weight::NORMAL,
        font_features: String::new(),
        text_inset: 0.0,
        text_clip_padding: 0.0,
        color: [148, 148, 148, 255],
        size: 22.0,
        line_height: 27.5,
        align: TextAlign::Center,
        vertical_align: TextVerticalAlign::Center,
        rotate_degrees: 90,
    };
    let mut font_system = editor_font_system();
    let mut swash_cache = SwashCache::new();
    let glyph = rotated_text_glyph_for_run(&run, &mut font_system, &mut swash_cache)
        .expect("rotated chevron should rasterize through the generic custom glyph path");

    assert!(glyph.mask.iter().any(|alpha| *alpha > 0));
    assert!(
        glyph.width > glyph.height,
        "90-degree ❯ should become a wider down-chevron mask"
    );
    assert!(
        (glyph.left - (run.bounds.x + (run.bounds.width - f32::from(glyph.width)) * 0.5)).abs()
            <= 0.5
    );
    assert!(
        (glyph.top - (run.bounds.y + (run.bounds.height - f32::from(glyph.height)) * 0.5)).abs()
            <= 0.5
    );
}


#[test]
fn explicit_button_text_alignment_overrides_center_default() {
    let mut style = StyleMap::new();
    style.insert("size".to_owned(), StyleValue::Number(16.0));
    style.insert("text_inset".to_owned(), StyleValue::Number(4.0));
    style.insert("align".to_owned(), StyleValue::Text("left".to_owned()));
    style.insert(
        "vertical_align".to_owned(),
        StyleValue::Text("top".to_owned()),
    );
    let frame = LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("button".to_owned()),
            kind: DocumentNodeKind::Button,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 120.0,
                height: 40.0,
            },
            text: Some("RUN".to_owned()),
            style,
            focused: false,
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let runs = text_runs(&frame, 320, 120);
    let run = runs.first().expect("button text should render");
    assert_eq!(run.align, TextAlign::Left);
    assert_eq!(run.vertical_align, TextVerticalAlign::Top);
    assert_eq!(text_left_for_width(run, 30.0), 14.0);
    assert_eq!(text_top_for_height(run), 21.0);
    assert_eq!(text_paint_left_for_width(run, 30.0), 14.0);
    assert_eq!(text_paint_top_for_height(run), 21.0);
}


#[test]
fn text_run_signatures_include_line_height() {
    let mut compact_style = StyleMap::new();
    compact_style.insert("size".to_owned(), StyleValue::Number(16.0));
    compact_style.insert("line_height".to_owned(), StyleValue::Number(18.0));
    let mut tall_style = compact_style.clone();
    tall_style.insert("line_height".to_owned(), StyleValue::Number(28.0));
    let frame = |style: StyleMap| LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("line-height-sensitive".to_owned()),
            kind: DocumentNodeKind::Text,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 160.0,
                height: 40.0,
            },
            text: Some("Line height".to_owned()),
            style,
            focused: false,
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let compact_run = text_runs(&frame(compact_style), 320, 120)
        .pop()
        .expect("compact text should render");
    let tall_run = text_runs(&frame(tall_style), 320, 120)
        .pop()
        .expect("tall text should render");

    assert_ne!(
        TextRunSignature::from_run(&compact_run),
        TextRunSignature::from_run(&tall_run),
        "changing only line_height must invalidate shaped text buffers"
    );
    assert_ne!(
        TextRunPlacementSignature::from_run(&compact_run),
        TextRunPlacementSignature::from_run(&tall_run),
        "changing only line_height must invalidate placement caches"
    );
}


#[test]
fn text_cache_reuse_counts_report_hits_misses_and_evictions() {
    let run_for_text = |text: &str| {
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId(format!("text-{text}")),
                kind: DocumentNodeKind::Text,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 160.0,
                    height: 40.0,
                },
                text: Some(text.to_owned()),
                style: StyleMap::new(),
                focused: false,
                style_identity: test_style_identity(),
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            materialization: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        TextRunSignature::from_run(
            &text_runs(&frame, 320, 120)
                .pop()
                .expect("text should render"),
        )
    };
    let a = run_for_text("A");
    let b = run_for_text("B");
    let c = run_for_text("C");

    assert_eq!(
        text_cache_reuse_counts(&[a.clone(), b.clone()], &[b.clone(), c.clone()]),
        (1, 1, 1)
    );
    assert_eq!(
        text_cache_reuse_counts(&[a.clone(), a.clone()], &[a.clone(), a.clone(), a]),
        (2, 1, 0)
    );
}


#[test]
fn state_style_values_apply_hover_and_focus_variants() {
    let mut style = StyleMap::new();
    style.insert(
        "color".to_owned(),
        StyleValue::Text("Oklch[lightness:0.20]".to_owned()),
    );
    style.insert(
        "__hover_color".to_owned(),
        StyleValue::Text("Oklch[lightness:0.70]".to_owned()),
    );
    style.insert(
        "__focus_color".to_owned(),
        StyleValue::Text("Oklch[lightness:0.50]".to_owned()),
    );
    style.insert("underline_if".to_owned(), StyleValue::Bool(false));
    style.insert("__hover_underline_if".to_owned(), StyleValue::Bool(true));
    style.insert("__hover".to_owned(), StyleValue::Bool(true));
    assert_eq!(
        state_style_value(&style, "underline_if"),
        Some(&StyleValue::Bool(true))
    );
    let hover_color = style_color_u8(&style, "color").expect("hover color");

    style.insert("__hover".to_owned(), StyleValue::Bool(false));
    style.insert("__focused".to_owned(), StyleValue::Bool(true));
    assert_eq!(
        state_style_value(&style, "underline_if"),
        Some(&StyleValue::Bool(false))
    );
    let focus_color = style_color_u8(&style, "color").expect("focus color");
    assert_ne!(hover_color, focus_color);

    style.insert("__focused".to_owned(), StyleValue::Bool(false));
    let base_color = style_color_u8(&style, "color").expect("base color");
    assert_ne!(hover_color, base_color);
    assert_ne!(focus_color, base_color);

    let frame_for_style = |style: StyleMap| LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("hover-button".to_owned()),
            kind: DocumentNodeKind::Button,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 120.0,
                height: 32.0,
            },
            text: Some("Clear completed".to_owned()),
            style,
            focused: false,
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let mut base_style = StyleMap::new();
    base_style.insert(
        "color".to_owned(),
        StyleValue::Text("Oklch[lightness:0.20]".to_owned()),
    );
    base_style.insert(
        "__hover_color".to_owned(),
        StyleValue::Text("Oklch[lightness:0.70]".to_owned()),
    );
    base_style.insert("underline_if".to_owned(), StyleValue::Bool(false));
    base_style.insert("__hover_underline_if".to_owned(), StyleValue::Bool(true));
    let base_frame = frame_for_style(base_style.clone());
    let base_run = text_runs(&base_frame, 320, 120)
        .pop()
        .expect("base text should render");

    base_style.insert("__hover".to_owned(), StyleValue::Bool(true));
    let hover_frame = frame_for_style(base_style);
    let hover_run = text_runs(&hover_frame, 320, 120)
        .pop()
        .expect("hover text should render");
    assert_ne!(
        hover_run.color, base_run.color,
        "__hover_color must affect the rendered text run, not only the style lookup helper"
    );
}


#[test]
fn editor_type_hints_render_as_virtual_text_without_changing_source_run() {
    let mut style = StyleMap::new();
    style.insert("size".to_owned(), StyleValue::Number(14.0));
    style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
    style.insert(
        "font".to_owned(),
        StyleValue::Text("JetBrains Mono".to_owned()),
    );
    style.insert(
        "font_features".to_owned(),
        StyleValue::Text("zero,calt".to_owned()),
    );
    style.insert(
        "syntax_spans_json".to_owned(),
        StyleValue::Text(
            r##"[{"text":"count","source_text":"count","color":"#eeeeee"}]"##.to_owned(),
        ),
    );
    style.insert(
        "editor_type_hints_json".to_owned(),
        StyleValue::Text(
            r#"[{"anchor_column":6,"compact_label":"Number","line":1,"start":0,"end":5,"category":"definition","detail_label":"Number"}]"#
                .to_owned(),
        ),
    );
    let frame = LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("dev-code-editor-line-text-1".to_owned()),
            kind: DocumentNodeKind::Text,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 260.0,
                height: 22.0,
            },
            text: Some("count".to_owned()),
            style,
            focused: false,
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let runs = text_runs(&frame, 320, 120);
    assert!(runs.iter().any(|run| run.text == "count"));
    assert!(runs.iter().any(|run| run.text == ": Number"));
    let source_run = runs
        .iter()
        .find(|run| run.text == "count")
        .expect("source run should remain source-exact");
    assert_eq!(source_run.node.0, "dev-code-editor-line-text-1");
}


#[test]
fn editor_type_hints_do_not_render_sliced_runs_outside_source_bounds() {
    let mut style = StyleMap::new();
    style.insert("size".to_owned(), StyleValue::Number(14.0));
    style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
    style.insert(
        "font".to_owned(),
        StyleValue::Text("JetBrains Mono".to_owned()),
    );
    style.insert(
        "font_features".to_owned(),
        StyleValue::Text("zero,calt".to_owned()),
    );
    style.insert(
        "editor_type_hints_json".to_owned(),
        StyleValue::Text(
            r#"[{"anchor_column":27,"compact_label":"Number","line":1,"start":0,"end":26,"category":"definition","detail_label":"Number"}]"#
                .to_owned(),
        ),
    );
    let frame = LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("dev-code-editor-line-text-1".to_owned()),
            kind: DocumentNodeKind::Text,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 42.0,
                height: 22.0,
            },
            text: Some("abcdefghijklmnopqrstuvwxyz".to_owned()),
            style,
            focused: false,
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let runs = text_runs(&frame, 320, 120);

    assert!(
        runs.iter()
            .any(|run| run.text == "abcdefghijklmnopqrstuvwxyz")
    );
    assert!(
        !runs.iter().any(|run| run.text == ": Number"),
        "off-row virtual type hints should be skipped instead of rendered as clipped slices"
    );
}


#[test]
fn text_runs_shape_as_single_unwrapped_lines_when_bounds_are_narrow() {
    let mut style = StyleMap::new();
    style.insert("size".to_owned(), StyleValue::Number(14.0));
    style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
    style.insert(
        "font".to_owned(),
        StyleValue::Text("JetBrains Mono".to_owned()),
    );
    style.insert(
        "font_features".to_owned(),
        StyleValue::Text("zero,calt".to_owned()),
    );
    let frame = LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("dev-code-editor-line-text-1".to_owned()),
            kind: DocumentNodeKind::Text,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 120.0,
                height: 22.0,
            },
            text: Some("active_count == 0 |> Bool/and(completed_count > 0)".to_owned()),
            style,
            focused: false,
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let runs = text_runs(&frame, 640, 160);
    let run = runs.first().expect("text run should render");
    let mut font_system = editor_font_system();
    let buffer = shape_text_run(&mut font_system, run);
    let line_count = buffer.layout_runs().count();
    let line_width = shaped_line_width(&buffer).expect("text should shape");

    assert_eq!(line_count, 1);
    assert!(
        line_width > run.bounds.width,
        "logical text width should exceed visible clip bounds instead of wrapping"
    );
}


#[test]
fn text_inputs_center_text_and_place_caret_from_text_metrics() {
    let mut style = StyleMap::new();
    style.insert("size".to_owned(), StyleValue::Number(12.0));
    style.insert("text_inset".to_owned(), StyleValue::Number(4.0));
    style.insert("caret_column".to_owned(), StyleValue::Number(1.0));
    style.insert("caret_visible".to_owned(), StyleValue::Bool(true));
    let frame = LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("input".to_owned()),
            kind: DocumentNodeKind::TextInput,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 90.0,
                height: 24.0,
            },
            text: Some("30".to_owned()),
            style,
            focused: true,
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let runs = text_runs(&frame, 320, 120);
    let run = runs.first().expect("focused input text should render");
    assert_eq!(run.vertical_align, TextVerticalAlign::Center);
    let line_box_top = text_top_for_parts(
        run.bounds,
        run.line_height,
        run.text_inset,
        run.vertical_align,
    );
    assert!(
        (line_box_top - 24.5).abs() <= 0.5,
        "input line box top should stay geometrically centered"
    );
    assert!(
        (text_top_for_height(run) - 24.5).abs() <= 0.5,
        "input glyph paint should use the same line-box top as placeholder text"
    );
    assert_eq!(
        text_paint_top_for_height(run),
        25.0,
        "input glyph paint origin should snap to whole pixels for sharper text"
    );
}


#[test]
fn unfocused_empty_text_inputs_render_placeholder_text() {
    let mut style = StyleMap::new();
    style.insert("size".to_owned(), StyleValue::Number(24.0));
    style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
    style.insert(
        "placeholder".to_owned(),
        StyleValue::Text("What needs to be done?".to_owned()),
    );
    style.insert(
        "placeholder_color".to_owned(),
        StyleValue::Text("Oklch[lightness:0.68]".to_owned()),
    );
    style.insert(
        "placeholder_style".to_owned(),
        StyleValue::Text("Italic".to_owned()),
    );
    let frame = LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("input".to_owned()),
            kind: DocumentNodeKind::TextInput,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 320.0,
                height: 65.0,
            },
            text: None,
            style,
            focused: false,
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };

    let run = text_runs(&frame, 640, 160)
        .into_iter()
        .find(|run| run.node.0 == "input")
        .expect("unfocused empty text input should still render placeholder text");
    assert_eq!(run.text, "What needs to be done?");
    assert_eq!(run.font_style, Style::Italic);
    assert_eq!(
        run.color,
        parse_oklch_color("Oklch[lightness:0.68]").expect("placeholder color should parse")
    );
    assert!(
        run.color[0] <= 190 && run.color[1] <= 190 && run.color[2] <= 190,
        "placeholder text should be readable gray, not washed-out near-white: {:?}",
        run.color
    );
}

fn max_shaped_word_gap(buffer: &Buffer) -> Option<f32> {
    let mut previous_right = None;
    let mut max_gap = 0.0_f32;
    for glyph in buffer
        .layout_runs()
        .next()?
        .glyphs
        .iter()
        .filter(|glyph| glyph.w > 0.0)
    {
        let left = glyph.x;
        if let Some(previous_right) = previous_right {
            max_gap = max_gap.max(left - previous_right);
        }
        previous_right = Some(left + glyph.w);
    }
    Some(max_gap)
}


#[test]
fn wide_todo_text_controls_shape_with_natural_word_spacing() {
    for (
        font_family,
        title_weight,
        placeholder_weight,
        placeholder_style,
        title_size,
        placeholder_size,
        title_text,
    ) in [
        (
            "Helvetica Neue, Helvetica, Arial, SansSerif",
            Weight(300),
            Weight(300),
            "Italic",
            24.0,
            24.0,
            "Read documentation",
        ),
        (
            "Segoe UI, Roboto, Helvetica, Arial, SansSerif",
            Weight(300),
            Weight(300),
            "Italic",
            25.0,
            25.0,
            "Read documentation",
        ),
        (
            "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, Liberation Mono, monospace",
            Weight(800),
            Weight::NORMAL,
            "Normal",
            23.0,
            22.0,
            "Read documentation",
        ),
    ] {
        let mut input_style = StyleMap::new();
        input_style.insert("size".to_owned(), StyleValue::Number(title_size));
        input_style.insert("line_height".to_owned(), StyleValue::Number(33.6));
        input_style.insert("text_inset".to_owned(), StyleValue::Number(6.0));
        input_style.insert("font".to_owned(), StyleValue::Text(font_family.to_owned()));
        input_style.insert(
            "weight".to_owned(),
            StyleValue::Number(f64::from(title_weight.0)),
        );
        input_style.insert(
            "placeholder_size".to_owned(),
            StyleValue::Number(placeholder_size),
        );
        input_style.insert(
            "placeholder_weight".to_owned(),
            StyleValue::Number(f64::from(placeholder_weight.0)),
        );
        input_style.insert(
            "placeholder_font".to_owned(),
            StyleValue::Text(font_family.to_owned()),
        );
        input_style.insert(
            "placeholder".to_owned(),
            StyleValue::Text("What needs to be done?".to_owned()),
        );
        input_style.insert(
            "placeholder_style".to_owned(),
            StyleValue::Text(placeholder_style.to_owned()),
        );

        let mut title_style = StyleMap::new();
        title_style.insert("size".to_owned(), StyleValue::Number(title_size));
        title_style.insert("line_height".to_owned(), StyleValue::Number(33.6));
        title_style.insert("text_inset".to_owned(), StyleValue::Number(0.0));
        title_style.insert("font".to_owned(), StyleValue::Text(font_family.to_owned()));
        title_style.insert(
            "weight".to_owned(),
            StyleValue::Number(f64::from(title_weight.0)),
        );

        let frame = LayoutFrame {
            display_list: vec![
                DisplayItem {
                    node: DocumentNodeId("new-todo-input".to_owned()),
                    kind: DocumentNodeKind::TextInput,
                    bounds: Rect {
                        x: 55.0,
                        y: 151.0,
                        width: 495.0,
                        height: 65.0,
                    },
                    text: None,
                    style: input_style,
                    focused: false,
                    style_identity: test_style_identity(),
                },
                DisplayItem {
                    node: DocumentNodeId("active-title".to_owned()),
                    kind: DocumentNodeKind::Text,
                    bounds: Rect {
                        x: 109.0,
                        y: 217.0,
                        width: 441.0,
                        height: 65.0,
                    },
                    text: Some(title_text.to_owned()),
                    style: title_style,
                    focused: false,
                    style_identity: test_style_identity(),
                },
            ],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            materialization: Vec::new(),
            metrics: LayoutMetrics::default(),
        };

        let mut font_system = editor_font_system();
        for run in text_runs(&frame, 640, 360) {
            let buffer = shape_text_run(&mut font_system, &run);
            let line_width = shaped_line_width(&buffer).expect("todo text should shape");
            let max_gap = max_shaped_word_gap(&buffer).expect("todo glyphs should shape");
            assert!(
                line_width < run.bounds.width * 0.75,
                "`{}` in `{}` should keep natural word spacing instead of expanding to its whole control width: line_width={line_width}, bounds={:?}",
                run.text,
                font_family,
                run.bounds
            );
            let run_size = if run.text == "What needs to be done?" {
                placeholder_size
            } else {
                title_size
            };
            assert!(
                max_gap <= (run_size as f32) * 0.65,
                "`{}` in `{}` should not shape with stretched spaces: max_gap={max_gap}, size={run_size}",
                run.text,
                font_family
            );
        }
    }
}


#[test]
fn text_clip_padding_expands_text_bounds_on_all_edges() {
    let run = TextRun {
        node: DocumentNodeId("accented-footer-link".to_owned()),
        font_id: 5,
        paint_id: 3,
        bounds: Rect {
            x: 10.0,
            y: 20.0,
            width: 40.0,
            height: 10.0,
        },
        clip: None,
        text: "Kavík".to_owned(),
        rich_spans: Vec::new(),
        font_family: "Nimbus Sans".to_owned(),
        font_style: Style::Normal,
        font_weight: Weight::NORMAL,
        font_features: String::new(),
        text_inset: 0.0,
        text_clip_padding: 3.0,
        color: [90, 90, 90, 255],
        size: 11.0,
        line_height: 15.0,
        align: TextAlign::Left,
        vertical_align: TextVerticalAlign::Center,
        rotate_degrees: 0,
    };

    let bounds = text_bounds(&run, 100, 100);
    assert_eq!(bounds.left, 7);
    assert_eq!(bounds.top, 17);
    assert_eq!(bounds.right, 53);
    assert_eq!(bounds.bottom, 33);
}


#[test]
fn text_input_placeholder_and_value_share_line_box_top() {
    let mut style = StyleMap::new();
    style.insert("size".to_owned(), StyleValue::Number(24.0));
    style.insert("line_height".to_owned(), StyleValue::Number(33.6));
    style.insert("text_inset".to_owned(), StyleValue::Number(6.0));
    style.insert(
        "placeholder".to_owned(),
        StyleValue::Text("What needs to be done?".to_owned()),
    );
    let frame_for_text = |text: Option<&str>| LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("new-todo-input".to_owned()),
            kind: DocumentNodeKind::TextInput,
            bounds: Rect {
                x: 55.0,
                y: 151.0,
                width: 495.0,
                height: 65.0,
            },
            text: text.map(str::to_owned),
            style: style.clone(),
            focused: text.is_some(),
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    };
    let placeholder_run = text_runs(&frame_for_text(None), 640, 240)
        .pop()
        .expect("placeholder should render");
    let value_run = text_runs(&frame_for_text(Some("abc")), 640, 240)
        .pop()
        .expect("value should render");

    assert_eq!(
        text_paint_top_for_height(&placeholder_run),
        text_paint_top_for_height(&value_run),
        "placeholder and real text should use one vertical line-box calculation"
    );
}


#[test]
fn rich_text_spans_preserve_exact_line_text() {
    let mut style = StyleMap::new();
    style.insert(
        "syntax_spans_json".to_owned(),
        StyleValue::RichTextSpans(vec![
            StyleRichTextSpan {
                text: "SOURCE".to_owned(),
                source_text: None,
                color: Some("#D2691E".to_owned()),
                font_weight: Some("800".to_owned()),
                font_style: Some("italic".to_owned()),
            },
            StyleRichTextSpan {
                text: " ".to_owned(),
                source_text: None,
                color: Some("#d9e1f2".to_owned()),
                font_weight: None,
                font_style: None,
            },
            StyleRichTextSpan {
                text: "]".to_owned(),
                source_text: None,
                color: Some("#D2691E".to_owned()),
                font_weight: Some("700".to_owned()),
                font_style: None,
            },
        ]),
    );

    let spans = rich_text_spans(&style, "SOURCE ]", [217, 225, 242, 255]);
    assert_eq!(
        spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<Vec<_>>(),
        vec!["SOURCE", " ", "]"]
    );
    assert!(rich_text_spans(&style, "SOURCE]", [217, 225, 242, 255]).is_empty());
}


#[test]
fn rich_text_spans_preserve_pipe_forward_source_text() {
    let mut style = StyleMap::new();
    style.insert(
        "syntax_spans_json".to_owned(),
        StyleValue::Text(
            r##"[{"text":"|>","source_text":"|>","color":"#ff9f43","font_weight":"600","font_style":null}]"##
                .to_owned(),
        ),
    );

    let spans = rich_text_spans(&style, "|>", [255, 159, 67, 255]);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "|>");
    assert!(rich_text_spans(&style, "\u{276F} ", [255, 159, 67, 255]).is_empty());
}
