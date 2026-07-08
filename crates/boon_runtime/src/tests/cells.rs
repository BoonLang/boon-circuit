// Cells runtime tests are grouped by behavior area while preserving private
// runtime helpers for currentness, index, and scenario diagnostics.
include!("cells/reports_and_compiled_artifacts.rs");
include!("cells/currentness_project_and_sources.rs");
include!("cells/edit_state_indexes_and_deltas.rs");
include!("cells/live_surfaces_and_field_slots.rs");
include!("cells/formulas_and_helpers.rs");
