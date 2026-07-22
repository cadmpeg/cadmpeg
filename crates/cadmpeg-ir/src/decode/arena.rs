// SPDX-License-Identifier: Apache-2.0
//! Append-only byte store with stable slice addresses.

use std::cell::RefCell;

/// Owns stable byte buffers allocated during a decode.
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
        let slice: &[u8] = buffers.last().expect("a buffer was just pushed").as_ref();
        // SAFETY: `slice` points into the heap allocation owned by the box we
        // just pushed. That allocation is not freed or moved until the arena
        // is dropped, and the arena outlives every borrow of `&self`, so the
        // pointer is valid for the returned lifetime. The bytes are never
        // mutated after being stored (the arena exposes no mutation), so
        // aliasing `&[u8]` borrows do not conflict.
        //
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
    /// `&mut Vec<Box<[u8]>>` and, across 256 pushes, reallocates the outer `Vec`
    /// many times over; reading every slice handed out so far, on every
    /// iteration, forces the aliasing model to validate each earlier pointer
    /// against the later retag. This is the alloc-while-earlier-borrows-live
    /// case a raw-pointer rework would need to satisfy.
    ///
    /// It subsumes the narrower scenarios that do not add coverage: outer-`Vec`
    /// regrowth (the reallocations here relocate the box *pointers* while the
    /// boxed bytes the borrows address never move) and read order (a shared read
    /// never mutates the borrow stack or tree, so revalidating every held
    /// pointer each iteration already covers any order). Neither could fail
    /// where this test passes.
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
}
