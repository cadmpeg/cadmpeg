// SPDX-License-Identifier: Apache-2.0
//! Append-only byte store with stable slice addresses.
//!
//! A [`View`](crate::decode::View) is `Copy` and may outlive the call that
//! produced the bytes it borrows, so those bytes must never move or drop
//! while the decode runs. The arena guarantees exactly that: it hands out
//! `&'a [u8]` borrows that stay valid for the arena's lifetime.

use std::cell::RefCell;

/// Owns every byte buffer a decode allocates: inflated entries, reconstructed
/// streams, and the copied root input.
///
/// Buffers are stored as `Box<[u8]>` behind a `RefCell<Vec<_>>`. Each `Box`
/// owns a heap allocation whose address is fixed for the box's life; pushing
/// more boxes may reallocate the outer `Vec` (moving the box *pointers*) but
/// never moves the heap bytes a box points at, and the arena never removes or
/// replaces a box. A slice into a box therefore stays valid for as long as
/// the arena is borrowed.
#[derive(Debug, Default)]
pub struct DecodeArena {
    buffers: RefCell<Vec<Box<[u8]>>>,
}

impl DecodeArena {
    /// Creates an empty arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `bytes` and returns a borrow valid for the arena's lifetime.
    ///
    /// The returned slice is stable: later `alloc` calls never invalidate it.
    pub fn alloc(&self, bytes: Box<[u8]>) -> &[u8] {
        let mut buffers = self.buffers.borrow_mut();
        buffers.push(bytes);
        // The box just pushed is never moved, replaced, or dropped for the
        // arena's borrow: the arena only ever pushes, and its heap allocation
        // is fixed independently of the outer `Vec`'s storage. Extending the
        // slice borrow to the arena's lifetime is therefore sound. The
        // `RefCell` guard is released at the end of this statement, but the
        // bytes it protected outlive it because they are heap-owned by the
        // box, not by the guard.
        let slice: &[u8] = buffers.last().expect("a buffer was just pushed").as_ref();
        // SAFETY: `slice` points into the heap allocation owned by the box we
        // just pushed. That allocation is not freed or moved until the arena
        // is dropped, and the arena outlives every borrow of `&self`, so the
        // pointer is valid for the returned lifetime. The bytes are never
        // mutated after being stored (the arena exposes no mutation), so
        // aliasing `&[u8]` borrows do not conflict.
        //
        // Aliasing-model note: this is the elsa/FrozenVec pattern. A later
        // `borrow_mut` retags `&mut Vec<Box<[u8]>>`; whether that invalidates
        // raw pointers derived through an earlier guard is debated under
        // strict Stacked Borrows but accepted under Tree Borrows, because the
        // pointee is behind an untouched `Box` allocation. A Miri run over
        // this module is the follow-up verification.
        unsafe { std::slice::from_raw_parts(slice.as_ptr(), slice.len()) }
    }
}
