//! Test only allocation probe.
//!
//! The performance work claims that a rewrite lowers peak memory, so the tests
//! have to be able to observe that instead of asserting it. A counting global
//! allocator reports how many bytes are live at once, which is the number the
//! refactoring actually moves; RSS is too noisy and too process dependent to be
//! useful in a unit test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

impl Counting {
    fn grow(size: usize) {
        let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
        let mut peak = PEAK.load(Ordering::Relaxed);
        while live > peak {
            match PEAK.compare_exchange_weak(peak, live, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => peak = actual,
            }
        }
    }

    fn shrink(size: usize) {
        // Saturating keeps the counter sane when the harness frees buffers that
        // were allocated before the first measurement.
        let _ = LIVE.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |live| {
            Some(live.saturating_sub(size))
        });
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            Self::grow(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            Self::grow(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        Self::shrink(layout.size());
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let resized = unsafe { System.realloc(pointer, layout, new_size) };
        if !resized.is_null() {
            Self::shrink(layout.size());
            Self::grow(new_size);
        }
        resized
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

// Starts a measurement and returns the live byte count to hand to peak_extra.
pub fn begin() -> usize {
    let live = LIVE.load(Ordering::Relaxed);
    PEAK.store(live, Ordering::Relaxed);
    live
}

// Peak live bytes above the baseline returned by begin.
pub fn peak_extra(baseline: usize) -> usize {
    PEAK.load(Ordering::Relaxed).saturating_sub(baseline)
}
