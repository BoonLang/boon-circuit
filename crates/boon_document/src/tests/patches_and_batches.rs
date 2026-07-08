// Document patch and batch tests are grouped by invariant family while staying
// inside the document cfg(test) module for private index helpers.
include!("patches_and_batches/patch_commit_and_identity.rs");
include!("patches_and_batches/derived_and_retained_layout.rs");
include!("patches_and_batches/bindings_and_style.rs");
include!("patches_and_batches/frame_batches.rs");
include!("patches_and_batches/structural_materialization.rs");
