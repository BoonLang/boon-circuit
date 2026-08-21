use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationInstrumentation {
    None,
    ThreadLocalRustGlobal,
}

impl AllocationInstrumentation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ThreadLocalRustGlobal => "thread-local-rust-global-allocator",
        }
    }

    pub const fn records_rust_global_allocations(self) -> bool {
        matches!(self, Self::ThreadLocalRustGlobal)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AllocatorConfiguration {
    pub id: &'static str,
    pub version: &'static str,
    pub upstream_tag: Option<&'static str>,
    pub upstream_commit: Option<&'static str>,
    pub source_sha256: Option<&'static str>,
    pub static_archive_sha256: Option<&'static str>,
    pub linkage: &'static str,
    pub build_options: &'static str,
}

impl AllocatorConfiguration {
    pub const fn system() -> Self {
        Self {
            id: "system",
            version: "platform-libc",
            upstream_tag: None,
            upstream_commit: None,
            source_sha256: None,
            static_archive_sha256: None,
            linkage: "rust-system-global-allocator",
            build_options: "platform-default",
        }
    }

    #[cfg(target_os = "linux")]
    pub const fn mimalloc_3_5() -> Self {
        Self {
            id: "microsoft-mimalloc",
            version: "3.5.0",
            upstream_tag: Some("v3.5.0"),
            upstream_commit: Some("18b08671c9302247bfb682286e6bf3cc1773f801"),
            source_sha256: Some(env!("BOON_MIMALLOC_SOURCE_SHA256")),
            static_archive_sha256: Some(env!("BOON_MIMALLOC_ARCHIVE_SHA256")),
            linkage: "static-rust-global-allocator-no-libc-override",
            build_options: "release;MI_STATIC_LIB=1;MI_BUILD_RELEASE=1;MI_OVERRIDE=0;MI_SECURE=0;MI_GUARDED=0;MI_TRACK=off;MI_OPT_ARCH=0;MI_OPT_SIMD=0",
        }
    }
}

thread_local! {
    // Normative compiler evidence is single-threaded. This scope is encoded in
    // every evidence report and worker-enabled compilation remains fail-closed
    // until counters are deterministically aggregated across all workers.
    static ALLOCATION_CALLS: Cell<u64> = const { Cell::new(0) };
    static ALLOCATED_BYTES: Cell<u64> = const { Cell::new(0) };
    static DEALLOCATION_CALLS: Cell<u64> = const { Cell::new(0) };
    static DEALLOCATED_BYTES: Cell<u64> = const { Cell::new(0) };
}

#[inline]
fn add_thread_counter(counter: &'static std::thread::LocalKey<Cell<u64>>, amount: u64) {
    let _ = counter.try_with(|value| value.set(value.get().saturating_add(amount)));
}

fn reset_thread_counter(counter: &'static std::thread::LocalKey<Cell<u64>>) {
    counter.set(0);
}

fn thread_counter(counter: &'static std::thread::LocalKey<Cell<u64>>) -> u64 {
    counter.get()
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CompilerAllocationCounters {
    pub allocation_calls: u64,
    pub allocated_bytes: u64,
    pub deallocation_calls: u64,
    pub deallocated_bytes: u64,
}

pub(crate) fn reset_compiler_allocation_counters() {
    reset_thread_counter(&ALLOCATION_CALLS);
    reset_thread_counter(&ALLOCATED_BYTES);
    reset_thread_counter(&DEALLOCATION_CALLS);
    reset_thread_counter(&DEALLOCATED_BYTES);
}

pub(crate) fn compiler_allocation_counters() -> CompilerAllocationCounters {
    CompilerAllocationCounters {
        allocation_calls: thread_counter(&ALLOCATION_CALLS),
        allocated_bytes: thread_counter(&ALLOCATED_BYTES),
        deallocation_calls: thread_counter(&DEALLOCATION_CALLS),
        deallocated_bytes: thread_counter(&DEALLOCATED_BYTES),
    }
}

pub struct CountingAllocator<A>(A);

impl<A> CountingAllocator<A> {
    pub const fn new(inner: A) -> Self {
        Self(inner)
    }
}

// SAFETY: every allocator operation is forwarded unchanged to `A`; the
// thread-local counters observe calls and do not alter pointer semantics.
unsafe impl<A: GlobalAlloc> GlobalAlloc for CountingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        add_thread_counter(&ALLOCATION_CALLS, 1);
        add_thread_counter(&ALLOCATED_BYTES, layout.size() as u64);
        unsafe { self.0.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        add_thread_counter(&ALLOCATION_CALLS, 1);
        add_thread_counter(&ALLOCATED_BYTES, layout.size() as u64);
        unsafe { self.0.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        add_thread_counter(&DEALLOCATION_CALLS, 1);
        add_thread_counter(&DEALLOCATED_BYTES, layout.size() as u64);
        unsafe { self.0.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        add_thread_counter(&DEALLOCATION_CALLS, 1);
        add_thread_counter(&DEALLOCATED_BYTES, layout.size() as u64);
        add_thread_counter(&ALLOCATION_CALLS, 1);
        add_thread_counter(&ALLOCATED_BYTES, new_size as u64);
        unsafe { self.0.realloc(ptr, layout, new_size) }
    }
}

#[cfg(target_os = "linux")]
pub struct MimallocAllocator;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn mi_malloc_aligned(size: usize, alignment: usize) -> *mut u8;
    fn mi_zalloc_aligned(size: usize, alignment: usize) -> *mut u8;
    fn mi_realloc_aligned(pointer: *mut u8, new_size: usize, alignment: usize) -> *mut u8;
    fn mi_free(pointer: *mut u8);
    fn mi_version() -> i32;
}

#[cfg(target_os = "linux")]
impl MimallocAllocator {
    pub fn runtime_version() -> i32 {
        unsafe { mi_version() }
    }
}

#[cfg(target_os = "linux")]
unsafe impl GlobalAlloc for MimallocAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { mi_malloc_aligned(layout.size(), layout.align()) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { mi_zalloc_aligned(layout.size(), layout.align()) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe { mi_free(ptr) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { mi_realloc_aligned(ptr, new_size, layout.align()) }
    }
}
