// SPDX-License-Identifier: Apache-2.0
//! Append-only byte store with stable slice addresses.

use std::cell::RefCell;
use std::ptr::NonNull;

use super::context::DecodeContext;
use super::ScopedReservation;
use crate::CodecError;

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
    pub fn alloc(&self, ctx: &DecodeContext<'_>, bytes: Box<[u8]>) -> Result<&[u8], CodecError> {
        // Registry entries outlive a caller's active scratch or candidate
        // capture. Their backing belongs to the arena's session output.
        let _routing = ctx.budget.session_storage();
        let mut buffers = self.buffers.borrow_mut();
        let mut registry_growth = ctx.provisional_retained("arena registry")?;
        let buffer = OwnedBuffer(NonNull::from(Box::leak(bytes)));
        let pointer = buffer.0;
        // Only newly reserved registry backing is provisional. A refusal
        // cannot refund backing that an earlier allocation already installed.
        registry_growth.with_storage(|| ctx.reserve_vec(&mut buffers, 1, "arena registry"))?;
        registry_growth.commit_to_session();
        buffers.push(buffer);
        // SAFETY: the arena owns every buffer until it is dropped, and no
        // method mutates a buffer. The borrow cannot outlive the arena.
        Ok(unsafe { pointer.as_ref() })
    }

    /// Promotes and installs one actual scoped payload. Registry admission
    /// remains separate; failure destroys the payload before its lease drops.
    pub(super) fn alloc_scoped(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: Box<[u8]>,
        storage: ScopedReservation<'_>,
    ) -> Result<&[u8], CodecError> {
        let (bytes, payload) = storage.promote_session_value(bytes)?;
        let bytes = self.alloc(ctx, bytes)?;
        payload.commit_to_session();
        Ok(bytes)
    }

    #[cfg(test)]
    pub(super) fn allocation_state(&self) -> (usize, usize) {
        let buffers = self.buffers.borrow();
        (buffers.len(), buffers.capacity())
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
    mod provisional_concat;

    use super::DecodeArena;
    use crate::decode::{DecodeContext, DecodePolicy};

    fn buffer(index: usize) -> Box<[u8]> {
        let len = (index % 7) + 1;
        vec![u8::try_from(index).expect("test index fits u8"); len].into_boxed_slice()
    }

    fn check(index: usize, slice: &[u8]) {
        assert_eq!(slice.len(), (index % 7) + 1, "length of buffer {index}");
        assert!(
            slice
                .iter()
                .all(|&byte| byte == u8::try_from(index).expect("test index fits u8")),
            "contents of buffer {index}",
        );
    }

    #[test]
    fn empty_buffer_borrows_survive_later_allocations() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .expect("empty root");
        let empty = arena
            .alloc(&ctx, Vec::new().into_boxed_slice())
            .expect("admitted registry");
        let present = arena
            .alloc(&ctx, vec![7].into_boxed_slice())
            .expect("admitted registry");
        assert!(empty.is_empty());
        assert_eq!(present, &[7]);
    }

    #[test]
    fn arena_registry_outlives_enclosing_scratch_and_candidate_scopes() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut scratch = ctx.reserve_scoped(0, "scratch").expect("guard");
        let view = scratch
            .with_storage(|| arena.alloc(&ctx, Vec::new().into_boxed_slice()))
            .expect("session registry");
        drop(scratch);
        assert!(view.is_empty());
        assert_eq!(arena.buffers.borrow().len(), 1);
        let registered = crate::decode::u64_from_index(
            arena.buffers.borrow().capacity() * std::mem::size_of::<super::OwnedBuffer>(),
        );
        assert_eq!(ctx.budget.retained_used(), registered);
        assert_eq!(ctx.budget.materialized_used(), 0);
        let mut candidate = ctx.provisional_retained("candidate").expect("guard");
        let view = candidate
            .with_storage(|| arena.alloc(&ctx, Vec::new().into_boxed_slice()))
            .expect("session registry");
        drop(candidate);
        assert!(view.is_empty());
        assert_eq!(arena.buffers.borrow().len(), 2);
        assert_eq!(ctx.budget.retained_used(), registered);
    }

    #[test]
    fn arena_registry_retained_refusal_leaves_live_arena_unchanged() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut scratch = ctx.reserve_scoped(0, "scratch").expect("guard");
        let error = scratch
            .with_storage(|| arena.alloc(&ctx, Vec::new().into_boxed_slice()))
            .expect_err("session retained refusal");
        let crate::CodecError::ResourceLimit(original) = error else {
            panic!("retained refusal")
        };
        assert_eq!(
            original.dimension,
            crate::decode::ResourceDimension::RetainedBytes
        );
        assert_eq!(original.operation, "arena registry");
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(arena.buffers.borrow().is_empty());
        assert_eq!(arena.buffers.borrow().capacity(), 0);
        drop(scratch);
        assert_eq!(ctx.budget.materialized_used(), 0);
    }

    #[test]
    fn an_owned_arena_can_move_between_threads() {
        let arena = DecodeArena::new();
        {
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
                .expect("empty root");
            arena
                .alloc(&ctx, vec![7].into_boxed_slice())
                .expect("admitted registry");
        }
        let arena = std::thread::spawn(move || {
            {
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
                        .expect("empty root");
                assert_eq!(
                    arena
                        .alloc(&ctx, vec![9].into_boxed_slice())
                        .expect("admitted registry"),
                    &[9]
                );
            }
            arena
        })
        .join()
        .expect("the receiving thread retains and reads the arena");
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .expect("empty root");
        assert_eq!(
            arena
                .alloc(&ctx, vec![11].into_boxed_slice())
                .expect("admitted registry"),
            &[11]
        );
    }

    /// Alloc while earlier borrows stay live: each iteration allocates, then
    /// re-reads every prior slice.
    #[test]
    fn interleaved_alloc_and_read_across_many_buffers() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .expect("empty root");
        let mut borrows: Vec<&[u8]> = Vec::new();
        for index in 0..256usize {
            borrows.push(arena.alloc(&ctx, buffer(index)).expect("admitted registry"));
            for (i, slice) in borrows.iter().enumerate() {
                check(i, slice);
            }
        }
    }
}
