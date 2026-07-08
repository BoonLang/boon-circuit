// Cells visible-click verifier tests are grouped by contract area while staying
// in one module so private report builders remain crate-local.
include!("cells_visible_click/retained_update_and_proof_isolation.rs");
include!("cells_visible_click/fixtures.rs");
include!("cells_visible_click/product_lane_contracts.rs");
include!("cells_visible_click/product_patch_contracts.rs");
include!("cells_visible_click/product_commit_matching.rs");
include!("cells_visible_click/handoff_and_visual_proof.rs");
include!("cells_visible_click/formula_probe.rs");
