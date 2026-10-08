// SPDX-License-Identifier: Apache-2.0
//! Charged growth of collections owned by a decode session.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};
use std::fmt::{self, Write};
use std::hash::Hash;

use crate::CodecError;

use super::cost::DecodeCost;
use super::extend_source::ExtendSource;
use super::text::TextSource;

use super::{
    u64_from_index, BoundedCount, DecodeContext, ResourceDimension, ResourceLimit,
    ScopedReservation,
};

/// A vector that must contain exactly a count proven against input.
#[derive(Debug)]
pub struct ExactVec<T> {
    values: Vec<T>,
    capacity: usize,
}

impl<T> ExactVec<T> {
    /// Charges and allocates storage for a count bounded by the input window.
    pub fn new(
        ctx: &DecodeContext<'_>,
        count: BoundedCount,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let capacity = count.get();
        let values = ctx.collection_vec(capacity, operation)?;
        Ok(Self { values, capacity })
    }

    /// Appends one value without exceeding the bounded count.
    pub fn push(
        &mut self,
        ctx: &DecodeContext<'_>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if self.values.len() == self.capacity {
            return Err(CodecError::Malformed(
                "fixed-capacity vector overflow".to_owned(),
            ));
        }
        ctx.charge_work(1, operation)?;
        ctx.reserve_capacity(&mut self.values, 1, operation)?;
        self.values.push(value);
        Ok(())
    }

    /// Returns the values if the bounded count was filled exactly.
    pub fn finish(self) -> Result<Vec<T>, CodecError> {
        if self.values.len() == self.capacity {
            Ok(self.values)
        } else {
            Err(CodecError::malformed(format_args!(
                "fixed-capacity vector contains {} of {} values",
                self.values.len(),
                self.capacity
            )))
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum LinearGrowth {
    Exact,
    Amortized,
    /// Exact byte storage whose input or expanded-byte charge precedes growth.
    PrechargedBytes,
}

impl DecodeContext<'_> {
    /// Charges the next source step before it can yield or run an adapter.
    /// Callers supply a fixed-step source or adapters over admitted bases.
    pub fn next_charged<I: Iterator>(
        &self,
        values: &mut I,
        operation: &'static str,
    ) -> Result<Option<I::Item>, CodecError> {
        self.charge_work(1, operation)?;
        Ok(values.next())
    }

    /// Reserve backing storage without admitting values not yet inserted.
    pub fn reserve_capacity_limit<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.reserve_retained_vec_storage(values, count, LinearGrowth::Amortized, None, operation)
    }

    fn reserve_retained_vec_storage<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        growth: LinearGrowth,
        items: Option<usize>,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.validate_vector_length::<T>(values.len(), count, operation)?;
        if let Some(items) = items {
            self.charge_collection_items_limit(u64_from_index(items), operation)?;
        }
        let (additional, bytes, _growth) =
            self.linear_growth::<T>(values.len(), values.capacity(), count, growth, operation)?;
        values.try_reserve_exact(additional).map_err(|_| {
            self.budget
                .retained_allocation_failed_limit(u64_from_index(bytes), operation)
        })?;
        Ok(())
    }

    /// Retains vector storage whose slots were admitted in aggregate.
    pub fn reserve_capacity<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_retained_vec_storage(values, count, LinearGrowth::Amortized, None, operation)
            .map_err(Into::into)
    }

    /// Creates retained storage whose collection slots are admitted separately.
    pub fn vector_storage<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut values = Vec::new();
        self.reserve_retained_vec_storage(
            &mut values,
            count,
            LinearGrowth::Exact,
            None,
            operation,
        )?;
        Ok(values)
    }

    /// Admits scoped collection slots whose storage is allocated by a later operation.
    pub fn reserve_scoped_collection<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, CodecError> {
        self.charge_collection_items(u64_from_index(count), operation)?;
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| self.budget.scoped_size_overflow_limit(operation))?;
        self.reserve_scoped(u64_from_index(bytes), operation)
    }

    /// Creates scoped storage whose collection slots are admitted separately.
    pub fn scoped_vector_storage<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(Vec<T>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let values = reservation.with_storage(|| self.vector_storage(count, operation))?;
        Ok((values, reservation))
    }

    /// Grows scoped storage and admits additional vector slots.
    pub fn reserve_scoped_vec<T>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_scoped_vec_limit(reservation, values, count, operation)
            .map_err(Into::into)
    }

    /// Reserves storage with a resource-only refusal channel.
    pub fn reserve_scoped_vec_limit<T>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        reservation.with_storage_limit(|| self.reserve_vec_limit(values, count, operation))
    }

    /// Appends one item with scoped storage and a charged collection slot.
    pub fn push_scoped_vec<T>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut Vec<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_scoped_vec(reservation, values, 1, operation)?;
        values.push(value);
        Ok(())
    }

    /// Reserves temporary vector storage and returns its live byte reservation.
    pub fn reserve_temporary_vec<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, ResourceLimit> {
        let mut reservation = self.reserve_scoped_limit(0, operation)?;
        reservation.with_storage_limit(|| {
            self.reserve_retained_vec_storage(
                values,
                count,
                LinearGrowth::Exact,
                Some(count),
                operation,
            )
        })?;
        Ok(reservation)
    }

    /// Copies a slice into scoped storage and returns its live byte reservation.
    pub fn copy_temporary_slice<T: Copy>(
        &self,
        values: &[T],
        operation: &'static str,
    ) -> Result<(Vec<T>, ScopedReservation<'_>), ResourceLimit> {
        let mut copy = Vec::new();
        let reservation = self.reserve_temporary_vec(&mut copy, values.len(), operation)?;
        self.charge_work_limit(u64_from_index(values.len()), operation)?;
        copy.extend_from_slice(values);
        Ok((copy, reservation))
    }

    /// Collects retained text and charges both vector storage and text bytes.
    pub fn collect_retained_texts<'text>(
        &self,
        values: impl IntoIterator<Item = &'text str>,
        operation: &'static str,
    ) -> Result<Vec<String>, CodecError> {
        self.try_collect_retained_with(values, operation, |value| {
            self.copy_retained_text(value, operation)
        })
    }

    /// Copies scoped text values and returns the reservation for their storage.
    pub fn collect_scoped_texts<'text>(
        &self,
        values: impl IntoIterator<Item = &'text str>,
        operation: &'static str,
    ) -> Result<(Vec<String>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let copies = reservation.with_storage(|| self.collect_retained_texts(values, operation))?;
        Ok((copies, reservation))
    }

    /// Formats text in an existing scope after charging its byte count as work.
    pub fn format_scoped_text(
        &self,
        reservation: &mut ScopedReservation<'_>,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        reservation.with_storage(|| self.format_retained(args, operation))
    }

    /// Admits records, retains their storage and additional text, and reserves slots.
    pub fn reserve_record_vec<T>(
        &self,
        records: &mut Vec<T>,
        count: usize,
        text_bytes: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_entities(u64_from_index(count), operation)?;
        self.charge_retained(text_bytes, operation)?;
        self.reserve_vec(records, count, operation)
    }

    /// Collects retained slots before adding each value.
    pub fn collect_retained_vec<T>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        self.try_collect_retained_with(values, operation, Ok::<T, CodecError>)
    }

    /// Inserts a new scoped tree key after charging lookup work and node storage.
    pub fn insert_scoped_btree_set<T: Ord + DecodeCost>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut BTreeSet<T>,
        value: T,
        work_operation: &'static str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        reservation.with_storage(|| {
            self.insert_btree_set_with_lookup(values, value, work_operation, operation)
        })
    }

    /// Inserts a vacant scoped tree entry after charging lookup work and node storage.
    pub fn insert_scoped_btree_map_if_vacant<K: Ord + DecodeCost, V>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut BTreeMap<K, V>,
        key: K,
        value: V,
        work_operation: &'static str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        reservation.with_storage(|| {
            if self.contains_key_btree_map(values, &key, work_operation)? {
                return Ok(false);
            }
            self.insert_btree_map(values, key, value, operation)?;
            Ok(true)
        })
    }

    /// Appends a lazily built value to a scoped group after aggregate admission.
    pub fn push_scoped_btree_group<K: Ord + DecodeCost, V>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        groups: &mut BTreeMap<K, Vec<V>>,
        key: K,
        value: impl FnOnce() -> V,
        owned_bytes: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        reservation.with_storage(|| {
            if let Some(values) = self.get_mut_btree_map(groups, &key, operation)? {
                self.reserve_vec(values, 1, operation)?;
                self.charge_retained(u64_from_index(owned_bytes), operation)?;
                values.push(value());
                return Ok(());
            }
            self.admit_btree_entry(groups, &key, operation)?;
            let mut values = self.collection_vec(1, operation)?;
            self.charge_retained(u64_from_index(owned_bytes), operation)?;
            values.push(value());
            self.charge_key(&key, Self::tree_comparisons(groups.len()), operation)?;
            groups.insert(key, values);
            Ok(())
        })
    }

    /// Admits tree-record storage, owned bytes, one slot and one work unit.
    ///
    /// Without the current tree length, each record admits one backing node.
    pub fn admit_retained_btree_record<K, V>(
        &self,
        owned_bytes: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let bytes = self
            .tree_growth_bytes::<K, V>(0, operation)?
            .checked_add(u64_from_index(owned_bytes))
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        self.charge_collection_items(1, operation)?;
        self.charge_retained(bytes, operation)?;
        self.charge_work(1, operation)
    }

    /// Collects scoped groups in input order within each key.
    pub fn collect_scoped_btree_groups<'ctx, K: Ord + DecodeCost, V>(
        &'ctx self,
        values: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<(BTreeMap<K, Vec<V>>, ScopedReservation<'ctx>), CodecError> {
        let mut groups = BTreeMap::new();
        let mut reservation = self.reserve_scoped(0, operation)?;
        let mut input = values.into_iter();
        loop {
            let Some((key, value)) = self.next_charged(&mut input, operation)? else {
                break;
            };
            self.push_scoped_btree_group(
                &mut reservation,
                &mut groups,
                key,
                || value,
                0,
                operation,
            )?;
        }
        Ok((groups, reservation))
    }

    /// Collects scoped entries, keeping the last value for each key.
    pub fn collect_scoped_btree_map<'ctx, K: Ord + DecodeCost, V>(
        &'ctx self,
        values: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<(BTreeMap<K, V>, ScopedReservation<'ctx>), CodecError> {
        let mut entries = BTreeMap::new();
        let mut reservation = self.reserve_scoped(0, operation)?;
        reservation.with_storage(|| {
            let mut input = values.into_iter();
            loop {
                let Some((key, value)) = self.next_charged(&mut input, operation)? else {
                    break;
                };
                self.insert_btree_map(&mut entries, key, value, operation)?;
            }
            Ok::<(), CodecError>(())
        })?;
        Ok((entries, reservation))
    }

    /// Grows a scoped string after admitting its additional byte storage.
    pub fn reserve_scoped_string(
        &self,
        reservation: &mut ScopedReservation<'_>,
        text: &mut String,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        reservation.with_storage(|| self.try_reserve_retained_text(text, additional, operation))
    }

    /// Creates empty scoped text with fallible storage admission.
    pub fn scoped_string(
        &self,
        length: usize,
        operation: &'static str,
    ) -> Result<(String, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let mut text = String::new();
        self.reserve_scoped_string(&mut reservation, &mut text, length, operation)?;
        Ok((text, reservation))
    }

    fn allocation_failed(
        &self,
        dimension: ResourceDimension,
        count: usize,
        operation: &'static str,
    ) -> CodecError {
        match dimension {
            ResourceDimension::CollectionItems => {
                self.collection_allocation_failed(count, operation)
            }
            ResourceDimension::MaterializedBytes => self
                .budget
                .scoped_allocation_failed(u64_from_index(count), operation),
            _ => self
                .budget
                .retained_allocation_failed(u64_from_index(count), operation),
        }
    }

    /// Allocates a vector for `count` admitted items.
    pub fn collection_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut values = Vec::new();
        self.reserve_retained_vec_storage(
            &mut values,
            count,
            LinearGrowth::Exact,
            Some(count),
            operation,
        )?;
        Ok(values)
    }

    /// Reserves additional vector items and charges their admission.
    pub fn reserve_vec<T>(
        &self,
        values: &mut Vec<T>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec_limit(values, additional, operation)
            .map_err(Into::into)
    }

    /// Reserves storage with a resource-only refusal channel.
    pub fn reserve_vec_limit<T>(
        &self,
        values: &mut Vec<T>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.reserve_retained_vec_storage(
            values,
            additional,
            LinearGrowth::Amortized,
            Some(additional),
            operation,
        )
    }

    /// Builds a vector of indexed values after charging all slots.
    pub fn collect_indexed_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
        mut value_at: impl FnMut(usize) -> Result<T, CodecError>,
    ) -> Result<Vec<T>, CodecError> {
        let mut values = self.collection_vec(count, operation)?;
        for index in 0..count {
            self.charge_work(1, operation)?;
            values.push(value_at(index)?);
        }
        Ok(values)
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

    /// Adds a formatted retained note after charging its collection slot.
    pub fn push_formatted_retained(
        &self,
        values: &mut Vec<String>,
        args: fmt::Arguments<'_>,
        slots_operation: &'static str,
        text_operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec(values, 1, slots_operation)?;
        values.push(self.format_retained(args, text_operation)?);
        Ok(())
    }

    /// Moves all source items into a vector after charging their slots.
    pub fn append_vec<T>(
        &self,
        target: &mut Vec<T>,
        source: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.admit_moves(source, 1, operation)?;
        self.reserve_vec(target, source.len(), operation)?;
        target.append(source);
        Ok(())
    }

    /// Moves owned items or copies borrowed `Copy` values after admitting
    /// their moves and the vector's growth.
    pub fn extend_vec<T>(
        &self,
        target: &mut Vec<T>,
        source: impl ExtendSource<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        source.extend_into(self, target, operation)
    }

    /// Collects iterator values with a charged slot for each value.
    pub fn collect_vec<T>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut out = Vec::new();
        let mut input = values.into_iter();
        while let Some(value) = self.next_charged(&mut input, operation)? {
            self.push_vec(&mut out, value, operation)?;
        }
        Ok(out)
    }

    /// Splits owned pairs into two vectors, admitting each source step and both slots.
    /// Adapted sources must start from admitted bases; child values move without cloning.
    pub fn unzip_vec<A, B>(
        &self,
        values: impl IntoIterator<Item = (A, B)>,
        operation: &'static str,
    ) -> Result<(Vec<A>, Vec<B>), CodecError> {
        let mut left = Vec::new();
        let mut right = Vec::new();
        let mut input = values.into_iter();
        loop {
            let Some((a, b)) = self.next_charged(&mut input, operation)? else {
                break;
            };
            self.push_vec(&mut left, a, operation)?;
            self.push_vec(&mut right, b, operation)?;
        }
        Ok((left, right))
    }

    /// Collects fallible iterator values with a charged slot for each success.
    pub fn try_collect_vec<T, E: From<CodecError>>(
        &self,
        values: impl IntoIterator<Item = Result<T, E>>,
        operation: &'static str,
    ) -> Result<Vec<T>, E> {
        self.try_collect_vec_with(values, operation, |out, value| {
            self.push_vec(out, value, operation).map_err(E::from)
        })
    }

    /// Collects temporary slots while producer allocations remain retained.
    /// Keep the returned reservation alive with the vector.
    pub fn try_collect_scoped_vec<'ctx, T, E: From<CodecError>>(
        &'ctx self,
        values: impl IntoIterator<Item = Result<T, E>>,
        operation: &'static str,
    ) -> Result<(Vec<T>, ScopedReservation<'ctx>), E> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let output = self.try_collect_vec_with(values, operation, |out, value| {
            self.push_scoped_vec(&mut reservation, out, value, operation)
                .map_err(E::from)
        })?;
        Ok((output, reservation))
    }

    fn try_collect_vec_with<T, E: From<CodecError>>(
        &self,
        values: impl IntoIterator<Item = Result<T, E>>,
        operation: &'static str,
        mut push: impl FnMut(&mut Vec<T>, T) -> Result<(), E>,
    ) -> Result<Vec<T>, E> {
        let mut out = Vec::new();
        let mut input = values.into_iter();
        while let Some(value) = self.next_charged(&mut input, operation)? {
            push(&mut out, value?)?;
        }
        Ok(out)
    }

    /// Inserts a unique tree value after admitting its scoped value storage.
    pub fn insert_scoped_btree_value<T: Ord + DecodeCost>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut BTreeSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        reservation.with_storage(|| self.insert_btree_set(values, value, operation))
    }

    /// Inserts a new set item after charging its slot.
    pub fn insert_hash_set<T: Eq + Hash + DecodeCost>(
        &self,
        values: &mut HashSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if self.contains_hash_set(values, &value, operation)? {
            return Ok(false);
        }
        self.reserve_set(values, 1, operation)?;
        self.charge_key(&value, 1, operation)?;
        Ok(values.insert(value))
    }

    /// Inserts a copied string into a set if the value is new.
    pub fn insert_string_set(
        &self,
        values: &mut HashSet<String>,
        value: &str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if self.contains_hash_set(values, value, operation)? {
            return Ok(false);
        }
        self.reserve_set(values, 1, operation)?;
        let owned = self.copy_retained_text(value, operation)?;
        self.charge_key(&owned, 1, operation)?;
        Ok(values.insert(owned))
    }

    /// Extends a set with one charged slot for each distinct new value.
    pub fn extend_hash_set<T: Eq + Hash + DecodeCost>(
        &self,
        values: &mut HashSet<T>,
        additions: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let mut input = additions.into_iter();
        while let Some(value) = self.next_charged(&mut input, operation)? {
            self.insert_hash_set(values, value, operation)?;
        }
        Ok(())
    }

    /// Collects distinct values into a charged hash set.
    pub fn collect_hash_set<T: Eq + Hash + DecodeCost>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<HashSet<T>, CodecError> {
        let mut out = HashSet::new();
        let mut input = values.into_iter();
        while let Some(value) = self.next_charged(&mut input, operation)? {
            self.insert_hash_set(&mut out, value, operation)?;
        }
        Ok(out)
    }

    /// Reserves a map slot only when the key is new.
    pub fn admit_hash_map_entry<K: Eq + Hash + DecodeCost, V>(
        &self,
        values: &mut HashMap<K, V>,
        key: &K,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if !self.contains_key_hash_map(values, key, operation)? {
            self.reserve_map(values, 1, operation)?;
        }
        Ok(())
    }

    /// Appends a grouped value after admitting both new index and value slots.
    pub fn push_hash_group<K: Eq + Hash + DecodeCost, V>(
        &self,
        values: &mut HashMap<K, Vec<V>>,
        key: K,
        value: V,
        index_operation: &'static str,
        value_operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(group) = self.get_mut_hash_map(values, &key, index_operation)? {
            return self.push_vec(group, value, value_operation);
        }
        self.charge_collection_items(1, index_operation)?;
        self.charge_collection_items(1, value_operation)?;
        self.reserve_hash_map_storage(values, 1, index_operation)?;
        let mut group = self.vector_storage(1, value_operation)?;
        group.push(value);
        self.charge_key(&key, 1, index_operation)?;
        values.insert(key, group);
        Ok(())
    }

    /// Inserts a map entry after admitting a new key, if needed.
    pub fn insert_hash_map<K: Eq + Hash + DecodeCost, V>(
        &self,
        values: &mut HashMap<K, V>,
        key: K,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError> {
        self.admit_hash_map_entry(values, &key, operation)?;
        self.charge_key(&key, 1, operation)?;
        Ok(values.insert(key, value))
    }

    /// Collects entries into a charged hash map.
    pub fn collect_hash_map<K: Eq + Hash + DecodeCost, V>(
        &self,
        values: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<HashMap<K, V>, CodecError> {
        let mut out = HashMap::new();
        let mut input = values.into_iter();
        loop {
            let Some((key, value)) = self.next_charged(&mut input, operation)? else {
                break;
            };
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
        reservation.with_storage(|| self.copy_retained_text(text, operation))
    }

    /// Rejects an unrepresentable allocation before collection or work admission.
    pub(super) fn validate_vector_length<T>(
        &self,
        len: usize,
        count: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        let bytes = len
            .checked_add(count)
            .and_then(|length| length.checked_mul(std::mem::size_of::<T>()))
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let maximum = usize::try_from(isize::MAX)
            .map_err(|_| self.retained_size_overflow_limit(operation))?;
        if bytes > maximum {
            return Err(self
                .budget
                .retained_allocation_failed_limit(u64_from_index(bytes), operation));
        }
        Ok(())
    }

    /// Admits capacity growth and all bytes that reallocation can move.
    pub(super) fn linear_growth<T>(
        &self,
        len: usize,
        capacity: usize,
        count: usize,
        growth: LinearGrowth,
        operation: &'static str,
    ) -> Result<(usize, usize, ScopedReservation<'_>), ResourceLimit> {
        let required = len
            .checked_add(count)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        if std::mem::size_of::<T>() == 0 || required <= capacity {
            self.charge_retained_limit(0, operation)?;
            return Ok((0, 0, self.reserve_scoped_limit(0, operation)?));
        }
        let target = if matches!(growth, LinearGrowth::Amortized) {
            let minimum = match std::mem::size_of::<T>() {
                1 => 8,
                2..=1024 => 4,
                _ => 1,
            };
            capacity
                .checked_mul(2)
                .ok_or_else(|| self.retained_size_overflow_limit(operation))?
                .max(required)
                .max(minimum)
        } else {
            required
        };
        let target_bytes = target
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let maximum = usize::try_from(isize::MAX)
            .map_err(|_| self.retained_size_overflow_limit(operation))?;
        if target_bytes > maximum {
            return Err(self
                .budget
                .retained_allocation_failed_limit(u64_from_index(target_bytes), operation));
        }
        let bytes = (target - capacity) * std::mem::size_of::<T>();
        if !matches!(growth, LinearGrowth::PrechargedBytes) {
            self.charge_retained_limit(u64_from_index(bytes), operation)?;
        }
        let moved = capacity
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| self.refuse_local_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_work_limit(u64_from_index(moved), operation)?;
        let overlap = self.reserve_scoped_limit(u64_from_index(moved), operation)?;
        Ok((target - len, bytes, overlap))
    }

    /// Reserves precharged input or expansion bytes with admitted move work
    /// and overlap storage. The caller selects its allocation failure dimension.
    pub(super) fn reserve_precharged_bytes(
        &self,
        values: &mut Vec<u8>,
        count: usize,
        operation: &'static str,
        allocation_failed: impl FnOnce(u64) -> CodecError,
    ) -> Result<(), CodecError> {
        let (additional, storage, _growth) = self.linear_growth::<u8>(
            values.len(),
            values.capacity(),
            count,
            LinearGrowth::PrechargedBytes,
            operation,
        )?;
        values
            .try_reserve_exact(additional)
            .map_err(|_| allocation_failed(u64_from_index(storage)))
    }

    /// Appends a deque item after charging its slot.
    pub fn push_back<T>(
        &self,
        values: &mut VecDeque<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let (additional, bytes, _growth) = self.linear_growth::<T>(
            values.len(),
            values.capacity(),
            1,
            LinearGrowth::Amortized,
            operation,
        )?;
        self.charge_collection_items(1, operation)?;
        if additional != 0 {
            // Wrapped deque growth can move the live slots after reallocation.
            let moved = values
                .len()
                .checked_mul(std::mem::size_of::<T>())
                .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
            self.charge_work(u64_from_index(moved), operation)?;
        }
        values.try_reserve_exact(additional).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(bytes), operation)
        })?;
        values.push_back(value);
        Ok(())
    }

    /// Prepends a deque item after charging its slot.
    pub fn push_front<T>(
        &self,
        values: &mut VecDeque<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let (additional, bytes, _growth) = self.linear_growth::<T>(
            values.len(),
            values.capacity(),
            1,
            LinearGrowth::Amortized,
            operation,
        )?;
        self.charge_collection_items(1, operation)?;
        if additional != 0 {
            // Wrapped deque growth can move the live slots after reallocation.
            let moved = values
                .len()
                .checked_mul(std::mem::size_of::<T>())
                .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
            self.charge_work(u64_from_index(moved), operation)?;
        }
        values.try_reserve_exact(additional).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(bytes), operation)
        })?;
        values.push_front(value);
        Ok(())
    }

    /// Reserves slots in a binary heap after charging them.
    pub fn reserve_heap<T: Ord>(
        &self,
        values: &mut BinaryHeap<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let (additional, bytes, _growth) = self.linear_growth::<T>(
            values.len(),
            values.capacity(),
            count,
            LinearGrowth::Amortized,
            operation,
        )?;
        self.charge_collection_items(u64_from_index(count), operation)?;
        values.try_reserve_exact(additional).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(bytes), operation)
        })
    }

    /// Creates a charged vector only when the source is present.
    pub fn optional_collection_vec<T>(
        &self,
        present: bool,
        count: usize,
        operation: &'static str,
    ) -> Result<Option<Vec<T>>, CodecError> {
        if present {
            Ok(Some(self.collection_vec(count, operation)?))
        } else {
            Ok(None)
        }
    }

    /// Copies a flat lane into rows after admitting every row and element slot.
    pub fn copy_rows<T: Copy>(
        &self,
        values: &[T],
        row_len: usize,
        row_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<Vec<Vec<T>>, CodecError> {
        if row_len == 0 {
            return Err(CodecError::malformed("row width must be nonzero"));
        }
        self.try_collect_retained_with(values.chunks(row_len), row_operation, |row| {
            self.copy_slice(row, item_operation)
        })
    }

    /// Collects fallible values and admits the storage of each output slot.
    pub fn try_collect_retained_with<I, T, E: From<CodecError>>(
        &self,
        values: impl IntoIterator<Item = I>,
        operation: &'static str,
        mut map: impl FnMut(I) -> Result<T, E>,
    ) -> Result<Vec<T>, E> {
        let mut values = values.into_iter();
        let (minimum, maximum) = values.size_hint();
        let mut collected = Vec::new();
        loop {
            let Some(value) = self.next_charged(&mut values, operation)? else {
                break;
            };
            if collected.is_empty() && maximum.is_some_and(|maximum: usize| maximum == minimum) {
                self.reserve_retained_vec_storage(
                    &mut collected,
                    minimum.max(1),
                    LinearGrowth::Exact,
                    Some(1),
                    operation,
                )
                .map_err(CodecError::from)?;
            } else {
                self.reserve_retained_vec_storage(
                    &mut collected,
                    1,
                    LinearGrowth::Amortized,
                    Some(1),
                    operation,
                )
                .map_err(CodecError::from)?;
            }
            let mapped = map(value)?;
            self.reserve_capacity(&mut collected, 1, operation)
                .map_err(E::from)?;
            collected.push(mapped);
        }
        Ok(collected)
    }

    /// Copies a slice after charging its collection slots.
    pub fn copy_slice<T: Copy>(
        &self,
        values: &[T],
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        self.charge_work(u64_from_index(values.len()), operation)?;
        let mut copy = self.collection_vec(values.len(), operation)?;
        copy.extend_from_slice(values);
        Ok(copy)
    }

    /// Collects optional values, stopping at the first absent value.
    pub fn collect_options<T>(
        &self,
        values: impl IntoIterator<Item = Option<T>>,
        operation: &'static str,
    ) -> Result<Option<Vec<T>>, CodecError> {
        let mut collected = Vec::new();
        let mut input = values.into_iter();
        while let Some(value) = self.next_charged(&mut input, operation)? {
            let Some(value) = value else { return Ok(None) };
            self.push_vec(&mut collected, value, operation)?;
        }
        Ok(Some(collected))
    }

    /// Collects fallible optional values, stopping at the first absent value.
    pub fn collect_fallible_options<T, E: Into<CodecError>>(
        &self,
        values: impl IntoIterator<Item = Result<Option<T>, E>>,
        operation: &'static str,
    ) -> Result<Option<Vec<T>>, CodecError> {
        let mut collected = Vec::new();
        let mut input = values.into_iter();
        while let Some(value) = self.next_charged(&mut input, operation)? {
            let Some(value) = value.map_err(Into::into)? else {
                return Ok(None);
            };
            self.push_vec(&mut collected, value, operation)?;
        }
        Ok(Some(collected))
    }

    /// Collects retained strings into a charged hash set.
    pub fn collect_string_set<'a>(
        &self,
        values: impl IntoIterator<Item = &'a str>,
        operation: &'static str,
    ) -> Result<HashSet<String>, CodecError> {
        let mut collected = HashSet::new();
        let mut input = values.into_iter();
        while let Some(value) = self.next_charged(&mut input, operation)? {
            self.insert_string_set(&mut collected, value, operation)?;
        }
        Ok(collected)
    }

    /// Copies retained rows and charges row and item slots.
    pub fn copy_retained_rows<T: Copy>(
        &self,
        rows: &[Vec<T>],
        row_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<Vec<Vec<T>>, CodecError> {
        self.try_collect_retained_with(rows, row_operation, |row| {
            self.copy_slice(row, item_operation)
        })
    }

    /// Charges rehashing every stored key when hash-table growth reallocates.
    /// Fixed-cost keys are charged without visiting the table. Variable-cost
    /// keys are measured where they are stored, and their sum, with one visit
    /// per key, is charged as one amount, so the unspecified visit order
    /// changes neither the total nor where a refusal falls. A key whose own
    /// measurement admits child traversal charges that inside the walk.
    /// The caller charges growth first. When the table reallocates, that work
    /// bounds the old table's storage, at least two units per bucket, which
    /// pays for this measuring walk and the rehash's own walk. When it rehashes
    /// in place, both walks are paid by the charged removals and insertions
    /// since the last rehash, as `charge_hash_growth` states.
    fn charge_rehash<'keys, K: DecodeCost + 'keys>(
        &self,
        len: usize,
        keys: impl Iterator<Item = &'keys K>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let len = u64_from_index(len);
        self.charge_work(len, operation)?;
        if let Some(bytes) = K::FIXED_BYTES {
            return self.charge_work(self.cost_product(len, bytes, operation)?, operation);
        }
        let mut bytes = len;
        for key in keys {
            bytes = self.cost_sum(bytes, key.decode_cost(self, operation)?, operation)?;
        }
        self.charge_work(bytes, operation)
    }

    // SwissTable has a maximum 7/8 load, power-of-two bucket storage, and
    // at most 16 trailing control bytes. Small elements require 16 bytes
    // of bucket storage. Alignment padding is bounded by max(align, 16)-1.
    fn hash_storage_bytes<T>(
        &self,
        capacity: usize,
        operation: &'static str,
    ) -> Result<usize, ResourceLimit> {
        if capacity == 0 {
            return Ok(0);
        }
        let buckets = if capacity < 4 {
            Some(4)
        } else if capacity < 8 {
            Some(8)
        } else {
            capacity
                .checked_mul(8)
                .and_then(|slots| (slots / 7).checked_next_power_of_two())
        }
        .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let alignment = std::mem::align_of::<T>().max(16);
        buckets
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|bytes| bytes.checked_add(alignment - 1))
            .and_then(|bytes| bytes.checked_add(buckets))
            .and_then(|bytes| bytes.checked_add(16))
            .ok_or_else(|| self.retained_size_overflow_limit(operation))
    }

    // `capacity()` is the live entries plus the remaining growth allowance; a
    // removal leaves a deleted slot that counts in neither, so the real table
    // can hold more buckets than `capacity()` implies. Hashbrown grows only
    // when the new length exceeds `capacity()`. It then reallocates only when
    // the new length exceeds half the real capacity, to the larger of the new
    // length and the real capacity plus one: at most twice the new length.
    // Otherwise it rehashes in place and allocates nothing. Storage for twice
    // the new length therefore bounds the old table a reallocation walks and
    // the new table it allocates: growth charges that bound as work and holds
    // it as a scoped reservation while the table grows, so a refusal comes
    // before the allocation. Right after a reallocation or an in-place rehash
    // the table has no deleted slots, so its new `capacity()` is real; growth
    // then charges the storage of the new `capacity()` less that of the old as
    // retained bytes. Earlier growth charged at least the old table, whose
    // storage is at least that of the old `capacity()`, so the retained total
    // stays at or above the real allocation. An in-place rehash runs only
    // when at least half the real capacity has been removed or filled since
    // the last rehash, so the charged removals and insertions since then pay
    // for its walk.
    //
    // Under churn the retained total exceeds the real allocation. Std exposes
    // neither the bucket count nor the deleted slots, so growth after
    // removals cannot tell an in-place rehash from a doubling and charges as
    // if the table doubled. That over-count is nonzero only when the old
    // `capacity()` maps to fewer buckets than the table holds, which takes
    // removals of at least half the real capacity since the last rehash,
    // and it is at most half the table's storage. Each removal therefore
    // adds at most 8/7 of an entry and its control byte: no more than a
    // table that had kept the removed entry would have retained for it.
    // Exact tracking needs the bucket count kept beside the table.
    fn charge_hash_growth<T>(
        &self,
        len: usize,
        capacity: usize,
        count: usize,
        operation: &'static str,
    ) -> Result<(u64, ScopedReservation<'_>), ResourceLimit> {
        let required = len
            .checked_add(count)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        if required <= capacity {
            return Ok((0, self.reserve_scoped_limit(0, operation)?));
        }
        let minimum = match std::mem::size_of::<T>() {
            0..=1 => 14,
            2..=3 => 7,
            _ => 3,
        };
        let largest = required
            .checked_mul(2)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?
            .max(minimum);
        let bound = u64_from_index(self.hash_storage_bytes::<T>(largest, operation)?);
        self.charge_work_limit(bound, operation)?;
        Ok((bound, self.reserve_scoped_limit(bound, operation)?))
    }

    /// Retains a grown table's storage from its real capacity, less the
    /// storage of the capacity counted before it grew.
    fn retain_hash_growth<T>(
        &self,
        counted: usize,
        capacity: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        let grown = self.hash_storage_bytes::<T>(capacity, operation)?;
        let before = self.hash_storage_bytes::<T>(counted, operation)?;
        if grown > before {
            self.charge_retained_limit(u64_from_index(grown - before), operation)?;
        }
        Ok(())
    }

    /// Reserves hash set entries after charging their slots.
    pub fn reserve_set<T: Eq + Hash + DecodeCost>(
        &self,
        values: &mut HashSet<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_collection_items(u64_from_index(count), operation)?;
        self.reserve_hash_set_storage(values, count, operation)
    }

    fn reserve_hash_set_storage<T: Eq + Hash + DecodeCost>(
        &self,
        values: &mut HashSet<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let required = values
            .len()
            .checked_add(count)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let counted = values.capacity();
        let (bound, growth) =
            self.charge_hash_growth::<T>(values.len(), values.capacity(), count, operation)?;
        if required > counted {
            self.charge_rehash(values.len(), values.iter(), operation)?;
        }
        values
            .try_reserve(count)
            .map_err(|_| self.budget.retained_allocation_failed(bound, operation))?;
        drop(growth);
        Ok(self.retain_hash_growth::<T>(counted, values.capacity(), operation)?)
    }

    /// Reserves hash map entries after charging their slots.
    pub fn reserve_map<K: Eq + Hash + DecodeCost, V>(
        &self,
        values: &mut HashMap<K, V>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_collection_items(u64_from_index(count), operation)?;
        self.reserve_hash_map_storage(values, count, operation)
    }

    fn reserve_hash_map_storage<K: Eq + Hash + DecodeCost, V>(
        &self,
        values: &mut HashMap<K, V>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let required = values
            .len()
            .checked_add(count)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let counted = values.capacity();
        let (bound, growth) =
            self.charge_hash_growth::<(K, V)>(values.len(), values.capacity(), count, operation)?;
        if required > counted {
            self.charge_rehash(values.len(), values.keys(), operation)?;
        }
        values
            .try_reserve(count)
            .map_err(|_| self.budget.retained_allocation_failed(bound, operation))?;
        drop(growth);
        Ok(self.retain_hash_growth::<(K, V)>(counted, values.capacity(), operation)?)
    }

    // A B-tree node stores at most 11 key/value lanes, 12 child pointers,
    // and a parent pointer plus length/index metadata. The bound includes
    // four pointer-width metadata slots and padding for both lane arrays.
    // A tree built by insertion has at most (n - 1) / 5 + 1 nodes for n > 0,
    // and none when empty: every node except the root keeps at least five keys.
    // Admit the increase of that bound before each insertion. Since insertion
    // frees no nodes, cumulative admission bounds the live node storage.
    fn tree_growth_bytes<K, V>(
        &self,
        len: usize,
        operation: &'static str,
    ) -> Result<u64, ResourceLimit> {
        let nodes = self.tree_node_increase(len, operation)?;
        self.tree_node_bytes::<K, V>(nodes, operation)
    }

    /// How much one insertion raises the node bound (n - 1) / 5 + 1.
    fn tree_node_increase(
        &self,
        len: usize,
        operation: &'static str,
    ) -> Result<usize, ResourceLimit> {
        let next = len
            .checked_add(1)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let before = if len == 0 { 0 } else { (len - 1) / 5 + 1 };
        Ok((next - 1) / 5 + 1 - before)
    }

    fn tree_node_bytes<K, V>(
        &self,
        nodes: usize,
        operation: &'static str,
    ) -> Result<u64, ResourceLimit> {
        let alignment = std::mem::align_of::<K>()
            .max(std::mem::align_of::<V>())
            .max(std::mem::align_of::<usize>());
        std::mem::size_of::<K>()
            .checked_add(std::mem::size_of::<V>())
            .and_then(|bytes| bytes.checked_mul(11))
            .and_then(|bytes| bytes.checked_add(16 * std::mem::size_of::<usize>()))
            .and_then(|bytes| bytes.checked_add(2 * alignment))
            .and_then(|bytes| bytes.checked_mul(nodes))
            .map(u64_from_index)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))
    }

    // Work is counted in node passes: one pass is the node byte bound, the
    // most a slot shift within one node moves. A split moves at most one
    // node's contents into its new sibling and shifts its parent's slots: two
    // passes. A merge moves at most one node's contents into its sibling and
    // shifts the parent's slots, and a steal shifts two nodes: two passes
    // each. Every split creates a node, and every node a tree holds beyond its
    // first was created by a split since the tree was empty, or since a merge
    // freed a node. A tree of n entries holds at most (n - 1) / 5 + 1 nodes,
    // so the splits so far are at most the increases of that bound over the
    // insertions so far plus the nodes merges have freed. An insertion
    // therefore pays one shift and two passes for each node its length adds
    // to the bound. A removal pays one shift, one steal, and for every level
    // of the tree a merge and the split that may later recreate its node.
    // Each charge precedes its operation, so the charged total covers the
    // work at every point. Std's eager merges let alternating insertions and
    // removals split and merge a whole path each time, which the per-level
    // removal charge pays for.
    pub(super) fn admit_tree_insertion<K, V>(
        &self,
        len: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let added = self.tree_node_increase(len, operation)?;
        let passes = added
            .checked_mul(2)
            .and_then(|passes| passes.checked_add(1))
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let bytes = self.tree_node_bytes::<K, V>(passes, operation)?;
        self.charge_work(bytes, operation)
    }

    /// Admits the shift, steal and per-level merge work of one B-tree removal,
    /// as `admit_tree_insertion` states. A tree of at most ten entries is one
    /// node, which a removal only shifts.
    pub(super) fn admit_tree_removal_work<K, V>(
        &self,
        len: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let height = usize::try_from(Self::tree_height(len))
            .map_err(|_| self.retained_size_overflow_limit(operation))?;
        let passes = if height <= 1 {
            Some(1)
        } else {
            height
                .checked_mul(4)
                .and_then(|passes| passes.checked_add(3))
        }
        .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let bytes = self.tree_node_bytes::<K, V>(passes, operation)?;
        self.charge_work(bytes, operation)
    }

    /// Admits the backing nodes for one ordered entry whose slot is already charged.
    pub fn admit_btree_node_storage<K, V>(
        &self,
        len: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.admit_tree_insertion::<K, V>(len, operation)?;
        self.charge_retained(self.tree_growth_bytes::<K, V>(len, operation)?, operation)
    }

    /// Charges a new B-tree map key before insertion.
    pub fn admit_btree_entry<K: Ord + DecodeCost, V>(
        &self,
        values: &BTreeMap<K, V>,
        key: &K,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if !self.contains_key_btree_map(values, key, operation)? {
            self.admit_btree_node_storage::<K, V>(values.len(), operation)?;
            self.charge_collection_items(1, operation)?;
        }
        Ok(())
    }

    /// Inserts a B-tree map entry after charging a new key.
    pub fn insert_btree_map<K: Ord + DecodeCost, V>(
        &self,
        values: &mut BTreeMap<K, V>,
        key: K,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError> {
        self.admit_btree_entry(values, &key, operation)?;
        self.charge_key(&key, Self::tree_comparisons(values.len()), operation)?;
        Ok(values.insert(key, value))
    }

    /// Inserts a B-tree set item after charging a new value.
    pub fn insert_btree_set<T: Ord + DecodeCost>(
        &self,
        values: &mut BTreeSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        self.insert_btree_set_with_lookup(values, value, operation, operation)
    }

    fn insert_btree_set_with_lookup<T: Ord + DecodeCost>(
        &self,
        values: &mut BTreeSet<T>,
        value: T,
        lookup_operation: &'static str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if self.contains_btree_set(values, &value, lookup_operation)? {
            return Ok(false);
        }
        self.admit_btree_node_storage::<T, ()>(values.len(), operation)?;
        self.charge_collection_items(1, operation)?;
        self.charge_key(&value, Self::tree_comparisons(values.len()), operation)?;
        Ok(values.insert(value))
    }

    /// Collects distinct ordered values after admitting each new entry.
    pub fn collect_btree_set<T: Ord + DecodeCost>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<BTreeSet<T>, CodecError> {
        let mut out = BTreeSet::new();
        let mut input = values.into_iter();
        while let Some(value) = self.next_charged(&mut input, operation)? {
            self.insert_btree_set(&mut out, value, operation)?;
        }
        Ok(out)
    }

    /// Appends an ordered group member after admitting its group and slot.
    pub fn push_btree_group<K: Ord + DecodeCost, V>(
        &self,
        groups: &mut BTreeMap<K, Vec<V>>,
        key: K,
        value: V,
        group_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(values) = self.get_mut_btree_map(groups, &key, group_operation)? {
            return self.push_vec(values, value, item_operation);
        }
        self.admit_btree_entry(groups, &key, group_operation)?;
        let mut values = self.collection_vec(1, item_operation)?;
        values.push(value);
        self.charge_key(&key, Self::tree_comparisons(groups.len()), group_operation)?;
        groups.insert(key, values);
        Ok(())
    }

    /// Inserts an ordered group member after admitting its group and value.
    pub fn insert_btree_group_set<K: Ord + DecodeCost, V: Ord + DecodeCost>(
        &self,
        groups: &mut BTreeMap<K, BTreeSet<V>>,
        key: K,
        value: V,
        group_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(values) = self.get_mut_btree_map(groups, &key, group_operation)? {
            self.insert_btree_set(values, value, item_operation)?;
            return Ok(());
        }
        self.admit_btree_entry(groups, &key, group_operation)?;
        let mut values = BTreeSet::new();
        self.insert_btree_set(&mut values, value, item_operation)?;
        self.charge_key(&key, Self::tree_comparisons(groups.len()), group_operation)?;
        groups.insert(key, values);
        Ok(())
    }

    /// Resizes a byte buffer after admitting additional retained storage.
    pub fn resize_retained_bytes(
        &self,
        values: &mut Vec<u8>,
        length: usize,
        fill: u8,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(additional) = length.checked_sub(values.len()) {
            self.reserve_capacity(values, additional, operation)?;
            for _ in 0..additional {
                self.charge_work(1, operation)?;
                self.reserve_capacity(values, 1, operation)?;
                values.push(fill);
            }
        } else {
            self.truncate_vec(values, length, operation)?;
        }
        Ok(())
    }

    /// Extends retained bytes after charging both their slots and storage.
    pub fn extend_retained_bytes(
        &self,
        target: &mut Vec<u8>,
        source: &[u8],
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec(target, source.len(), operation)?;
        self.charge_work(u64_from_index(source.len()), operation)?;
        target.extend_from_slice(source);
        Ok(())
    }

    /// Reserves a scoped vector and returns its live reservation.
    pub fn temporary_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(Vec<T>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let values = reservation.with_storage(|| self.collection_vec(count, operation))?;
        Ok((values, reservation))
    }

    /// Reserves a scoped hash set and returns its live reservation.
    pub fn temporary_set<T: Eq + Hash>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(HashSet<T>, ScopedReservation<'_>), CodecError> {
        self.temporary_set_limit(count, operation)
            .map_err(Into::into)
    }

    /// Reserves a scoped hash set with a typed resource error.
    pub fn temporary_set_limit<T: Eq + Hash>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(HashSet<T>, ScopedReservation<'_>), ResourceLimit> {
        let mut reservation = self.reserve_scoped_limit(0, operation)?;
        let mut values = HashSet::new();
        reservation.with_storage_limit(|| {
            let (bound, growth) =
                self.charge_hash_growth::<T>(values.len(), values.capacity(), count, operation)?;
            self.charge_collection_items_limit(u64_from_index(count), operation)?;
            values.try_reserve(count).map_err(|_| {
                self.budget
                    .retained_allocation_failed_limit(bound, operation)
            })?;
            drop(growth);
            self.retain_hash_growth::<T>(0, values.capacity(), operation)
        })?;
        Ok((values, reservation))
    }

    /// Collects a borrowed string index with scoped hash storage.
    pub fn collect_scoped_string_set<'text>(
        &self,
        count: usize,
        values: impl IntoIterator<Item = &'text str>,
        operation: &'static str,
    ) -> Result<(HashSet<&'text str>, ScopedReservation<'_>), CodecError> {
        let (mut output, mut reservation) = self.temporary_set(count, operation)?;
        let mut remaining = count;
        reservation.with_storage(|| {
            let mut input = values.into_iter();
            while let Some(value) = self.next_charged(&mut input, operation)? {
                if !self.contains_hash_set(&output, value, operation)? {
                    if remaining == 0 {
                        self.charge_collection_items(1, operation)?;
                    } else {
                        remaining -= 1;
                    }
                    self.reserve_hash_set_storage(&mut output, 1, operation)?;
                    self.charge_key(value, 1, operation)?;
                    output.insert(value);
                }
            }
            Ok::<(), CodecError>(())
        })?;
        Ok((output, reservation))
    }

    /// Collects a borrowed string-keyed index with scoped hash storage.
    pub fn collect_scoped_string_map<'text, V>(
        &self,
        count: usize,
        values: impl IntoIterator<Item = (&'text str, V)>,
        operation: &'static str,
    ) -> Result<(HashMap<&'text str, V>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let mut output = HashMap::new();
        reservation.with_storage(|| {
            self.reserve_map(&mut output, count, operation)?;
            let mut remaining = count;
            let mut input = values.into_iter();
            loop {
                let Some((key, value)) = self.next_charged(&mut input, operation)? else {
                    break;
                };
                if let Some(stored) = self.get_mut_hash_map(&mut output, key, operation)? {
                    *stored = value;
                    continue;
                }
                if remaining == 0 {
                    self.charge_collection_items(1, operation)?;
                } else {
                    remaining -= 1;
                }
                self.reserve_hash_map_storage(&mut output, 1, operation)?;
                self.charge_key(key, 1, operation)?;
                output.insert(key, value);
            }
            Ok::<(), CodecError>(())
        })?;
        Ok((output, reservation))
    }

    /// Reserves a scoped deque and returns its live reservation.
    pub fn temporary_queue<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(VecDeque<T>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let mut values = VecDeque::new();
        reservation.with_storage(|| {
            let bytes = count
                .checked_mul(std::mem::size_of::<T>())
                .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
            self.charge_retained(u64_from_index(bytes), operation)?;
            self.charge_collection_items(u64_from_index(count), operation)?;
            values.try_reserve_exact(count).map_err(|_| {
                self.budget
                    .retained_allocation_failed(u64_from_index(bytes), operation)
            })
        })?;
        Ok((values, reservation))
    }

    /// Charges retained text produced by a later aggregate materialization.
    pub fn charge_formatted_retained(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let length = self.formatted_length(args, operation)?;
        self.charge_retained(u64_from_index(length), operation)
    }

    /// Formats a retained string after charging its exact byte count.
    pub fn format_retained(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let mut text = String::new();
        self.append_formatted_retained(&mut text, args, operation)?;
        Ok(text)
    }

    /// Formats a temporary string and returns its live reservation.
    pub fn format_scoped(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<(String, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let text = self.format_scoped_text(&mut reservation, args, operation)?;
        Ok((text, reservation))
    }

    /// Appends retained text after charging its bytes.
    pub fn append_retained(
        &self,
        output: &mut String,
        suffix: &str,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(suffix.len()), operation)?;
        self.try_reserve_retained_text(output, suffix.len(), operation)?;
        output.push_str(suffix);
        Ok(())
    }

    /// Appends one retained character after charging its UTF-8 bytes.
    pub fn push_retained_char(
        &self,
        output: &mut String,
        value: char,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let length = value.len_utf8();
        self.charge_work(u64_from_index(length), operation)?;
        self.try_reserve_retained_text(output, length, operation)?;
        output.push(value);
        Ok(())
    }

    /// Appends formatted text after admitting its exact retained byte count.
    pub fn append_formatted_retained(
        &self,
        output: &mut String,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        struct Output<'ctx, 'arena, 'text> {
            ctx: &'ctx DecodeContext<'arena>,
            text: &'text mut String,
            operation: &'static str,
            refusal: Option<CodecError>,
        }
        impl Write for Output<'_, '_, '_> {
            fn write_str(&mut self, fragment: &str) -> fmt::Result {
                if let Err(error) = self
                    .ctx
                    .append_retained(self.text, fragment, self.operation)
                {
                    self.refusal = Some(error);
                    return Err(fmt::Error);
                }
                Ok(())
            }
        }
        let length = self.formatted_length(args, operation)?;
        self.try_reserve_retained_text(output, length, operation)?;
        let mut writer = Output {
            ctx: self,
            text: output,
            operation,
            refusal: None,
        };
        match fmt::write(&mut writer, args) {
            Ok(()) => Ok(()),
            Err(error) => Err(match writer.refusal {
                Some(refusal) => refusal,
                None => CodecError::malformed(error),
            }),
        }
    }

    /// Reserves an empty retained string after charging its declared length.
    pub fn retained_string(
        &self,
        length: usize,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        self.validate_vector_length::<u8>(0, length, operation)?;
        self.charge_retained(u64_from_index(length), operation)?;
        let mut value = String::new();
        value.try_reserve_exact(length).map_err(|_| {
            self.allocation_failed(ResourceDimension::RetainedBytes, length, operation)
        })?;
        Ok(value)
    }

    /// Copies retained text and appends a retained suffix.
    pub fn retained_suffix(
        &self,
        value: &str,
        suffix: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let mut output = self.copy_retained_text(value, operation)?;
        self.append_retained(&mut output, suffix, operation)?;
        Ok(output)
    }

    /// Copies a slice of retained strings after charging collection slots.
    pub fn copy_retained_strings(
        &self,
        values: &[String],
        operation: &'static str,
    ) -> Result<Vec<String>, CodecError> {
        let mut copies = self.collection_vec(values.len(), operation)?;
        let mut input = values.iter();
        while let Some(value) = self.next_charged(&mut input, operation)? {
            let copy = self.copy_retained_text(value, operation)?;
            self.reserve_capacity(&mut copies, 1, operation)?;
            copies.push(copy);
        }
        Ok(copies)
    }

    /// Joins retained text after measuring and charging the exact byte count.
    pub fn join_retained<S: TextSource>(
        &self,
        parts: &[S],
        separator: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let mut count = 0_usize;
        for part in self.admit_iter(parts, operation)? {
            count = count
                .checked_add(part.as_text().len())
                .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        }
        let gaps = if parts.is_empty() { 0 } else { parts.len() - 1 };
        count = separator
            .len()
            .checked_mul(gaps)
            .and_then(|separators| count.checked_add(separators))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        let mut output = self.retained_string(count, operation)?;
        for (index, part) in self.admit_iter(parts, operation)?.enumerate() {
            if index != 0 {
                self.append_retained(&mut output, separator, operation)?;
            }
            self.append_retained(&mut output, part.as_text(), operation)?;
        }
        Ok(output)
    }

    /// Joins displayed values while charging each retained text fragment.
    pub fn join_display_retained<T: fmt::Display>(
        &self,
        values: impl IntoIterator<Item = T>,
        separator: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        struct ChargedText<'a, 'arena> {
            ctx: &'a DecodeContext<'arena>,
            operation: &'static str,
            text: String,
            refusal: Option<CodecError>,
        }
        impl Write for ChargedText<'_, '_> {
            fn write_str(&mut self, fragment: &str) -> fmt::Result {
                if let Err(error) =
                    self.ctx
                        .append_retained(&mut self.text, fragment, self.operation)
                {
                    self.refusal = Some(error);
                    return Err(fmt::Error);
                }
                Ok(())
            }
        }
        let mut output = ChargedText {
            ctx: self,
            operation,
            text: String::new(),
            refusal: None,
        };
        let mut values = values.into_iter();
        let mut first = true;
        loop {
            let Some(value) = self.next_charged(&mut values, operation)? else {
                break;
            };
            let written = if first {
                output.write_fmt(format_args!("{value}"))
            } else {
                output
                    .write_str(separator)
                    .and_then(|()| output.write_fmt(format_args!("{value}")))
            };
            first = false;
            if written.is_err() {
                return Err(match output.refusal {
                    Some(error) => error,
                    None => self.refuse_codec_limit(operation, 0, 1),
                });
            }
        }
        Ok(output.text)
    }

    fn formatted_length(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<usize, CodecError> {
        struct Count<'ctx, 'arena> {
            ctx: &'ctx DecodeContext<'arena>,
            operation: &'static str,
            length: Option<usize>,
            refusal: Option<CodecError>,
        }
        impl Write for Count<'_, '_> {
            fn write_str(&mut self, value: &str) -> fmt::Result {
                if let Err(error) = self
                    .ctx
                    .charge_work(u64_from_index(value.len()), self.operation)
                {
                    self.refusal = Some(error);
                    return Err(fmt::Error);
                }
                self.length = self.length.and_then(|total| total.checked_add(value.len()));
                self.length.map(|_| ()).ok_or(fmt::Error)
            }
        }
        let mut count = Count {
            ctx: self,
            operation,
            length: Some(0),
            refusal: None,
        };
        if fmt::write(&mut count, args).is_err() {
            return Err(match count.refusal {
                Some(error) => error,
                None => self.refuse_codec_limit(operation, u64::MAX, u64::MAX),
            });
        }
        count
            .length
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod temporary_capacity_tests {
    #[test]
    fn temporary_reserve_charges_no_storage_with_available_capacity() {
        use super::DecodeContext;
        let arena = super::super::DecodeArena::new();
        let mut policy = super::super::DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
        let mut values = Vec::<u64>::with_capacity(8);
        let capacity = values.capacity();
        let storage = ctx
            .reserve_temporary_vec(&mut values, 3, "existing temporary capacity")
            .expect("admitted test operation");
        assert_eq!(values.capacity(), capacity);
        drop(storage);
    }
}
