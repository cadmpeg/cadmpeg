// SPDX-License-Identifier: Apache-2.0
//! Unique native identities in ordered reconstruction inputs.

use std::collections::HashMap;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

pub(super) struct UniqueRows<'rows, 'ctx, T> {
    rows: HashMap<u32, Option<&'rows T>>,
    _storage: ScopedReservation<'ctx>,
}

impl<'rows, 'ctx, T> UniqueRows<'rows, 'ctx, T> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, rows: &'rows [T],
        identity: impl Fn(&T) -> Option<u32>, operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut indexed = HashMap::new();
        storage.with_storage(|| {
            for row in ctx.admit_iter(rows, operation)? {
                let Some(identity) = identity(row) else { continue; };
                match ctx.entry_hash_map(&mut indexed, identity, operation)? {
                    std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(row)); }
                    std::collections::hash_map::Entry::Occupied(mut entry) => { *entry.get_mut() = None; }
                }
            }
            Ok::<_, CodecError>(())
        })?;
        Ok(Self { rows: indexed, _storage: storage })
    }

    pub(super) fn unique(&self, id: u32) -> Option<&'rows T> {
        self.rows.get(&id).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::UniqueRows;
    #[test]
    fn native_identity_index_storage_is_scoped_and_refuses_materialization() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let rows = [1, 2, 3];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let index = UniqueRows::new(&ctx, &rows, |id| Some(*id), "native scoped index").expect("scoped index");
        assert_eq!(index.unique(2), Some(&2));
        assert!(ctx.resource_refusal().is_none());
        let error = crate::test_support::last_refusal_at(&[], ResourceDimension::MaterializedBytes,
            "native scoped index", |ctx| {
                let index = UniqueRows::new(ctx, &rows, |id| Some(*id), "native scoped index")?;
                assert_eq!(index.unique(2), Some(&2)); Ok(())
            });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "native scoped index"));
    }

    #[test]
    fn native_identity_index_preserves_duplicate_and_absent_rows() {
        crate::test_support::assert_work_boundaries(&["native row index"], |ctx| {
            let rows = [1, 2, 1, 1, 3];
            let index = UniqueRows::new(ctx, &rows, |id| Some(*id), "native row index")?;
            assert_eq!(index.unique(1), None);
            assert_eq!(index.unique(4), None);
            assert_eq!(index.unique(2), Some(&2));
            assert_eq!(index.unique(3), Some(&3));
            Ok(())
        });
    }
    #[test]
    fn native_identity_index_ignores_unselected_rows() {
        crate::test_support::assert_work_boundaries(&["native selected rows"], |ctx| {
            let rows = [(1, false), (1, true), (2, false), (3, true), (3, true)];
            let index = super::UniqueRows::new(ctx, &rows, |(id, selected)| selected.then_some(*id), "native selected rows")?;
            assert_eq!(index.unique(1), Some(&rows[1]));
            assert_eq!(index.unique(2), None);
            assert_eq!(index.unique(3), None);
            Ok(())
        });
    }
}
