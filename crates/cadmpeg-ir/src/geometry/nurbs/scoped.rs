// SPDX-License-Identifier: Apache-2.0
//! Immutable NURBS scratch rows with their live storage reservation.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit, ScopedReservation};
use std::ops::Deref;

/// Temporary rows whose storage remains reserved until the rows are dropped.
#[derive(Debug)]
pub struct ScopedRows<'ctx, T> {
    rows: Vec<T>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx, T> ScopedRows<'ctx, T> {
    pub(crate) fn new(rows: Vec<T>, storage: ScopedReservation<'ctx>) -> Self {
        Self { rows, _storage: storage }
    }

    pub(crate) fn reverse(
        &mut self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        ctx.charge_work_limit(u64_from_index(self.rows.len()), operation)?;
        self.rows.reverse();
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
    use super::ScopedRows;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
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
            let storage;
            let mut rows = Vec::new();
            storage = ctx.reserve_temporary_vec(&mut rows, 2, "test scoped rows").expect("eight bytes");
            rows.extend_from_slice(&[3_u32, 7_u32]);
            let rows = ScopedRows::new(rows, storage);
            assert_eq!(&*rows, &[3, 7]);
            assert_eq!(rows[1], 7);
            if release {
                drop(rows);
                let reuse = ctx.reserve_scoped_limit(8, "test scoped rows reuse").expect("all bytes released");
                drop(reuse);
                ctx.finish_session().expect("temporary rows retain no bytes");
            } else {
                let limit = ctx.reserve_scoped_limit(1, "test scoped rows live").expect_err("rows keep all eight bytes reserved");
                assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(limit.limit, 8);
                assert_eq!(limit.operation, "test scoped rows live");
                assert_eq!(&*rows, &[3, 7]);
                drop(rows);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
            }
        }
    }
}
