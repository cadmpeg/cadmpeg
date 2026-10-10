// SPDX-License-Identifier: Apache-2.0
//! Immutable NURBS scratch rows with their live storage reservation.

use cadmpeg_core::decode::{DecodeContext, ResourceLimit, ScopedReservation};
use std::ops::Deref;

/// Temporary rows whose storage remains reserved until the rows are dropped.
#[derive(Debug)]
pub struct ScopedRows<'ctx, T> {
    rows: Vec<T>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx, T> ScopedRows<'ctx, T> {
    pub(crate) fn new(rows: Vec<T>, storage: ScopedReservation<'ctx>) -> Self {
        Self {
            rows,
            _storage: storage,
        }
    }

    pub(crate) fn reverse(
        &mut self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        super::reverse_values(ctx.admit_iter(&mut self.rows, operation)?);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.rows.capacity()
    }
}

impl<T> Deref for ScopedRows<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::ScopedRows;
    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit,
    };
    use cadmpeg_core::CodecError;

    #[test]
    fn scoped_rows_hold_storage_through_borrows_and_release_it_on_drop() {
        for release in [false, true] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 8;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 2;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let (storage, mut rows) = {
                let mut values = Vec::new();
                let reservation = ctx
                    .reserve_temporary_vec(&mut values, 2, "test scoped rows")
                    .expect("eight bytes");
                (reservation, values)
            };
            rows.extend_from_slice(&[3_u32, 7_u32]);
            let rows = ScopedRows::new(rows, storage);
            assert_eq!(&*rows, &[3, 7]);
            assert_eq!(rows[1], 7);
            if release {
                drop(rows);
                let reuse = ctx
                    .reserve_scoped_limit(8, "test scoped rows reuse")
                    .expect("all bytes released");
                drop(reuse);
                ctx.finish_session()
                    .expect("temporary rows retain no bytes");
            } else {
                let limit = ctx
                    .reserve_scoped_limit(1, "test scoped rows live")
                    .expect_err("rows keep all eight bytes reserved");
                assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(limit.limit, 8);
                assert_eq!(limit.operation, "test scoped rows live");
                assert_eq!(&*rows, &[3, 7]);
                drop(rows);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
                );
            }
        }
    }

    struct DropWithLiveStorage<'ctx, 'arena> {
        ctx: &'ctx DecodeContext<'arena>,
        observed: &'ctx Cell<Option<ResourceLimit>>,
    }

    impl Drop for DropWithLiveStorage<'_, '_> {
        fn drop(&mut self) {
            self.observed.set(
                self.ctx
                    .reserve_scoped_limit(1, "observe live rows during child drop")
                    .err(),
            );
        }
    }

    #[test]
    fn scoped_rows_destroy_children_before_releasing_their_backing_reservation(
    ) -> Result<(), CodecError> {
        let bytes = u64::try_from(std::mem::size_of::<DropWithLiveStorage<'_, '_>>())
            .expect("row size fits the resource counter");
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        let observed = Cell::new(None);
        let storage;
        let mut rows = Vec::new();
        storage = ctx.reserve_temporary_vec(&mut rows, 1, "construct observed scoped rows")?;
        rows.push(DropWithLiveStorage {
            ctx: &ctx,
            observed: &observed,
        });
        let rows = ScopedRows::new(rows, storage);
        assert_eq!(rows.capacity(), 1);
        drop(rows);
        let limit = observed
            .get()
            .expect("child drop must observe the live reservation");
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(
            (limit.limit, limit.used, limit.additional),
            (bytes, bytes, 1)
        );
        assert_eq!(limit.operation, "observe live rows during child drop");
        assert_eq!(ctx.resource_refusal(), Some(limit));
        Ok(())
    }

    struct PanicOnFirstDrop<'a>(&'a Cell<usize>);

    impl Drop for PanicOnFirstDrop<'_> {
        fn drop(&mut self) {
            let previous = self.0.replace(self.0.get() + 1);
            assert!(previous != 0, "first row destructor fails");
        }
    }

    #[test]
    fn scoped_rows_unwind_destroys_remaining_children_and_releases_storage(
    ) -> Result<(), CodecError> {
        let bytes = u64::try_from(2 * std::mem::size_of::<PanicOnFirstDrop<'_>>())
            .expect("two row sizes fit the resource counter");
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        let dropped = Cell::new(0);
        let storage;
        let mut rows = Vec::new();
        storage = ctx.reserve_temporary_vec(&mut rows, 2, "construct unwinding scoped rows")?;
        rows.extend([PanicOnFirstDrop(&dropped), PanicOnFirstDrop(&dropped)]);
        let rows = ScopedRows::new(rows, storage);
        assert_eq!(rows.capacity(), 2);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(rows)));
        assert!(result.is_err());
        assert_eq!(dropped.get(), 2);
        assert_eq!(ctx.resource_refusal(), None);
        drop(ctx.reserve_scoped_limit(bytes, "reuse storage after child unwind")?);
        ctx.finish_session()?;
        Ok(())
    }
}
