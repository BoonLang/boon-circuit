use boon_document::{
    DocumentFrame, DocumentNode, DocumentNodeId, DocumentNodeKind, ScrollRootId, StyleValue,
    TextValue,
};
use boon_document_model::{ScrollState, SourceBinding, SourceBindingId};

pub const DEV_PREVIOUS: &str = "dev.previous";
pub const DEV_NEXT: &str = "dev.next";
pub const DEV_RUN: &str = "dev.run";
pub const DEV_RESET: &str = "dev.reset";
pub const DEV_TEST: &str = "dev.test";
pub const DEV_EDITOR: &str = "dev.editor";

pub struct DevFrameState<'a> {
    pub example_label: &'a str,
    pub source_path: &'a str,
    pub source: &'a str,
    pub editor_scroll: f32,
    pub status: &'a str,
    pub perf: &'a str,
}

pub fn dev_frame(state: DevFrameState<'_>) -> DocumentFrame {
    let mut frame = DocumentFrame::empty("dev.root");
    style_root(
        frame.nodes.get_mut(&frame.root).expect("dev root"),
        "#f3f5f7",
    );

    let mut toolbar = node("dev.toolbar", DocumentNodeKind::Row, Some("dev.root"));
    toolbar.style.insert("height".into(), number(52.0));
    toolbar.style.insert("width".into(), text("Fill"));
    toolbar.style.insert("gap".into(), number(8.0));
    toolbar.style.insert("padding".into(), number(8.0));
    toolbar.style.insert("background".into(), text("#ffffff"));
    add(&mut frame, "dev.root", toolbar);

    for (id, label, width) in [
        (DEV_TEST, "TEST", 78.0),
        (DEV_PREVIOUS, "Previous", 92.0),
        (DEV_NEXT, "Next", 72.0),
        (DEV_RUN, "Run", 70.0),
        (DEV_RESET, "Reset", 74.0),
    ] {
        add(&mut frame, "dev.toolbar", button(id, label, width));
    }

    let mut title = node("dev.example", DocumentNodeKind::Text, Some("dev.toolbar"));
    title.text = Some(TextValue {
        text: format!("{}  {}", state.example_label, state.source_path),
    });
    title.style.insert("width".into(), text("Fill"));
    title.style.insert("height".into(), number(34.0));
    title.style.insert("font_size".into(), number(15.0));
    title.style.insert("color".into(), text("#20252b"));
    add(&mut frame, "dev.toolbar", title);

    let mut editor = node(DEV_EDITOR, DocumentNodeKind::ScrollRoot, Some("dev.root"));
    editor.style.insert("width".into(), text("Fill"));
    editor.style.insert("height".into(), text("Fill"));
    editor.style.insert("padding".into(), number(16.0));
    editor.style.insert("background".into(), text("#171a1f"));
    editor.style.insert("color".into(), text("#e8edf2"));
    editor.style.insert("font_size".into(), number(14.0));
    editor.style.insert("border_width".into(), number(0.0));
    editor.style.insert("border".into(), text("#4c8bf5"));
    editor
        .style
        .insert("__focus_border_width".into(), number(2.0));
    editor
        .style
        .insert("__focus_border".into(), text("#4c8bf5"));
    editor.text = Some(TextValue {
        text: state.source.to_owned(),
    });
    editor.scroll = Some(ScrollState {
        x: 0.0,
        y: state.editor_scroll,
    });
    editor.source_bindings.push(binding(DEV_EDITOR, "edit"));
    frame.scroll_roots.insert(
        ScrollRootId(DEV_EDITOR.to_owned()),
        ScrollState {
            x: 0.0,
            y: state.editor_scroll,
        },
    );
    add(&mut frame, "dev.root", editor);

    let mut footer = node("dev.footer", DocumentNodeKind::Row, Some("dev.root"));
    footer.style.insert("height".into(), number(28.0));
    footer.style.insert("width".into(), text("Fill"));
    footer.style.insert("gap".into(), number(16.0));
    footer.style.insert("padding".into(), number(5.0));
    footer.style.insert("background".into(), text("#ffffff"));
    add(&mut frame, "dev.root", footer);
    add(&mut frame, "dev.footer", label("dev.status", state.status));
    add(&mut frame, "dev.footer", label("dev.perf", state.perf));

    frame
}

