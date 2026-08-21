#[cfg(target_os = "linux")]
use boon_cli::allocator::MimallocAllocator;

#[cfg(target_os = "linux")]
#[global_allocator]
static GLOBAL_ALLOCATOR: MimallocAllocator = MimallocAllocator;

#[cfg(target_os = "linux")]
fn main() {
    boon_cli::main_entry(boon_cli::mimalloc_product_configuration());
}

#[cfg(not(target_os = "linux"))]
fn main() {
    boon_cli::main_entry(boon_cli::system_product_configuration());
}
