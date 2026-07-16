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
        // `borrow_mut` retags `&mut Vec<Box<[u8]>>` and may reallocate the
        // outer `Vec`, but the returned pointer addresses the pushed box's own
        // heap allocation, which that retag and any reallocation leave
        // untouched, so the borrow stays valid. Miri accepts this module under
        // both Stacked Borrows and Tree Borrows; the `tests` module exercises
        // alloc-while-earlier-borrows-live, outer-`Vec` regrowth, and
        // out-of-order reads that make each aliasing model validate every
        // earlier pointer against later retags.
        unsafe { std::slice::from_raw_parts(slice.as_ptr(), slice.len()) }
    }
}

#[cfg(test)]
mod tests {
    use super::DecodeArena;

    fn buffer(index: usize) -> Box<[u8]> {
        let len = (index % 7) + 1;
        vec![index as u8; len].into_boxed_slice()
    }

    fn check(index: usize, slice: &[u8]) {
        assert_eq!(slice.len(), (index % 7) + 1, "length of buffer {index}");
        assert!(
            slice.iter().all(|&byte| byte == index as u8),
            "contents of buffer {index}",
        );
    }

    /// Interleave allocation and reads so every earlier borrow is read *after*
    /// a later `alloc` has re-entered `borrow_mut`. Each `alloc` retags
    /// `&mut Vec<Box<[u8]>>` and may reallocate the outer `Vec`; reading every
    /// slice handed out so far, on every iteration, forces the aliasing model
    /// to validate each earlier pointer against the later retag. This is the
    /// alloc-while-earlier-borrows-live pattern the §4.1 note calls out, and is
    /// the case a raw-pointer rework would exist to satisfy.
    #[test]
    fn interleaved_alloc_and_read_across_many_buffers() {
        let arena = DecodeArena::new();
        let mut borrows: Vec<&[u8]> = Vec::new();
        for index in 0..256usize {
            borrows.push(arena.alloc(buffer(index)));
            for (i, slice) in borrows.iter().enumerate() {
                check(i, slice);
            }
        }
    }

    /// Isolate outer `Vec` regrowth: hold a borrow into the *first* buffer,
    /// then push enough buffers to reallocate the `Vec<Box<[u8]>>` backing
    /// store many times over, reading the held borrow after every push. The
    /// backing store moves (the box *pointers* are relocated) while the box's
    /// own heap allocation — the bytes the borrow addresses — never moves.
    #[test]
    fn outer_vec_regrowth_preserves_earliest_borrow() {
        let arena = DecodeArena::new();
        let first = arena.alloc(vec![0xA5u8; 3].into_boxed_slice());
        for index in 1..512usize {
            let _ = arena.alloc(buffer(index));
            assert_eq!(
                first,
                &[0xA5u8, 0xA5, 0xA5][..],
                "first borrow after push {index}"
            );
        }
    }

    /// Reads through borrows kept in a shuffled order, so pointer validation
    /// does not follow allocation order and cannot be satisfied by a stack
    /// discipline that only ever touches the most recent buffer.
    #[test]
    fn out_of_order_reads_stay_valid() {
        let arena = DecodeArena::new();
        let mut borrows: Vec<(usize, &[u8])> = Vec::new();
        for index in 0..128usize {
            borrows.push((index, arena.alloc(buffer(index))));
        }
        // Read youngest-to-oldest, then oldest-to-youngest.
        for &(index, slice) in borrows.iter().rev() {
            check(index, slice);
        }
        for &(index, slice) in &borrows {
            check(index, slice);
        }
    }
}
