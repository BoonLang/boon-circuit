use super::*;

fn node(id: &str, kind: DocumentNodeKind, parent: Option<&str>) -> DocumentNode {
    let mut node = DocumentNode::new(id, kind);
    node.parent = parent.map(|parent| DocumentNodeId(parent.to_owned()));
    node
}

// Document tests are grouped by model/layout area while staying in this module for private helper access.
include!("tests/indexes_and_identity.rs");
include!("tests/materialization.rs");
include!("tests/patches_and_batches.rs");
include!("tests/semantic_and_accessibility.rs");
include!("tests/structural_document.rs");
