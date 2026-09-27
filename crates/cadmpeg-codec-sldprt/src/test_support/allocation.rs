// SPDX-License-Identifier: Apache-2.0
//! Per-thread allocation observation for native admission tests.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn record_allocation() {
    if TRACK.try_with(Cell::get).unwrap_or(false) {
        let _counted = COUNT.try_with(|count| count.set(count.get() + 1));
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_allocation();
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_allocation();
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let resized = unsafe { System.realloc(pointer, layout, new_size) };
        if !resized.is_null() {
            record_allocation();
        }
        resized
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }
}

pub(crate) fn count_allocations<R>(action: impl FnOnce() -> R) -> (R, usize) {
    COUNT.with(|count| count.set(0));
    TRACK.with(|track| track.set(true));
    let result = action();
    TRACK.with(|track| track.set(false));
    (result, COUNT.with(Cell::get))
}

#[cfg(test)]
mod tests {
    #[test]
    fn allocation_counter_distinguishes_heap_copy_from_stack_value() {
        let (value, copied) = super::count_allocations(|| "copied value".to_owned());
        assert_eq!(value, "copied value");
        assert!(copied > 0);
        let (value, stack_only) = super::count_allocations(|| 42_u32);
        assert_eq!(value, 42);
        assert_eq!(stack_only, 0);
    }
}