pub fn message_frame(title: &str, message: &str, background: &str) -> DocumentFrame {
    let mut frame = DocumentFrame::empty("message.root");
    style_root(
        frame.nodes.get_mut(&frame.root).expect("message root"),
        background,
    );
    let mut stack = node(
        "message.stack",
        DocumentNodeKind::Stack,
        Some("message.root"),
    );
    stack.style.insert("width".into(), text("Fill"));
    stack.style.insert("height".into(), text("Fill"));
    stack.style.insert("padding".into(), number(32.0));
    stack.style.insert("gap".into(), number(14.0));
    add(&mut frame, "message.root", stack);
    let mut title_node = label("message.title", title);
    title_node.style.insert("font_size".into(), number(28.0));
    title_node.style.insert("height".into(), number(44.0));
    add(&mut frame, "message.stack", title_node);
    let mut body = label("message.body", message);
    body.style.insert("font_size".into(), number(15.0));
    body.style.insert("height".into(), text("Fill"));
    add(&mut frame, "message.stack", body);
    frame
}

fn style_root(root: &mut DocumentNode, background: &str) {
    root.style.insert("width".into(), text("Fill"));
    root.style.insert("height".into(), text("Fill"));
    root.style.insert("background".into(), text(background));
    root.style.insert("color".into(), text("#20252b"));
    root.style.insert("font_size".into(), number(14.0));
}

fn button(id: &str, label: &str, width: f64) -> DocumentNode {
    let mut button = node(id, DocumentNodeKind::Button, Some("dev.toolbar"));
    button.text = Some(TextValue {
        text: label.to_owned(),
    });
    button.style.insert("width".into(), number(width));
    button.style.insert("height".into(), number(34.0));
    button.style.insert("padding".into(), number(8.0));
    button.style.insert("background".into(), text("#eef2f6"));
    button.style.insert("border".into(), text("#b8c2cc"));
    button.style.insert("border_width".into(), number(1.0));
    button.source_bindings.push(binding(id, "press"));
    button
}

fn label(id: &str, value: &str) -> DocumentNode {
    let mut node = node(id, DocumentNodeKind::Text, Some("dev.footer"));
    node.text = Some(TextValue {
        text: value.to_owned(),
    });
    node.style.insert("width".into(), text("Fill"));
    node.style.insert("height".into(), number(20.0));
    node.style.insert("font_size".into(), number(13.0));
    node.style.insert("color".into(), text("#3c4650"));
    node
}

fn node(id: &str, kind: DocumentNodeKind, parent: Option<&str>) -> DocumentNode {
    let mut node = DocumentNode::new(id, kind);
    node.parent = parent.map(|value| DocumentNodeId(value.to_owned()));
    node
}

fn add(frame: &mut DocumentFrame, parent: &str, node: DocumentNode) {
    let id = node.id.clone();
    frame
        .nodes
        .get_mut(&DocumentNodeId(parent.to_owned()))
        .unwrap_or_else(|| panic!("missing parent {parent}"))
        .children
        .push(id.clone());
    frame.nodes.insert(id, node);
}

fn binding(id: &str, intent: &str) -> SourceBinding {
    SourceBinding {
        id: SourceBindingId(format!("binding:{id}")),
        source_path: id.to_owned(),
        intent: intent.to_owned(),
    }
}

fn text(value: &str) -> StyleValue {
    StyleValue::Text(value.to_owned())
}

fn number(value: f64) -> StyleValue {
    StyleValue::Number(value)
}
