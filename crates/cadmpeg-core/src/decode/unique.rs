// SPDX-License-Identifier: Apache-2.0
//! One-table indexes that exclude every repeated key.

use super::cost::DecodeCost;
use super::{DecodeContext, ScopedReservation};
use crate::CodecError;
use std::collections::HashMap;
use std::hash::Hash;

impl DecodeContext<'_> {
    /// Builds a scoped table. Repeated keys become tombstones until the final
    /// in-place removal, so a third occurrence cannot restore a duplicate.
    /// Surviving values are Some; no second table is allocated.
    pub fn unique_index<K: Eq + Hash + DecodeCost, V>(
        &self,
        entries: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<(HashMap<K, Option<V>>, ScopedReservation<'_>), CodecError> {
        let mut table = HashMap::<K, Option<V>>::new();
        let mut storage = self.reserve_scoped(0, operation)?;
        let mut entries = entries.into_iter();
        loop {
            let Some((key, value)) = self.next_charged(&mut entries, operation)? else {
                break;
            };
            if let Some(previous) = self.get_mut_hash_map(&mut table, &key, operation)? {
                *previous = None;
                continue;
            }
            // discarded-value: the key was absent before the admitted insertion.
            let _ = storage
                .with_storage(|| self.insert_hash_map(&mut table, key, Some(value), operation))?;
        }
        self.retain_hash_map(&mut table, |_, value| Ok(value.is_some()), operation)?;
        Ok((table, storage))
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    use crate::CodecError;

    #[test]
    fn unique_index_removes_all_duplicate_occurrences_in_one_table() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        policy.limits.max_materialized_bytes =
            u64_from_index(4 * (std::mem::size_of::<(u32, Option<i32>)>() + 32));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (table, storage) = ctx
            .unique_index([(1_u32, 2), (1, 3), (1, 4), (2, 5)], "unique test")
            .expect("two slots");
        assert_eq!(table.len(), 1);
        assert_eq!(table.get(&2), Some(&Some(5)));
        assert!(!table.contains_key(&1));
        drop((table, storage));
        ctx.reserve_scoped(policy.limits.max_materialized_bytes, "released table")
            .expect("storage released");
    }

    #[test]
    fn unique_index_one_slot_keeps_one_record() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (table, _storage) = ctx
            .unique_index([(7_u32, 7)], "unique test")
            .expect("one table slot");
        assert_eq!(table.get(&7), Some(&Some(7)));
    }

    #[test]
    fn unique_index_propagates_slot_workspace_and_key_work_refusals() {
        for dimension in [
            ResourceDimension::CollectionItems,
            ResourceDimension::MaterializedBytes,
            ResourceDimension::WorkUnits,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 3,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
            assert!(matches!(ctx.unique_index([("long key", 1)], "unique test"),
                Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension));
        }
    }

    #[test]
    fn unique_index_charges_shared_lookup_insertion_and_bucket_retention() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Two source steps, three three-byte key operations, and three hash bucket visits.
        policy.limits.max_work_units = 14;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (values, _scope) = ctx
            .unique_index([(String::from("key"), 7_u8)], "index")
            .expect("admission");
        assert_eq!(values.get("key"), Some(&Some(7)));
        assert_eq!(values.capacity(), 3);
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, "probe").expect_err("exact work")
        else {
            panic!("refusal")
        };
        assert_eq!(limit.used, 14);
        policy.limits.max_work_units = 13;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) = ctx
            .unique_index([(String::from("key"), 7_u8)], "index")
            .expect_err("bucket refusal")
        else {
            panic!("refusal")
        };
        let CodecError::ResourceLimit(second) = ctx.charge_work(0, "later").expect_err("fused")
        else {
            panic!("refusal")
        };
        assert_eq!(first, second);
    }
}
