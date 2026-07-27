use crate::report::AllocatorEvidence;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static INTERVAL_ACTIVE: AtomicBool = AtomicBool::new(false);
static ALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static ZEROED_ALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static REALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static DEALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static REQUESTED_ALLOCATION_BYTES: AtomicU64 = AtomicU64::new(0);
static REQUESTED_REALLOCATION_BYTES: AtomicU64 = AtomicU64::new(0);
static REQUESTED_FREED_BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE_REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);
static INTERVAL_LIVE_START: AtomicU64 = AtomicU64::new(0);
static INTERVAL_LIVE_PEAK: AtomicU64 = AtomicU64::new(0);

pub struct CountingSystem;

unsafe impl GlobalAlloc for CountingSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size() as u64, false);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size() as u64, true);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        record_deallocation(layout.size() as u64);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(pointer, layout, new_size) };
        if !next.is_null() {
            record_reallocation(layout.size() as u64, new_size as u64);
        }
        next
    }
}

pub struct AllocationInterval {
    finished: bool,
}

impl AllocationInterval {
    pub fn begin() -> Result<Self, &'static str> {
        if INTERVAL_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("allocator measurement intervals may not overlap");
        }
        ALLOCATION_CALLS.store(0, Ordering::Release);
        ZEROED_ALLOCATION_CALLS.store(0, Ordering::Release);
        REALLOCATION_CALLS.store(0, Ordering::Release);
        DEALLOCATION_CALLS.store(0, Ordering::Release);
        REQUESTED_ALLOCATION_BYTES.store(0, Ordering::Release);
        REQUESTED_REALLOCATION_BYTES.store(0, Ordering::Release);
        REQUESTED_FREED_BYTES.store(0, Ordering::Release);
        let live = LIVE_REQUESTED_BYTES.load(Ordering::Acquire);
        INTERVAL_LIVE_START.store(live, Ordering::Release);
        INTERVAL_LIVE_PEAK.store(live, Ordering::Release);
        Ok(Self { finished: false })
    }

    pub fn finish(mut self) -> AllocatorEvidence {
        let evidence = finish_interval();
        self.finished = true;
        evidence
    }
}

impl Drop for AllocationInterval {
    fn drop(&mut self) {
        if !self.finished {
            INTERVAL_ACTIVE.store(false, Ordering::Release);
        }
    }
}

pub fn live_requested_bytes() -> u64 {
    LIVE_REQUESTED_BYTES.load(Ordering::Acquire)
}

fn finish_interval() -> AllocatorEvidence {
    INTERVAL_ACTIVE.store(false, Ordering::Release);
    AllocatorEvidence {
        allocation_calls: ALLOCATION_CALLS.load(Ordering::Acquire),
        zeroed_allocation_calls: ZEROED_ALLOCATION_CALLS.load(Ordering::Acquire),
        reallocation_calls: REALLOCATION_CALLS.load(Ordering::Acquire),
        deallocation_calls: DEALLOCATION_CALLS.load(Ordering::Acquire),
        requested_allocation_bytes: REQUESTED_ALLOCATION_BYTES.load(Ordering::Acquire),
        requested_reallocation_bytes: REQUESTED_REALLOCATION_BYTES.load(Ordering::Acquire),
        requested_freed_bytes: REQUESTED_FREED_BYTES.load(Ordering::Acquire),
        live_requested_bytes_start: INTERVAL_LIVE_START.load(Ordering::Acquire),
        live_requested_bytes_end: LIVE_REQUESTED_BYTES.load(Ordering::Acquire),
        peak_live_requested_bytes: INTERVAL_LIVE_PEAK.load(Ordering::Acquire),
    }
}

fn record_allocation(bytes: u64, zeroed: bool) {
    let live = LIVE_REQUESTED_BYTES
        .fetch_add(bytes, Ordering::AcqRel)
        .saturating_add(bytes);
    if INTERVAL_ACTIVE.load(Ordering::Acquire) {
        if zeroed {
            ZEROED_ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        } else {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        REQUESTED_ALLOCATION_BYTES.fetch_add(bytes, Ordering::Relaxed);
        record_peak(live);
    }
}

fn record_deallocation(bytes: u64) {
    saturating_sub(&LIVE_REQUESTED_BYTES, bytes);
    if INTERVAL_ACTIVE.load(Ordering::Acquire) {
        DEALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        REQUESTED_FREED_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }
}

fn record_reallocation(previous: u64, next: u64) {
    if next >= previous {
        let growth = next - previous;
        let live = LIVE_REQUESTED_BYTES
            .fetch_add(growth, Ordering::AcqRel)
            .saturating_add(growth);
        if INTERVAL_ACTIVE.load(Ordering::Acquire) {
            record_peak(live);
        }
    } else {
        saturating_sub(&LIVE_REQUESTED_BYTES, previous - next);
    }
    if INTERVAL_ACTIVE.load(Ordering::Acquire) {
        REALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        REQUESTED_REALLOCATION_BYTES.fetch_add(next, Ordering::Relaxed);
        REQUESTED_FREED_BYTES.fetch_add(previous, Ordering::Relaxed);
    }
}

fn record_peak(candidate: u64) {
    let mut observed = INTERVAL_LIVE_PEAK.load(Ordering::Relaxed);
    while candidate > observed {
        match INTERVAL_LIVE_PEAK.compare_exchange_weak(
            observed,
            candidate,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(next) => observed = next,
        }
    }
}

fn saturating_sub(counter: &AtomicU64, value: u64) {
    let mut observed = counter.load(Ordering::Relaxed);
    loop {
        let next = observed.saturating_sub(value);
        match counter.compare_exchange_weak(observed, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(current) => observed = current,
        }
    }
}
