// SPDX-License-Identifier: Apache-2.0
//! Charged growth of collections owned by a decode session.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use crate::CodecError;

use super::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, ScopedReservation};

impl DecodeContext<'_> {
    fn collection_allocation_failed(&self, count: usize, operation: &'static str) -> CodecError {
        CodecError::ResourceLimit(ResourceLimit::allocation_failed(
            ResourceDimension::CollectionItems,
            self.policy().limits.max_collection_items,
            u64_from_index(count),
            operation,
        ))
    }

    /// Allocates a vector for `count` admitted items.
    pub fn collection_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        self.charge_collection_items(u64_from_index(count), operation)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| self.collection_allocation_failed(count, operation))?;
        Ok(values)
    }

    /// Reserves additional vector items and charges their admission.
    pub fn reserve_vec<T>(
        &self,
        values: &mut Vec<T>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_collection_items(u64_from_index(additional), operation)?;
        values
            .try_reserve(additional)
            .map_err(|_| self.collection_allocation_failed(additional, operation))
    }

    /// Adds one vector item after charging its slot.
    pub fn push_vec<T>(
        &self,
        values: &mut Vec<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec(values, 1, operation)?;
        values.push(value);
        Ok(())
    }

    /// Moves all source items into a vector after charging their slots.
    pub fn append_vec<T>(
        &self,
        target: &mut Vec<T>,
        source: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec(target, source.len(), operation)?;
        target.append(source);
        Ok(())
    }

    /// Moves owned items into a vector after charging their slots.
    pub fn extend_vec<T>(
        &self,
        target: &mut Vec<T>,
        mut source: Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.append_vec(target, &mut source, operation)
    }

    /// Collects iterator values with a charged slot for each value.
    pub fn collect_vec<T>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut out = Vec::new();
        for value in values {
            self.push_vec(&mut out, value, operation)?;
        }
        Ok(out)
    }

    /// Collects fallible iterator values with a charged slot for each success.
    pub fn try_collect_vec<T, E: From<CodecError>>(
        &self,
        values: impl IntoIterator<Item = Result<T, E>>,
        operation: &'static str,
    ) -> Result<Vec<T>, E> {
        let mut out = Vec::new();
        for value in values {
            self.push_vec(&mut out, value?, operation)?;
        }
        Ok(out)
    }

    /// Inserts a new set item after charging its slot.
    pub fn insert_hash_set<T: Eq + Hash>(
        &self,
        values: &mut HashSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if values.contains(&value) {
            return Ok(false);
        }
        self.charge_collection_items(1, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| self.collection_allocation_failed(1, operation))?;
        Ok(values.insert(value))
    }

    /// Inserts a copied string into a set if the value is new.
    pub fn insert_string_set(
        &self,
        values: &mut HashSet<String>,
        value: &str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if values.contains(value) {
            return Ok(false);
        }
        self.charge_collection_items(1, operation)?;
        let owned = self.copy_retained_text(value, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| self.collection_allocation_failed(1, operation))?;
        Ok(values.insert(owned))
    }

    /// Collects distinct values into a charged hash set.
    pub fn collect_hash_set<T: Eq + Hash>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<HashSet<T>, CodecError> {
        let mut out = HashSet::new();
        for value in values {
            self.insert_hash_set(&mut out, value, operation)?;
        }
        Ok(out)
    }

    /// Reserves a map slot only when the key is new.
    pub fn admit_hash_map_entry<K: Eq + Hash, V>(
        &self,
        values: &mut HashMap<K, V>,
        key: &K,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if !values.contains_key(key) {
            self.charge_collection_items(1, operation)?;
            values
                .try_reserve(1)
                .map_err(|_| self.collection_allocation_failed(1, operation))?;
        }
        Ok(())
    }

    /// Inserts a map entry after admitting a new key, if needed.
    pub fn insert_hash_map<K: Eq + Hash, V>(
        &self,
        values: &mut HashMap<K, V>,
        key: K,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError> {
        self.admit_hash_map_entry(values, &key, operation)?;
        Ok(values.insert(key, value))
    }

    /// Collects entries into a charged hash map.
    pub fn collect_hash_map<K: Eq + Hash, V>(
        &self,
        values: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<HashMap<K, V>, CodecError> {
        let mut out = HashMap::new();
        for (key, value) in values {
            self.insert_hash_map(&mut out, key, value, operation)?;
        }
        Ok(out)
    }

    /// Copies temporary text while its scoped reservation remains live.
    pub fn copy_scoped_text(
        &self,
        text: &str,
        reservation: &mut ScopedReservation<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        reservation.grow(u64_from_index(text.len()))?;
        let mut copy = String::new();
        copy.try_reserve_exact(text.len()).map_err(|_| {
            CodecError::ResourceLimit(ResourceLimit::allocation_failed(
                ResourceDimension::MaterializedBytes,
                self.policy().limits.max_materialized_bytes,
                u64_from_index(text.len()),
                operation,
            ))
        })?;
        copy.push_str(text);
        Ok(copy)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::super::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    fn context<'a>(arena: &'a DecodeArena, items: u64) -> DecodeContext<'a> {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        match DecodeContext::from_root_bytes(&[], arena, &policy) {
            Ok((ctx, _)) => ctx,
            Err(error) => panic!("test context failed: {error}"),
        }
    }

    macro_rules! collection_case {
        ($name:ident, $needed:expr, $refused:expr, $success:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let ctx = context(&arena, $needed - 1);
                let result: Result<(), CodecError> = ($refused)(&ctx);
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems));
                let arena = DecodeArena::new();
                let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
                let result: Result<(), CodecError> = ($success)(&ctx);
                assert!(result.is_ok(), "service profile admits the operation");
            }
        };
    }

    collection_case!(collection_vec_charges_before_allocation, 2,
        |ctx: &DecodeContext<'_>| ctx.collection_vec::<u8>(2, "test collection vec").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.collection_vec::<u8>(2, "test collection vec").map(|_| ()));
    collection_case!(reserve_vec_charges_before_growth, 1,
        |ctx: &DecodeContext<'_>| ctx.reserve_vec(&mut Vec::<u8>::new(), 1, "test reserve vec"),
        |ctx: &DecodeContext<'_>| ctx.reserve_vec(&mut Vec::<u8>::new(), 1, "test reserve vec"));
    collection_case!(push_vec_charges_before_growth, 1,
        |ctx: &DecodeContext<'_>| ctx.push_vec(&mut Vec::new(), 7_u8, "test push vec"),
        |ctx: &DecodeContext<'_>| ctx.push_vec(&mut Vec::new(), 7_u8, "test push vec"));
    collection_case!(append_vec_charges_before_growth, 2,
        |ctx: &DecodeContext<'_>| ctx.append_vec(&mut Vec::new(), &mut vec![1_u8, 2], "test append vec"),
        |ctx: &DecodeContext<'_>| ctx.append_vec(&mut Vec::new(), &mut vec![1_u8, 2], "test append vec"));
    collection_case!(extend_vec_charges_before_growth, 2,
        |ctx: &DecodeContext<'_>| ctx.extend_vec(&mut Vec::new(), vec![1_u8, 2], "test extend vec"),
        |ctx: &DecodeContext<'_>| ctx.extend_vec(&mut Vec::new(), vec![1_u8, 2], "test extend vec"));
    collection_case!(collect_vec_charges_before_growth, 2,
        |ctx: &DecodeContext<'_>| ctx.collect_vec([1_u8, 2], "test collect vec").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.collect_vec([1_u8, 2], "test collect vec").map(|_| ()));
    collection_case!(try_collect_vec_charges_before_growth, 2,
        |ctx: &DecodeContext<'_>| ctx.try_collect_vec([Ok::<u8, CodecError>(1), Ok(2)], "test try collect vec").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.try_collect_vec([Ok::<u8, CodecError>(1), Ok(2)], "test try collect vec").map(|_| ()));
    collection_case!(insert_hash_set_charges_before_growth, 1,
        |ctx: &DecodeContext<'_>| ctx.insert_hash_set(&mut HashSet::new(), 1_u8, "test insert set").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.insert_hash_set(&mut HashSet::new(), 1_u8, "test insert set").map(|_| ()));
    collection_case!(insert_string_set_charges_before_growth, 1,
        |ctx: &DecodeContext<'_>| ctx.insert_string_set(&mut HashSet::new(), "a", "test insert string").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.insert_string_set(&mut HashSet::new(), "a", "test insert string").map(|_| ()));
    collection_case!(collect_hash_set_charges_before_growth, 2,
        |ctx: &DecodeContext<'_>| ctx.collect_hash_set([1_u8, 2], "test collect set").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.collect_hash_set([1_u8, 2], "test collect set").map(|_| ()));
    collection_case!(admit_hash_map_entry_charges_before_growth, 1,
        |ctx: &DecodeContext<'_>| ctx.admit_hash_map_entry(&mut HashMap::<u8, u8>::new(), &1, "test admit map"),
        |ctx: &DecodeContext<'_>| ctx.admit_hash_map_entry(&mut HashMap::<u8, u8>::new(), &1, "test admit map"));
    collection_case!(insert_hash_map_charges_before_growth, 1,
        |ctx: &DecodeContext<'_>| ctx.insert_hash_map(&mut HashMap::new(), 1_u8, 2_u8, "test insert map").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.insert_hash_map(&mut HashMap::new(), 1_u8, 2_u8, "test insert map").map(|_| ()));
    collection_case!(collect_hash_map_charges_before_growth, 2,
        |ctx: &DecodeContext<'_>| ctx.collect_hash_map([(1_u8, 2_u8), (3, 4)], "test collect map").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.collect_hash_map([(1_u8, 2_u8), (3, 4)], "test collect map").map(|_| ()));

    #[test]
    fn copy_scoped_text_refuses_before_allocation_and_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test context");
        let mut reservation = ctx.reserve_scoped(0, "test scoped text").expect("empty reserve");
        let result = ctx.copy_scoped_text("abc", &mut reservation, "test scoped text");
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut reservation = ctx.reserve_scoped(0, "test scoped text").expect("empty reserve");
        assert_eq!(ctx.copy_scoped_text("abc", &mut reservation, "test scoped text").expect("copy"), "abc");
    }
}
