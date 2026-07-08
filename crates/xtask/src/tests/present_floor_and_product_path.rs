// Present-floor and product-path verifier tests are grouped by report contract
// area while sharing crate-private report builders.
include!("present_floor_and_product_path/present_floor_contracts.rs");
include!("present_floor_and_product_path/scroll_loop_and_render_hook.rs");
include!("present_floor_and_product_path/adapter_and_product_timing.rs");
include!("present_floor_and_product_path/scroll_budget_contracts.rs");
include!("present_floor_and_product_path/dev_editor_scroll.rs");
