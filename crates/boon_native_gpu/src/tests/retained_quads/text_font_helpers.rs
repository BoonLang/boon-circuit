fn shape_glyph_ids(text: &str, font_features: FontFeatures) -> Vec<u16> {
    shape_glyphs(text, font_features)
        .into_iter()
        .map(|(glyph_id, _)| glyph_id)
        .collect()
}

fn shape_glyphs(text: &str, font_features: FontFeatures) -> Vec<(u16, f32)> {
    let mut font_system = editor_font_system();
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 22.0));
    buffer.set_size(&mut font_system, Some(320.0), Some(32.0));
    buffer.set_text(
        &mut font_system,
        text,
        &Attrs::new()
            .family(Family::Name("JetBrains Mono"))
            .font_features(font_features),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut font_system, false);
    buffer.lines[0]
        .shape_opt()
        .expect("line should be shaped")
        .spans
        .iter()
        .flat_map(|span| span.words.iter())
        .flat_map(|word| word.glyphs.iter())
        .map(|glyph| (glyph.glyph_id, glyph.x_advance))
        .collect()
}

fn shape_rich_glyph_ids(spans: &[(&str, [u8; 4], Style, Weight)]) -> Vec<u16> {
    shape_rich_glyphs(spans)
        .into_iter()
        .map(|(glyph_id, _)| glyph_id)
        .collect()
}

fn shape_rich_glyphs(spans: &[(&str, [u8; 4], Style, Weight)]) -> Vec<(u16, f32)> {
    let mut font_system = editor_font_system();
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 22.0));
    buffer.set_size(&mut font_system, Some(320.0), Some(32.0));
    let default_attrs = text_attrs(
        "JetBrains Mono",
        Style::Normal,
        Weight::NORMAL,
        [217, 225, 242, 255],
        "zero,calt",
    );
    buffer.set_rich_text(
        &mut font_system,
        spans.iter().map(|(text, color, style, weight)| {
            (
                *text,
                text_attrs("JetBrains Mono", *style, *weight, *color, "zero,calt"),
            )
        }),
        &default_attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut font_system, false);
    buffer.lines[0]
        .shape_opt()
        .expect("line should be shaped")
        .spans
        .iter()
        .flat_map(|span| span.words.iter())
        .flat_map(|word| word.glyphs.iter())
        .map(|glyph| (glyph.glyph_id, glyph.x_advance))
        .collect()
}

fn disabled_editor_ligature_features() -> FontFeatures {
    let mut features = FontFeatures::new();
    features.disable(FeatureTag::CONTEXTUAL_ALTERNATES);
    features.disable(FeatureTag::STANDARD_LIGATURES);
    features.disable(FeatureTag::CONTEXTUAL_LIGATURES);
    features
}


