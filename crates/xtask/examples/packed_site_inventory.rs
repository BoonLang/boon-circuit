#[path = "../src/packed_site_inventory.rs"]
mod packed_site_inventory;

use std::path::Path;

fn main() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("xtask workspace root");
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if let Err(error) = packed_site_inventory::run_cli(&workspace, &args) {
        eprintln!("packed-site-inventory: {error}");
        std::process::exit(1);
    }
}
