// SPDX-License-Identifier: Apache-2.0
//! Scoped uniqueness of numeric presentation orders.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

pub(super) struct Orders<'ctx, 'arena> {
    values: Vec<u32>,
    ctx: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx, 'arena> Orders<'ctx, 'arena> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self { values: Vec::new(), ctx, storage: ctx.reserve_scoped(0, "validation order storage")? })
    }

    pub(super) fn insert(&mut self, order: u32) -> Result<bool, CodecError> {
        let mut low = 0;
        let mut high = self.values.len();
        while low < high {
            self.ctx.charge_work(1, "compare validation order")?;
            let middle = low + (high - low) / 2;
            match self.values[middle].cmp(&order) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Ok(false),
            }
        }
        self.ctx.charge_work(u64_from_index(self.values.len() - low), "move validation orders")?;
        self.storage.with_storage(|| self.ctx.reserve_retained_vec(&mut self.values, 1, "validation order slots"))?;
        self.values.insert(low, order);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn validation_orders_preserve_growth_and_comparison_refusals() {
        for (dimension, cap, operation) in [
            (ResourceDimension::MaterializedBytes, 0, "validation order slots"),
            (ResourceDimension::CollectionItems, 0, "validation order slots"),
            (ResourceDimension::WorkUnits, 0, "compare validation order"),
            (ResourceDimension::WorkUnits, 1, "move validation orders"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut orders = super::Orders::new(&ctx).unwrap();
            let result = if dimension == ResourceDimension::WorkUnits {
                assert!(orders.insert(10).unwrap());
                orders.insert(5)
            } else { orders.insert(10) };
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("order operation must refuse"); };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(limit.operation, operation);
            assert_eq!(orders.values, if dimension == ResourceDimension::WorkUnits { vec![10] } else { Vec::new() });
            drop(orders);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
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
            let mut orders = super::Orders::new(&ctx).unwrap();
            for order in [10, 3, 7] { assert!(orders.insert(order).unwrap()); }
            for order in [7, 3, 10] { assert!(!orders.insert(order).unwrap()); }
            assert_eq!(orders.values, [3, 7, 10]);
        }
        drop(ctx.reserve_scoped(4096, "order scope released").unwrap());
        ctx.finish_session().unwrap();
    }
}
