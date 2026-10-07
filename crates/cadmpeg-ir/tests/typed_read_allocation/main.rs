// SPDX-License-Identifier: Apache-2.0
//! Typed native reads allocate nothing the decode budget has not admitted.
//!
//! A counting allocator measures the peak a read reaches above the stored
//! record, so a reservation made ahead of admission or an error message that
//! copies stored text shows up as allocation the budget never saw.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::Deserialize;

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call forwards to the system allocator with the caller's
// layout; the counters only observe sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let live = LIVE.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
        PEAK.fetch_max(live, Ordering::SeqCst);
        // SAFETY: the layout is the caller's, unchanged.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        // SAFETY: `ptr` came from `alloc` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn stored(record: &serde_json::Value) -> NativeNamespace {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("empty root is admitted");
    let mut namespace = NativeNamespace::default();
    namespace
        .set_arena(&ctx, "records", std::slice::from_ref(record))
        .expect("the record is stored");
    namespace
}

/// Read the stored record under the given limits; return the outcome, the
/// peak allocation above the starting point and the error text's length.
fn read<T: for<'de> Deserialize<'de>>(
    namespace: &NativeNamespace,
    retained: u64,
    work: u64,
) -> (bool, usize, usize) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = retained;
    policy.limits.max_work_units = work;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let base = LIVE.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let result = namespace.arena_as_for_decode::<T>(&ctx, "records");
    let peak = PEAK.load(Ordering::SeqCst) - base;
    let text = result
        .as_ref()
        .err()
        .map_or(0, |error| error.to_string().len());
    let refused = matches!(result, Err(NativeConvertError::Resource(_)));
    (refused, peak, text)
}

#[derive(Deserialize)]
struct Wide {
    #[allow(dead_code)]
    v: Vec<[u64; 16]>,
}

#[derive(Deserialize)]
struct Narrow {
    #[allow(dead_code)]
    n: u32,
}

/// Far below the stored values' own size and far above one record's
/// bookkeeping.
const SMALL_PEAK: usize = 64 * 1024;

#[test]
fn typed_reads_allocate_only_admitted_storage() {
    // A sequence reserves nothing ahead of its admitted elements.
    let rows = vec![vec![0_u64; 16]; 8000];
    let wide = stored(&serde_json::json!({"id": "test:probe:record#wide", "v": rows}));
    let (refused, peak, _) = read::<Wide>(&wide, 4096, u64::MAX);
    assert!(refused, "the vector's elements exceed the retained limit");
    assert!(
        peak < SMALL_PEAK,
        "allocated {peak} bytes ahead of admission"
    );

    // A refusal for a mistyped string copies none of the string.
    for text in ["x".repeat(1_000_000), "\u{1}".repeat(200_000)] {
        let narrow = stored(&serde_json::json!({"id": "test:probe:record#narrow", "n": text}));
        let (refused, peak, message) = read::<Narrow>(&narrow, 4096, 1_000_000);
        assert!(!refused, "a mistyped member is a data error");
        assert!(message < 1024, "the error carried {message} bytes");
        assert!(peak < SMALL_PEAK, "allocated {peak} bytes for the error");
    }
}
