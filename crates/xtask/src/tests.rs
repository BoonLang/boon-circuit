use super::*;

// Xtask verifier tests are grouped by report/contract area while staying in this module for private helper access.
include!("tests/bytes_plan_reports.rs");
include!("tests/cells_visible_click.rs");
include!("tests/freshness_and_identity.rs");
include!("tests/generic_xtask_core.rs");
include!("tests/native_handoff_manifest.rs");
include!("tests/native_visible_preview_surface.rs");
include!("tests/present_floor_and_product_path.rs");
include!("tests/proof_and_stale_path.rs");
include!("tests/refresh_queue.rs");
include!("tests/scenario_integrity.rs");
