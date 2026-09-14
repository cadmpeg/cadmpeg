// SPDX-License-Identifier: Apache-2.0
//! Append-only byte store with stable slice addresses.

use std::cell::RefCell;
use std::ptr::NonNull;

/// Owns stable byte buffers allocated during a decode.
#[derive(Debug, Default)]
pub struct DecodeArena {
    buffers: RefCell<Vec<OwnedBuffer>>,
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
        let buffer = OwnedBuffer(NonNull::from(Box::leak(bytes)));
        let pointer = buffer.0;
        buffers.push(buffer);
        // SAFETY: the arena owns every buffer until it is dropped, and no
        // method mutates a buffer. The borrow cannot outlive the arena.
        unsafe { pointer.as_ref() }
    }
}

/// Owns one allocation without imposing a movable Box's unique-borrow tag.
#[derive(Debug)]
struct OwnedBuffer(NonNull<[u8]>);

// SAFETY: the allocation is uniquely owned, contains only bytes, and has no
// mutation API. Shared arena borrows cannot cross threads because RefCell
// makes DecodeArena !Sync.
unsafe impl Send for OwnedBuffer {}

impl Drop for OwnedBuffer {
    fn drop(&mut self) {
        // SAFETY: this pointer came from exactly one Box::leak and is released
        // exactly once. DecodeArena owns it and outlives every borrowed slice.
        unsafe { drop(Box::from_raw(self.0.as_ptr())) };
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

    #[test]
    fn empty_buffer_borrows_survive_later_allocations() {
        let arena = DecodeArena::new();
        let empty = arena.alloc(Vec::new().into_boxed_slice());
        let present = arena.alloc(vec![7].into_boxed_slice());
        assert!(empty.is_empty());
        assert_eq!(present, &[7]);
    }

    #[test]
    fn an_owned_arena_can_move_between_threads() {
        let arena = DecodeArena::new();
        arena.alloc(vec![7].into_boxed_slice());
        let arena = std::thread::spawn(move || {
            assert_eq!(arena.alloc(vec![9].into_boxed_slice()), &[9]);
            arena
        })
        .join()
        .expect("the receiving thread retains and reads the arena");
        assert_eq!(arena.alloc(vec![11].into_boxed_slice()), &[11]);
    }

    /// Alloc while earlier borrows stay live: each iteration allocates, then
    /// re-reads every prior slice.
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
