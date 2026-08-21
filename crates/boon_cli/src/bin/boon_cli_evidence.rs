use boon_cli::allocator::CountingAllocator;
#[cfg(target_os = "linux")]
use boon_cli::allocator::MimallocAllocator;
#[cfg(not(target_os = "linux"))]
use std::alloc::System;

#[cfg(target_os = "linux")]
#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator<MimallocAllocator> =
    CountingAllocator::new(MimallocAllocator);

#[cfg(not(target_os = "linux"))]
#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[cfg(target_os = "linux")]
fn main() {
    boon_cli::main_entry(boon_cli::mimalloc_evidence_configuration());
}

#[cfg(not(target_os = "linux"))]
fn main() {
    boon_cli::main_entry(boon_cli::system_evidence_configuration());
}
