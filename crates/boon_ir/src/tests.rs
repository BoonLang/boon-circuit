use super::*;

// IR tests are grouped by lowering domain while staying in this module for private helper access.
include!("tests/bytes.rs");
include!("tests/cells.rs");
include!("tests/output_roots.rs");
include!("tests/migrations.rs");
include!("tests/sources_and_events.rs");
include!("tests/todomvc.rs");
