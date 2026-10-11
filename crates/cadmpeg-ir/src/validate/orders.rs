// SPDX-License-Identifier: Apache-2.0
//! Scoped uniqueness of numeric presentation orders.

use std::collections::BTreeSet;

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

pub(super) struct Orders<'ctx, 'arena, T = u32> {
    values: BTreeSet<T>,
    ctx: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx, 'arena, T: Ord + DecodeCost> Orders<'ctx, 'arena, T> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self {
            values: BTreeSet::new(),
            ctx,
            storage: ctx.reserve_scoped(0, "validation order storage")?,
        })
    }

    /// Admit `order`, reporting `false` when it is already present.
    pub(super) fn insert(&mut self, order: T) -> Result<bool, CodecError> {
        self.ctx.insert_scoped_btree_set(
            &mut self.storage,
            &mut self.values,
            order,
            "compare validation order",
            "validation order slots",
        )
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn validation_orders_preserve_growth_and_comparison_refusals() {
        for (dimension, operation) in [
            (
                ResourceDimension::MaterializedBytes,
                "validation order slots",
            ),
            (ResourceDimension::CollectionItems, "validation order slots"),
            // An empty set needs no comparison, so the node storage refuses.
            (ResourceDimension::WorkUnits, "validation order slots"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut orders = super::Orders::<u32>::new(&ctx).unwrap();
            let Err(CodecError::ResourceLimit(limit)) = orders.insert(10) else {
                panic!("order operation must refuse");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(limit.operation, operation);
            assert!(orders.values.is_empty());
            drop(orders);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        }
    }

    #[test]
    fn validation_orders_keep_uniqueness_and_release_temporary_storage() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 4096;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        {
            let mut orders = super::Orders::<u32>::new(&ctx).unwrap();
            for order in [10, 3, 7] {
                assert!(orders.insert(order).unwrap());
            }
            for order in [7, 3, 10] {
                assert!(!orders.insert(order).unwrap());
            }
            assert!(orders.values.iter().copied().eq([3, 7, 10]));
        }
        drop(ctx.reserve_scoped(4096, "order scope released").unwrap());
        ctx.finish_session().unwrap();
    }
    #[test]
    fn validation_orders_keep_full_width_feature_ordinals() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut orders = super::Orders::new(&ctx).unwrap();
        assert!(orders.insert(u64::MAX).unwrap());
        assert!(orders.insert(0u64).unwrap());
        assert!(!orders.insert(u64::MAX).unwrap());
        assert!(orders.values.iter().copied().eq([0, u64::MAX]));
        drop(orders);
        ctx.finish_session().unwrap();
    }
}
