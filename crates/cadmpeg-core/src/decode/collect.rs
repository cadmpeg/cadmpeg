// SPDX-License-Identifier: Apache-2.0
//! Charged growth of collections owned by a decode session.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};
use std::fmt::{self, Write};
use std::hash::Hash;

use crate::CodecError;

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
    pub fn push(&mut self, value: T) -> Result<(), CodecError> {
        if self.values.len() == self.capacity {
            return Err(CodecError::Malformed(
                "fixed-capacity vector overflow".to_owned(),
            ));
        }
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

enum VecGrowth {
    Exact,
    Amortized,
}

impl DecodeContext<'_> {
    /// Admit additional items and reserve retained backing storage.
    pub fn reserve_retained_vec<T>(&self, values: &mut Vec<T>, count: usize, operation: &'static str) -> Result<(), CodecError> {
        self.reserve_retained_vec_limit(values, count, operation).map_err(Into::into)
    }

    /// Admit items with a resource-only refusal channel.
    pub fn reserve_retained_vec_limit<T>(&self, values: &mut Vec<T>, count: usize, operation: &'static str) -> Result<(), ResourceLimit> {
        self.reserve_retained_vec_storage(values, count, VecGrowth::Amortized, Some(count), operation)
    }

    /// Reserve backing storage without admitting values not yet inserted.
    pub fn reserve_retained_capacity_limit<T>(&self, values: &mut Vec<T>, count: usize, operation: &'static str) -> Result<(), ResourceLimit> {
        self.reserve_retained_vec_storage(values, count, VecGrowth::Amortized, None, operation)
    }

    fn reserve_retained_vec_storage<T>(&self, values: &mut Vec<T>, count: usize, growth: VecGrowth, items: Option<usize>, operation: &'static str) -> Result<(), ResourceLimit> {
        let required = values.len().checked_add(count).ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let capacity = if std::mem::size_of::<T>() == 0 || required <= values.capacity() {
            values.capacity()
        } else if matches!(growth, VecGrowth::Exact) {
            required
        } else if values.capacity() == 0 {
            if required < 4 { 4 } else { required }
        } else {
            match values.capacity().checked_mul(2) {
                Some(grown) if grown > required => grown,
                _ => required,
            }
        };
        let added = capacity - values.capacity();
        let bytes = added.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        self.charge_retained_limit(u64_from_index(bytes), operation)?;
        if let Some(items) = items { self.charge_collection_items_limit(u64_from_index(items), operation)?; }
        if added != 0 {
            values.try_reserve_exact(capacity - values.len())
                .map_err(|_| self.budget.retained_allocation_failed_limit(u64_from_index(bytes), operation))?;
        }
        Ok(())
    }

    /// Appends a value after admitting its slot and retained element storage.
    pub fn push_retained_vec<T>(
        &self,
        values: &mut Vec<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_retained_vec(values, 1, operation)?;
        values.push(value);
        Ok(())
    }

    /// Creates a vector with charged slots and retained element storage.
    pub fn retained_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut values = Vec::new();
        self.reserve_retained_vec_storage(&mut values, count, VecGrowth::Exact, Some(count), operation)?;
        Ok(values)
    }

    /// Retains vector storage whose slots were admitted in aggregate.
    pub fn reserve_retained_admitted_vec<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_retained_vec_storage(values, count, VecGrowth::Amortized, None, operation).map_err(Into::into)
    }

    /// Creates retained storage whose collection slots are admitted separately.
    pub fn retained_admitted_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut values = Vec::new();
        self.reserve_retained_vec_storage(&mut values, count, VecGrowth::Exact, None, operation)?;
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
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.reserve_scoped(u64_from_index(bytes), operation)
    }

    /// Creates scoped storage whose collection slots are admitted separately.
    pub fn scoped_admitted_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(Vec<T>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let values = reservation.with_storage(|| self.retained_admitted_vec(count, operation))?;
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
        self.reserve_scoped_vec_limit(reservation, values, count, operation).map_err(Into::into)
    }

    /// Reserves storage with a resource-only refusal channel.
    pub fn reserve_scoped_vec_limit<T>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        reservation.with_storage_limit(|| self.reserve_retained_vec_limit(values, count, operation))
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
        let count_u64 = u64_from_index(count);
        self.charge_collection_items_limit(count_u64, operation)?;
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| ResourceLimit {
                dimension: ResourceDimension::MaterializedBytes,
                reason: super::ResourceFailure::BudgetExceeded,
                limit: self.policy().limits.max_materialized_bytes,
                used: 0,
                additional: u64::MAX,
                operation,
            })?;
        let reservation = self.reserve_scoped_limit(u64_from_index(bytes), operation)?;
        values.try_reserve_exact(count).map_err(|_| {
            ResourceLimit::allocation_failed(
                ResourceDimension::CollectionItems,
                self.policy().limits.max_collection_items,
                count_u64,
                operation,
            )
        })?;
        Ok(reservation)
    }

    /// Copies a slice into scoped storage and returns its live byte reservation.
    pub fn copy_temporary_slice<T: Clone>(
        &self,
        values: &[T],
        operation: &'static str,
    ) -> Result<(Vec<T>, ScopedReservation<'_>), ResourceLimit> {
        let mut copy = Vec::new();
        let reservation = self.reserve_temporary_vec(&mut copy, values.len(), operation)?;
        copy.extend_from_slice(values);
        Ok((copy, reservation))
    }

    /// Copies text whose byte storage was admitted in aggregate.
    pub fn copy_admitted_text(text: &str, operation: &'static str) -> Result<String, CodecError> {
        let mut copy = String::new();
        copy.try_reserve_exact(text.len()).map_err(|_| {
            CodecError::ResourceLimit(ResourceLimit::allocation_failed(
                ResourceDimension::RetainedBytes,
                u64::MAX,
                u64_from_index(text.len()),
                operation,
            ))
        })?;
        copy.push_str(text);
        Ok(copy)
    }

    /// Collects retained text and charges both vector storage and text bytes.
    pub fn collect_retained_texts<'text>(
        &self,
        values: impl IntoIterator<Item = &'text str>,
        operation: &'static str,
    ) -> Result<Vec<String>, CodecError> {
        self.try_collect_retained_with(values, operation, |value| self.copy_retained_text(value, operation))
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
        let length = self.formatted_length(args, operation)?;
        self.charge_work(u64_from_index(length), operation)?;
        reservation.grow(u64_from_index(length))?;
        let mut text = String::new();
        text.try_reserve_exact(length).map_err(|_| {
            self.allocation_failed(ResourceDimension::MaterializedBytes, length, operation)
        })?;
        fmt::write(&mut text, args).map_err(CodecError::malformed)?;
        Ok(text)
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
        self.reserve_retained_vec(records, count, operation)
    }

    /// Collects retained slots before adding each value.
    pub fn collect_retained_vec<T>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        self.try_collect_retained_with(values, operation, Ok::<T, CodecError>)
    }

    /// Reserves text bytes charged by aggregate admission.
    pub fn reserve_admitted_string(
        text: &mut String,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        text.try_reserve(additional).map_err(|_| {
            CodecError::ResourceLimit(ResourceLimit::allocation_failed(
                ResourceDimension::RetainedBytes,
                u64::MAX,
                u64_from_index(additional),
                operation,
            ))
        })
    }

    /// Inserts a new scoped tree key after charging lookup work and node storage.
    pub fn insert_scoped_btree_set<T: Ord>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut BTreeSet<T>,
        value: T,
        work_operation: &'static str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        self.charge_work(u64_from_index(values.len()), work_operation)?;
        if values.contains(&value) {
            return Ok(false);
        }
        let bytes = std::mem::size_of::<T>()
            .checked_mul(4)
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_collection_items(1, operation)?;
        reservation.grow(u64_from_index(bytes))?;
        Ok(values.insert(value))
    }

    /// Inserts a vacant scoped tree entry after charging lookup work and node storage.
    pub fn insert_scoped_btree_map_if_vacant<K: Ord, V>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut BTreeMap<K, V>,
        key: K,
        value: V,
        work_operation: &'static str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        self.charge_work(u64_from_index(values.len()), work_operation)?;
        match values.entry(key) {
            std::collections::btree_map::Entry::Occupied(_) => Ok(false),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let bytes = std::mem::size_of::<(K, V)>()
                    .checked_mul(4)
                    .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
                self.charge_collection_items(1, operation)?;
                reservation.grow(u64_from_index(bytes))?;
                entry.insert(value);
                Ok(true)
            }
        }
    }

    /// Appends a lazily built value to a scoped group after aggregate admission.
    pub fn push_scoped_btree_group<K: Ord, V>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        groups: &mut BTreeMap<K, Vec<V>>,
        key: K,
        value: impl FnOnce() -> V,
        owned_bytes: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_work(1, operation)?;
        let vacant = !groups.contains_key(&key);
        let key_bytes = if vacant {
            std::mem::size_of::<(K, Vec<V>)>()
        } else {
            0
        };
        let bytes = key_bytes
            .checked_add(std::mem::size_of::<V>())
            .and_then(|bytes| bytes.checked_add(owned_bytes))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_collection_items(1 + u64::from(vacant), operation)?;
        reservation.grow(u64_from_index(bytes))?;
        let values = groups.entry(key).or_default();
        Self::reserve_admitted_vec(values, 1, operation)?;
        values.push(value());
        Ok(())
    }

    /// Admits retained tree-record storage, owned bytes, one slot and one work unit.
    pub fn admit_retained_btree_record<K, V>(
        &self,
        owned_bytes: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let bytes = std::mem::size_of::<(K, V)>()
            .checked_add(owned_bytes)
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_collection_items(1, operation)?;
        self.charge_retained(u64_from_index(bytes), operation)?;
        self.charge_work(1, operation)
    }

    /// Collects scoped groups in input order within each key.
    pub fn collect_scoped_btree_groups<'ctx, K: Ord, V>(
        &'ctx self,
        values: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<(BTreeMap<K, Vec<V>>, ScopedReservation<'ctx>), CodecError> {
        let mut groups = BTreeMap::new();
        let mut reservation = self.reserve_scoped(0, operation)?;
        for (key, value) in values {
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
    pub fn collect_scoped_btree_map<'ctx, K: Ord, V>(
        &'ctx self,
        values: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<(BTreeMap<K, V>, ScopedReservation<'ctx>), CodecError> {
        let mut entries = BTreeMap::new();
        let mut reservation = self.reserve_scoped(0, operation)?;
        for (key, value) in values {
            self.charge_work(1, operation)?;
            if !entries.contains_key(&key) {
                self.charge_collection_items(1, operation)?;
                reservation.grow(u64_from_index(std::mem::size_of::<(K, V)>()))?;
            }
            entries.insert(key, value);
        }
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
        reservation.grow(u64_from_index(additional))?;
        Self::reserve_admitted_string(text, additional, operation)
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
        let limit = match dimension {
            ResourceDimension::CollectionItems => self.policy().limits.max_collection_items,
            ResourceDimension::RetainedBytes => self.policy().limits.max_retained_bytes,
            ResourceDimension::MaterializedBytes => self.policy().limits.max_materialized_bytes,
            _ => u64::MAX,
        };
        CodecError::ResourceLimit(ResourceLimit::allocation_failed(
            dimension,
            limit,
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
        self.reserve_vec_limit(values, additional, operation).map_err(Into::into)
    }

    /// Reserves storage with a resource-only refusal channel.
    pub fn reserve_vec_limit<T>(
        &self,
        values: &mut Vec<T>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.charge_collection_items_limit(u64_from_index(additional), operation)?;
        values
            .try_reserve(additional)
            .map_err(|_| self.collection_allocation_failed_limit(additional, operation))
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
            self.push_vec(&mut out, value?, operation)
                .map_err(E::from)?;
        }
        Ok(out)
    }

    /// Inserts a unique set value after retaining its storage and slot.
    pub fn insert_retained_hash_set<T: Eq + Hash>(
        &self,
        values: &mut HashSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if values.contains(&value) {
            return Ok(false);
        }
        self.charge_retained(u64_from_index(std::mem::size_of::<T>()), operation)?;
        self.reserve_set(values, 1, operation)?;
        Ok(values.insert(value))
    }

    /// Inserts a unique tree value after admitting its scoped value storage.
    pub fn insert_scoped_btree_value<T: Ord>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut BTreeSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if values.contains(&value) {
            return Ok(false);
        }
        self.charge_collection_items(1, operation)?;
        reservation.grow(u64_from_index(std::mem::size_of::<T>()))?;
        Ok(values.insert(value))
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

    /// Extends a set with one charged slot for each distinct new value.
    pub fn extend_hash_set<T: Eq + Hash>(
        &self,
        values: &mut HashSet<T>,
        additions: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        for value in additions {
            self.insert_hash_set(values, value, operation)?;
        }
        Ok(())
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

    /// Appends a grouped value after admitting both new index and value slots.
    pub fn push_hash_group<K: Eq + Hash, V>(
        &self,
        values: &mut HashMap<K, Vec<V>>,
        key: K,
        value: V,
        index_operation: &'static str,
        value_operation: &'static str,
    ) -> Result<(), CodecError> {
        let new_key = !values.contains_key(&key);
        if new_key {
            self.charge_collection_items(1, index_operation)?;
        }
        self.charge_collection_items(1, value_operation)?;
        if new_key {
            Self::reserve_admitted_map(values, 1, index_operation)?;
        }
        let group = values.entry(key).or_default();
        Self::reserve_admitted_vec(group, 1, value_operation)?;
        group.push(value);
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

    /// Appends a deque item after charging its slot.
    pub fn push_back<T>(
        &self,
        values: &mut VecDeque<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_collection_items(1, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| self.collection_allocation_failed(1, operation))?;
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
        self.charge_collection_items(1, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| self.collection_allocation_failed(1, operation))?;
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
        self.charge_collection_items(u64_from_index(count), operation)?;
        values
            .try_reserve(count)
            .map_err(|_| self.collection_allocation_failed(count, operation))
    }

    /// Reserves a vector whose items were charged by aggregate admission.
    pub fn reserve_admitted_vec<T>(
        values: &mut Vec<T>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        values.try_reserve(additional).map_err(|_| {
            CodecError::ResourceLimit(ResourceLimit::allocation_failed(
                ResourceDimension::CollectionItems,
                u64::MAX,
                u64_from_index(additional),
                operation,
            ))
        })
    }

    /// Creates a vector whose items were charged by aggregate admission.
    pub fn admitted_vec<T>(count: usize, operation: &'static str) -> Result<Vec<T>, CodecError> {
        let mut values = Vec::new();
        Self::reserve_admitted_vec(&mut values, count, operation)?;
        Ok(values)
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

    /// Reserves a hash map whose entries were charged by aggregate admission.
    pub fn reserve_admitted_map<K: Eq + Hash, V>(
        values: &mut HashMap<K, V>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        values.try_reserve(additional).map_err(|_| {
            CodecError::ResourceLimit(ResourceLimit::allocation_failed(
                ResourceDimension::CollectionItems,
                u64::MAX,
                u64_from_index(additional),
                operation,
            ))
        })
    }

    /// Reserves a hash set whose entries were charged by aggregate admission.
    pub fn reserve_admitted_set<T: Eq + Hash>(
        values: &mut HashSet<T>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        values.try_reserve(additional).map_err(|_| {
            CodecError::ResourceLimit(ResourceLimit::allocation_failed(
                ResourceDimension::CollectionItems,
                u64::MAX,
                u64_from_index(additional),
                operation,
            ))
        })
    }

    /// Copies items whose slots were charged by aggregate admission.
    pub fn copy_admitted_slice<T: Copy>(
        values: &[T],
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut copy = Vec::new();
        copy.try_reserve_exact(values.len()).map_err(|_| CodecError::ResourceLimit(ResourceLimit::allocation_failed(ResourceDimension::CollectionItems, u64::MAX, u64_from_index(values.len()), operation)))?;
        copy.extend_from_slice(values);
        Ok(copy)
    }

    /// Copies rows whose slots were charged by aggregate admission.
    pub fn copy_admitted_rows<T: Copy>(
        values: &[T],
        row_len: usize,
        operation: &'static str,
    ) -> Result<Vec<Vec<T>>, CodecError> {
        let mut rows = Vec::new();
        let row_count = values.len().div_ceil(row_len);
        rows.try_reserve_exact(row_count).map_err(|_| CodecError::ResourceLimit(ResourceLimit::allocation_failed(ResourceDimension::CollectionItems, u64::MAX, u64_from_index(row_count), operation)))?;
        for row in values.chunks(row_len) {
            rows.push(Self::copy_admitted_slice(row, operation)?);
        }
        Ok(rows)
    }

    /// Collects fallible values and admits the storage of each output slot.
    pub fn try_collect_retained_with<I, T, E: From<CodecError>>(
        &self,
        values: impl IntoIterator<Item = I>,
        operation: &'static str,
        mut map: impl FnMut(I) -> Result<T, E>,
    ) -> Result<Vec<T>, E> {
        let values = values.into_iter();
        let (minimum, maximum) = values.size_hint();
        let mut collected = Vec::new();
        for (index, value) in values.enumerate() {
            self.charge_work(1, operation)?;
            if index == 0 && maximum == Some(minimum) {
                self.reserve_retained_vec_storage(&mut collected, minimum.max(1), VecGrowth::Exact, Some(1), operation).map_err(CodecError::from)?;
            } else {
                self.reserve_retained_vec_storage(&mut collected, 1, VecGrowth::Amortized, Some(1), operation).map_err(CodecError::from)?;
            }
            collected.push(map(value)?);
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
        self.charge_collection_items(u64_from_index(values.len()), operation)?;
        Self::copy_admitted_slice(values, operation)
            .map_err(|_| self.collection_allocation_failed(values.len(), operation))
    }

    /// Collects optional values, stopping at the first absent value.
    pub fn collect_options<T>(
        &self,
        values: impl IntoIterator<Item = Option<T>>,
        operation: &'static str,
    ) -> Result<Option<Vec<T>>, CodecError> {
        let mut collected = Vec::new();
        for value in values {
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
        for value in values {
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
        for value in values {
            self.insert_string_set(&mut collected, value, operation)?;
        }
        Ok(collected)
    }

    /// Copies retained items and charges both their slots and storage.
    pub fn copy_retained_slice<T: Copy>(
        &self,
        values: &[T],
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let bytes = values
            .len()
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| CodecError::ResourceLimit(self.retained_size_overflow_limit(operation)))?;
        self.charge_retained(u64_from_index(bytes), operation)?;
        self.copy_slice(values, operation)
    }

    /// Copies retained rows and charges row and item slots.
    pub fn copy_retained_rows<T: Copy>(
        &self,
        rows: &[Vec<T>],
        row_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<Vec<Vec<T>>, CodecError> {
        self.try_collect_retained_with(rows, row_operation, |row| {
            self.copy_retained_slice(row, item_operation)
        })
    }

    /// Copies a retained set after charging storage and entries.
    pub fn copy_retained_set<T: Copy + Eq + Hash>(
        &self,
        values: &HashSet<T>,
        operation: &'static str,
    ) -> Result<HashSet<T>, CodecError> {
        self.charge_retained(self.temporary_hash_bytes::<T>(values.len(), operation)?, operation)?;
        self.charge_work(u64_from_index(values.len()), operation)?;
        let mut copy = HashSet::new();
        self.reserve_set(&mut copy, values.len(), operation)?;
        copy.extend(values.iter().copied());
        Ok(copy)
    }

    /// Reserves hash set entries after charging their slots.
    pub fn reserve_set<T: Eq + Hash>(
        &self,
        values: &mut HashSet<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_collection_items(u64_from_index(count), operation)?;
        values
            .try_reserve(count)
            .map_err(|_| self.collection_allocation_failed(count, operation))
    }

    /// Reserves hash map entries after charging their slots.
    pub fn reserve_map<K: Eq + Hash, V>(
        &self,
        values: &mut HashMap<K, V>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_collection_items(u64_from_index(count), operation)?;
        values
            .try_reserve(count)
            .map_err(|_| self.collection_allocation_failed(count, operation))
    }

    /// Charges a new B-tree map key before insertion.
    pub fn admit_btree_entry<K: Ord, V>(
        &self,
        values: &BTreeMap<K, V>,
        key: &K,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if !values.contains_key(key) {
            self.charge_collection_items(1, operation)?;
        }
        Ok(())
    }

    /// Inserts a B-tree map entry after charging a new key.
    pub fn insert_btree_map<K: Ord, V>(
        &self,
        values: &mut BTreeMap<K, V>,
        key: K,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError> {
        self.admit_btree_entry(values, &key, operation)?;
        Ok(values.insert(key, value))
    }

    /// Inserts a B-tree set item after charging a new value.
    pub fn insert_btree_set<T: Ord>(
        &self,
        values: &mut BTreeSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if values.contains(&value) {
            return Ok(false);
        }
        self.charge_collection_items(1, operation)?;
        Ok(values.insert(value))
    }

    /// Collects distinct ordered values after admitting each new entry.
    pub fn collect_btree_set<T: Ord>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<BTreeSet<T>, CodecError> {
        let mut out = BTreeSet::new();
        for value in values {
            self.insert_btree_set(&mut out, value, operation)?;
        }
        Ok(out)
    }

    /// Appends an ordered group member after admitting its group and slot.
    pub fn push_btree_group<K: Ord, V>(
        &self,
        groups: &mut BTreeMap<K, Vec<V>>,
        key: K,
        value: V,
        group_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(values) = groups.get_mut(&key) {
            return self.push_vec(values, value, item_operation);
        }
        self.admit_btree_entry(groups, &key, group_operation)?;
        let mut values = self.collection_vec(1, item_operation)?;
        values.push(value);
        groups.insert(key, values);
        Ok(())
    }

    /// Inserts an ordered group member after admitting its group and value.
    pub fn insert_btree_group_set<K: Ord, V: Ord>(
        &self,
        groups: &mut BTreeMap<K, BTreeSet<V>>,
        key: K,
        value: V,
        group_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(values) = groups.get_mut(&key) {
            self.insert_btree_set(values, value, item_operation)?;
            return Ok(());
        }
        self.admit_btree_entry(groups, &key, group_operation)?;
        let mut values = BTreeSet::new();
        self.insert_btree_set(&mut values, value, item_operation)?;
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
            self.reserve_retained_admitted_vec(values, additional, operation)?;
            for _ in 0..additional {
                values.push(fill);
            }
        } else {
            values.truncate(length);
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
        self.charge_retained(u64_from_index(source.len()), operation)?;
        self.reserve_vec(target, source.len(), operation)?;
        target.extend_from_slice(source);
        Ok(())
    }

    /// Reserves a scoped vector and returns its live reservation.
    pub fn temporary_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(Vec<T>, ScopedReservation<'_>), CodecError> {
        let bytes = count
            .checked_mul(std::mem::size_of::<T>().max(1))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        let reservation = self.reserve_scoped(u64_from_index(bytes), operation)?;
        let values = self.collection_vec(count, operation)?;
        Ok((values, reservation))
    }

    fn temporary_hash_bytes<T>(&self, count: usize, operation: &'static str) -> Result<u64, ResourceLimit> {
        std::mem::size_of::<T>().max(1).checked_add(32)
            .and_then(|size| size.checked_mul(count)).map(u64_from_index)
            .ok_or_else(|| self.budget.scoped_size_overflow_limit(operation))
    }

    /// Reserves a scoped hash set and returns its live reservation.
    pub fn temporary_set<T: Eq + Hash>(&self, count: usize, operation: &'static str) -> Result<(HashSet<T>, ScopedReservation<'_>), CodecError> {
        self.temporary_set_limit(count, operation).map_err(Into::into)
    }

    /// Reserves a scoped hash set with a typed resource error.
    pub fn temporary_set_limit<T: Eq + Hash>(&self, count: usize, operation: &'static str) -> Result<(HashSet<T>, ScopedReservation<'_>), ResourceLimit> {
        let bytes = self.temporary_hash_bytes::<T>(count, operation)?;
        let reservation = self.reserve_scoped_limit(bytes, operation)?;
        self.charge_collection_items_limit(u64_from_index(count), operation)?;
        let mut values = HashSet::new();
        values.try_reserve(count).map_err(|_| self.budget.scoped_allocation_failed_limit(bytes, operation))?;
        Ok((values, reservation))
    }

    /// Collects a borrowed string index with scoped hash storage.
    pub fn collect_scoped_string_set<'text>(
        &self,
        count: usize,
        values: impl IntoIterator<Item = &'text str>,
        operation: &'static str,
    ) -> Result<(HashSet<&'text str>, ScopedReservation<'_>), CodecError> {
        let (mut output, reservation) = self.temporary_set(count, operation)?;
        for value in values {
            self.charge_work(u64_from_index(value.len()).checked_mul(2).and_then(|work| work.checked_add(1))
                .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?, operation)?;
            output.insert(value);
        }
        Ok((output, reservation))
    }

    /// Collects a borrowed string-keyed index with scoped hash storage.
    pub fn collect_scoped_string_map<'text, V>(
        &self,
        count: usize,
        values: impl IntoIterator<Item = (&'text str, V)>,
        operation: &'static str,
    ) -> Result<(HashMap<&'text str, V>, ScopedReservation<'_>), CodecError> {
        let reservation = self.reserve_scoped(self.temporary_hash_bytes::<(&str, V)>(count, operation)?, operation)?;
        self.charge_collection_items(u64_from_index(count), operation)?;
        let mut output = HashMap::new();
        Self::reserve_admitted_map(&mut output, count, operation)?;
        for (key, value) in values {
            self.charge_work(u64_from_index(key.len()).checked_mul(2).and_then(|work| work.checked_add(1))
                .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?, operation)?;
            output.insert(key, value);
        }
        Ok((output, reservation))
    }

    /// Reserves a scoped deque and returns its live reservation.
    pub fn temporary_queue<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(VecDeque<T>, ScopedReservation<'_>), CodecError> {
        let reservation =
            self.reserve_scoped(self.temporary_hash_bytes::<T>(count, operation)?, operation)?;
        self.charge_collection_items(u64_from_index(count), operation)?;
        let mut values = VecDeque::new();
        values.try_reserve(count).map_err(|_| {
            self.allocation_failed(ResourceDimension::MaterializedBytes, count, operation)
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
        let length = self.formatted_length(args, operation)?;
        self.charge_work(u64_from_index(length), operation)?;
        self.charge_retained(u64_from_index(length), operation)?;
        let mut text = String::new();
        text.try_reserve_exact(length).map_err(|_| {
            self.allocation_failed(ResourceDimension::RetainedBytes, length, operation)
        })?;
        fmt::write(&mut text, args).map_err(CodecError::malformed)?;
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
        self.charge_retained(u64_from_index(suffix.len()), operation)?;
        output.try_reserve(suffix.len()).map_err(|_| {
            self.allocation_failed(ResourceDimension::RetainedBytes, suffix.len(), operation)
        })?;
        output.push_str(suffix);
        Ok(())
    }

    /// Appends formatted text after admitting its exact retained byte count.
    pub fn append_formatted_retained(
        &self,
        output: &mut String,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let length = self.formatted_length(args, operation)?;
        self.charge_retained(u64_from_index(length), operation)?;
        Self::reserve_admitted_string(output, length, operation)?;
        fmt::write(output, args).map_err(CodecError::malformed)
    }

    /// Reserves an empty retained string after charging its declared length.
    pub fn retained_string(
        &self,
        length: usize,
        operation: &'static str,
    ) -> Result<String, CodecError> {
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
        for value in values {
            copies.push(self.copy_retained_text(value, operation)?);
        }
        Ok(copies)
    }

    /// Joins retained text after measuring and charging the exact byte count.
    pub fn join_retained<S: AsRef<str>>(
        &self,
        parts: &[S],
        separator: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let mut count = 0_usize;
        for part in parts {
            count = count
                .checked_add(part.as_ref().len())
                .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        }
        let gaps = if parts.is_empty() { 0 } else { parts.len() - 1 };
        count = separator
            .len()
            .checked_mul(gaps)
            .and_then(|separators| count.checked_add(separators))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_retained(u64_from_index(count), operation)?;
        let mut output = String::new();
        output.try_reserve_exact(count).map_err(|_| {
            self.allocation_failed(ResourceDimension::RetainedBytes, count, operation)
        })?;
        for (index, part) in parts.iter().enumerate() {
            if index != 0 {
                output.push_str(separator);
            }
            output.push_str(part.as_ref());
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
                if let Err(error) = self
                    .ctx
                    .charge_retained(u64_from_index(fragment.len()), self.operation)
                {
                    self.refusal = Some(error);
                    return Err(fmt::Error);
                }
                if self.text.try_reserve(fragment.len()).is_err() {
                    self.refusal = Some(self.ctx.allocation_failed(
                        ResourceDimension::RetainedBytes,
                        fragment.len(),
                        self.operation,
                    ));
                    return Err(fmt::Error);
                }
                self.text.push_str(fragment);
                Ok(())
            }
        }
        let mut output = ChargedText {
            ctx: self,
            operation,
            text: String::new(),
            refusal: None,
        };
        for (index, value) in values.into_iter().enumerate() {
            let written = if index == 0 {
                output.write_fmt(format_args!("{value}"))
            } else {
                output
                    .write_str(separator)
                    .and_then(|()| output.write_fmt(format_args!("{value}")))
            };
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
                if let Err(error) = self.ctx.charge_work(u64_from_index(value.len()), self.operation) {
                    self.refusal = Some(error);
                    return Err(fmt::Error);
                }
                self.length = self.length.and_then(|total| total.checked_add(value.len()));
                self.length.map(|_| ()).ok_or(fmt::Error)
            }
        }
        let mut count = Count { ctx: self, operation, length: Some(0), refusal: None };
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
mod tests {
    #[test]
    fn retained_capacity_reservation_and_push_charge_storage_and_item_once() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 4 * u64::try_from(std::mem::size_of::<u64>()).unwrap();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut values = Vec::new();
        ctx.reserve_retained_capacity_limit(&mut values, 1, "reserve retained capacity").unwrap();
        assert_eq!(values.capacity(), 4);
        ctx.push_retained_vec(&mut values, 7u64, "admit retained item").unwrap();
        assert_eq!(values, [7]);
        let error = ctx.push_retained_vec(&mut values, 8u64, "refuse second item").unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.used == 1 && limit.additional == 1));
        assert_eq!(values, [7]);
    }

    #[test]
    fn flat_copies_do_not_charge_storage_for_empty_or_zero_sized_values() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(ctx.copy_retained_slice(&[(); 3], "zero sized copy").unwrap(), vec![(); 3]);
        assert!(ctx.copy_retained_set(&HashSet::<u64>::new(), "empty set copy").unwrap().is_empty());
        assert!(ctx.finish_session().is_ok());
    }

    #[test]
    fn retained_vector_size_overflow_fuses_in_the_storage_dimension() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut values = Vec::<u64>::new();
        let error = ctx.reserve_retained_vec(&mut values, usize::MAX, "oversized retained vector").unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes));
        assert!(values.is_empty());
        assert!(ctx.finish_session().is_err());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = ctx.with_scoped_storage("oversized scoped vector", || {
            ctx.reserve_retained_vec(&mut values, usize::MAX, "oversized scoped vector")
        }).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes));
        assert!(ctx.finish_session().is_err());
    }

    use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};

    use super::super::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use super::ExactVec;
    use crate::CodecError;

    fn context(arena: &DecodeArena, items: u64) -> DecodeContext<'_> {
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

    collection_case!(
        collection_vec_charges_before_allocation,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .collection_vec::<u8>(2, "test collection vec")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .collection_vec::<u8>(2, "test collection vec")
            .map(|_| ())
    );

    #[test]
    fn exact_vec_charges_before_allocation_and_requires_full_count() {
        let bytes = [0_u8, 0];
        let count = super::super::View::over_retained(&bytes)
            .counted(2, 1)
            .expect("two bytes prove two items");
        let arena = DecodeArena::new();
        let ctx = context(&arena, 1);
        assert!(matches!(
            ExactVec::<u8>::new(&ctx, count, "test exact vec"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
        ));
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut values = ExactVec::new(&ctx, count, "test exact vec").expect("service profile");
        values.push(1_u8).expect("first item fits");
        values.push(2_u8).expect("second item fits");
        assert!(values.push(3_u8).is_err());
        assert_eq!(values.finish().expect("exact count"), [1, 2]);
        let mut short = ExactVec::new(&ctx, count, "test exact vec").expect("service profile");
        short.push(1_u8).expect("first item fits");
        assert!(short.finish().is_err());
    }
    collection_case!(
        reserve_vec_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.reserve_vec(&mut Vec::<u8>::new(), 1, "test reserve vec"),
        |ctx: &DecodeContext<'_>| ctx.reserve_vec(&mut Vec::<u8>::new(), 1, "test reserve vec")
    );
    collection_case!(
        push_vec_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.push_vec(&mut Vec::new(), 7_u8, "test push vec"),
        |ctx: &DecodeContext<'_>| ctx.push_vec(&mut Vec::new(), 7_u8, "test push vec")
    );
    collection_case!(
        push_formatted_retained_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.push_formatted_retained(
            &mut Vec::new(),
            format_args!("a"),
            "test note slots",
            "test note text"
        ),
        |ctx: &DecodeContext<'_>| ctx.push_formatted_retained(
            &mut Vec::new(),
            format_args!("a"),
            "test note slots",
            "test note text"
        )
    );
    collection_case!(
        append_vec_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| ctx.append_vec(
            &mut Vec::new(),
            &mut vec![1_u8, 2],
            "test append vec"
        ),
        |ctx: &DecodeContext<'_>| ctx.append_vec(
            &mut Vec::new(),
            &mut vec![1_u8, 2],
            "test append vec"
        )
    );
    collection_case!(
        extend_vec_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| ctx.extend_vec(&mut Vec::new(), vec![1_u8, 2], "test extend vec"),
        |ctx: &DecodeContext<'_>| ctx.extend_vec(&mut Vec::new(), vec![1_u8, 2], "test extend vec")
    );
    collection_case!(
        collect_vec_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| ctx.collect_vec([1_u8, 2], "test collect vec").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.collect_vec([1_u8, 2], "test collect vec").map(|_| ())
    );
    collection_case!(
        try_collect_vec_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .try_collect_vec([Ok::<u8, CodecError>(1), Ok(2)], "test try collect vec")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .try_collect_vec([Ok::<u8, CodecError>(1), Ok(2)], "test try collect vec")
            .map(|_| ())
    );
    collection_case!(
        insert_hash_set_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx
            .insert_hash_set(&mut HashSet::new(), 1_u8, "test insert set")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .insert_hash_set(&mut HashSet::new(), 1_u8, "test insert set")
            .map(|_| ())
    );
    collection_case!(
        insert_string_set_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx
            .insert_string_set(&mut HashSet::new(), "a", "test insert string")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .insert_string_set(&mut HashSet::new(), "a", "test insert string")
            .map(|_| ())
    );
    collection_case!(
        collect_hash_set_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .collect_hash_set([1_u8, 2], "test collect set")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .collect_hash_set([1_u8, 2], "test collect set")
            .map(|_| ())
    );
    collection_case!(
        admit_hash_map_entry_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.admit_hash_map_entry(
            &mut HashMap::<u8, u8>::new(),
            &1,
            "test admit map"
        ),
        |ctx: &DecodeContext<'_>| ctx.admit_hash_map_entry(
            &mut HashMap::<u8, u8>::new(),
            &1,
            "test admit map"
        )
    );
    collection_case!(
        insert_hash_map_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx
            .insert_hash_map(&mut HashMap::new(), 1_u8, 2_u8, "test insert map")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .insert_hash_map(&mut HashMap::new(), 1_u8, 2_u8, "test insert map")
            .map(|_| ())
    );
    collection_case!(
        collect_hash_map_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .collect_hash_map([(1_u8, 2_u8), (3, 4)], "test collect map")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .collect_hash_map([(1_u8, 2_u8), (3, 4)], "test collect map")
            .map(|_| ())
    );
    collection_case!(
        push_back_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.push_back(&mut VecDeque::new(), 1_u8, "test push back"),
        |ctx: &DecodeContext<'_>| ctx.push_back(&mut VecDeque::new(), 1_u8, "test push back")
    );
    #[test]
    fn push_front_refuses_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, 0);
        let mut values = VecDeque::new();
        assert!(
            matches!(ctx.push_front(&mut values, 1_u8, "test push front"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems)
        );
        assert_eq!(values.capacity(), 0);
        assert!(values.is_empty());
    }

    #[test]
    fn push_front_service_profile_preserves_order() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut values = VecDeque::from([2_u8]);
        ctx.push_front(&mut values, 1, "test push front")
            .expect("service profile");
        assert_eq!(values, VecDeque::from([1, 2]));
    }

    collection_case!(
        reserve_heap_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.reserve_heap(
            &mut BinaryHeap::<u8>::new(),
            1,
            "test reserve heap"
        ),
        |ctx: &DecodeContext<'_>| ctx.reserve_heap(
            &mut BinaryHeap::<u8>::new(),
            1,
            "test reserve heap"
        )
    );
    collection_case!(
        copy_slice_charges_before_allocation,
        2,
        |ctx: &DecodeContext<'_>| ctx.copy_slice(&[1_u8, 2], "test copy slice").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.copy_slice(&[1_u8, 2], "test copy slice").map(|_| ())
    );
    collection_case!(
        collect_options_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .collect_options([Some(1_u8), Some(2)], "test collect options")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .collect_options([Some(1_u8), Some(2)], "test collect options")
            .map(|_| ())
    );
    collection_case!(
        collect_fallible_options_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .collect_fallible_options(
                [Ok::<Option<u8>, CodecError>(Some(1)), Ok(Some(2))],
                "test fallible options"
            )
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .collect_fallible_options(
                [Ok::<Option<u8>, CodecError>(Some(1)), Ok(Some(2))],
                "test fallible options"
            )
            .map(|_| ())
    );
    collection_case!(
        collect_string_set_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx
            .collect_string_set(["one"], "test string set")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .collect_string_set(["one"], "test string set")
            .map(|_| ())
    );
    collection_case!(
        reserve_set_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.reserve_set(&mut HashSet::<u8>::new(), 1, "test reserve set"),
        |ctx: &DecodeContext<'_>| ctx.reserve_set(&mut HashSet::<u8>::new(), 1, "test reserve set")
    );
    collection_case!(
        reserve_map_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.reserve_map(
            &mut HashMap::<u8, u8>::new(),
            1,
            "test reserve map"
        ),
        |ctx: &DecodeContext<'_>| ctx.reserve_map(
            &mut HashMap::<u8, u8>::new(),
            1,
            "test reserve map"
        )
    );
    collection_case!(
        admit_btree_entry_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.admit_btree_entry(
            &BTreeMap::<u8, u8>::new(),
            &1,
            "test admit btree"
        ),
        |ctx: &DecodeContext<'_>| ctx.admit_btree_entry(
            &BTreeMap::<u8, u8>::new(),
            &1,
            "test admit btree"
        )
    );
    collection_case!(
        insert_btree_map_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx
            .insert_btree_map(&mut BTreeMap::new(), 1_u8, 2_u8, "test insert btree map")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .insert_btree_map(&mut BTreeMap::new(), 1_u8, 2_u8, "test insert btree map")
            .map(|_| ())
    );
    collection_case!(
        insert_btree_set_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx
            .insert_btree_set(&mut BTreeSet::new(), 1_u8, "test insert btree set")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .insert_btree_set(&mut BTreeSet::new(), 1_u8, "test insert btree set")
            .map(|_| ())
    );
    collection_case!(
        extend_retained_bytes_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.extend_retained_bytes(
            &mut Vec::new(),
            b"a",
            "test extend bytes"
        ),
        |ctx: &DecodeContext<'_>| ctx.extend_retained_bytes(
            &mut Vec::new(),
            b"a",
            "test extend bytes"
        )
    );
    collection_case!(
        temporary_vec_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| ctx.temporary_vec::<u8>(1, "test temporary vec").map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx.temporary_vec::<u8>(1, "test temporary vec").map(|_| ())
    );
    collection_case!(
        copy_retained_strings_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .copy_retained_strings(
                &[String::from("a"), String::from("b")],
                "test retained strings"
            )
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .copy_retained_strings(
                &[String::from("a"), String::from("b")],
                "test retained strings"
            )
            .map(|_| ())
    );
    collection_case!(
        optional_collection_vec_charges_before_allocation,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .optional_collection_vec::<u8>(true, 2, "test optional collection")
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .optional_collection_vec::<u8>(true, 2, "test optional collection")
            .map(|_| ())
    );
    collection_case!(
        collect_indexed_vec_charges_before_allocation,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .collect_indexed_vec(2, "test indexed vec", |index| u8::try_from(index)
                .map_err(CodecError::malformed))
            .map(|_| ()),
        |ctx: &DecodeContext<'_>| ctx
            .collect_indexed_vec(2, "test indexed vec", |index| u8::try_from(index)
                .map_err(CodecError::malformed))
            .map(|_| ())
    );

    macro_rules! admitted_case {
        ($name:ident, $body:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let ctx = context(&arena, 1);
                let result: Result<(), CodecError> = (|| {
                    ctx.charge_collection_items(2, "test admitted")?;
                    ($body)(&ctx)
                })();
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems));
                let arena = DecodeArena::new();
                let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
                ctx.charge_collection_items(2, "test admitted").expect("admission");
                assert!(($body)(&ctx).is_ok());
            }
        };
    }
    admitted_case!(
        reserve_admitted_vec_follows_prior_admission,
        |_ctx: &DecodeContext<'_>| DecodeContext::reserve_admitted_vec(
            &mut Vec::<u8>::new(),
            2,
            "test admitted"
        )
    );
    admitted_case!(
        reserve_admitted_map_follows_prior_admission,
        |_ctx: &DecodeContext<'_>| DecodeContext::reserve_admitted_map(
            &mut HashMap::<u8, u8>::new(),
            2,
            "test admitted"
        )
    );
    admitted_case!(
        reserve_admitted_set_follows_prior_admission,
        |_ctx: &DecodeContext<'_>| DecodeContext::reserve_admitted_set(
            &mut HashSet::<u8>::new(),
            2,
            "test admitted"
        )
    );
    admitted_case!(
        copy_admitted_slice_follows_prior_admission,
        |_ctx: &DecodeContext<'_>| DecodeContext::copy_admitted_slice(&[1_u8, 2], "test admitted")
            .map(|_| ())
    );
    admitted_case!(
        copy_admitted_rows_follows_prior_admission,
        |_ctx: &DecodeContext<'_>| DecodeContext::copy_admitted_rows(
            &[1_u8, 2],
            1,
            "test admitted"
        )
        .map(|_| ())
    );
    admitted_case!(
        admitted_vec_follows_prior_admission,
        |_ctx: &DecodeContext<'_>| DecodeContext::admitted_vec::<u8>(2, "test admitted")
            .map(|_| ())
    );

    macro_rules! retained_case {
        ($name:ident, $need:expr, $body:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = $need - 1;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("test context");
                let result: Result<(), CodecError> = ($body)(&ctx);
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes));
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
                    .expect("test context");
                assert!(($body)(&ctx).is_ok());
            }
        };
    }
    retained_case!(
        copy_retained_slice_charges_before_allocation,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .copy_retained_slice(&[1_u8, 2], "test retained slice")
            .map(|_| ())
    );
    retained_case!(
        copy_retained_rows_charges_before_allocation,
        crate::decode::u64_from_index(std::mem::size_of::<Vec<u8>>()),
        |ctx: &DecodeContext<'_>| ctx
            .copy_retained_rows(
                &[vec![1_u8]],
                "test retained rows",
                "test retained row items"
            )
            .map(|_| ())
    );
    retained_case!(
        copy_retained_set_charges_before_allocation,
        crate::decode::u64_from_index(std::mem::size_of::<HashSet<u8>>() + 1),
        |ctx: &DecodeContext<'_>| ctx
            .copy_retained_set(&HashSet::from([1_u8]), "test retained set")
            .map(|_| ())
    );
    retained_case!(
        format_retained_charges_before_allocation,
        3,
        |ctx: &DecodeContext<'_>| ctx
            .format_retained(format_args!("abc"), "test format retained")
            .map(|_| ())
    );
    retained_case!(
        append_retained_charges_before_growth,
        3,
        |ctx: &DecodeContext<'_>| ctx.append_retained(
            &mut String::new(),
            "abc",
            "test append retained"
        )
    );
    retained_case!(
        retained_suffix_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| ctx.retained_suffix("a", "b", "test suffix").map(|_| ())
    );
    retained_case!(
        join_retained_charges_before_allocation,
        3,
        |ctx: &DecodeContext<'_>| ctx
            .join_retained(&["a", "b"], "-", "test join retained")
            .map(|_| ())
    );
    retained_case!(
        join_display_retained_charges_before_growth,
        7,
        |ctx: &DecodeContext<'_>| ctx
            .join_display_retained(["one", "two"], ",", "test display join")
            .map(|_| ())
    );
    retained_case!(
        retained_string_charges_before_allocation,
        3,
        |ctx: &DecodeContext<'_>| ctx.retained_string(3, "test retained string").map(|_| ())
    );
    retained_case!(
        copy_retained_charges_before_allocation,
        3,
        |ctx: &DecodeContext<'_>| ctx
            .copy_retained(b"abc", "test optional retained")
            .map(|_| ())
    );

    macro_rules! materialized_case {
        ($name:ident, $need:expr, $body:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = $need - 1;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("test context");
                let result: Result<(), CodecError> = ($body)(&ctx);
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::MaterializedBytes));
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
                    .expect("test context");
                assert!(($body)(&ctx).is_ok());
            }
        };
    }
    materialized_case!(
        temporary_set_reserves_scoped_storage,
        33,
        |ctx: &DecodeContext<'_>| ctx.temporary_set::<u8>(1, "test temporary set").map(|_| ())
    );
    materialized_case!(
        temporary_queue_reserves_scoped_storage,
        33,
        |ctx: &DecodeContext<'_>| ctx
            .temporary_queue::<u8>(1, "test temporary queue")
            .map(|_| ())
    );
    materialized_case!(
        format_scoped_charges_before_allocation,
        3,
        |ctx: &DecodeContext<'_>| ctx
            .format_scoped(format_args!("abc"), "test format scoped")
            .map(|_| ())
    );

    #[test]
    fn copy_scoped_text_refuses_before_allocation_and_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped text")
            .expect("empty reserve");
        let result = ctx.copy_scoped_text("abc", &mut reservation, "test scoped text");
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped text")
            .expect("empty reserve");
        assert_eq!(
            ctx.copy_scoped_text("abc", &mut reservation, "test scoped text")
                .expect("copy"),
            "abc"
        );
    }
    #[test]
    fn charged_join_refuses_input_sized_text_before_growth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 5;
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
        assert!(matches!(
            ctx.join_display_retained(["one", "two"], ",", "step_test_join"),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_test_join"
        ));
    }

    #[test]
    fn charged_format_refuses_retained_text_before_growth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 3;
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
        let error = ctx
            .format_retained(
                format_args!("prefix {suffix}", suffix = "input"),
                "step_test_format",
            )
            .expect_err("formatted text exceeds three bytes");
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "step_test_format"
        ));
    }
    fn operation_context(
        arena: &DecodeArena,
        dimension: ResourceDimension,
        limit: u64,
    ) -> DecodeContext<'_> {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = limit,
            ResourceDimension::Entities => policy.limits.max_entities = limit,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
            _ => panic!("test needs a byte, entity or work dimension"),
        }
        DecodeContext::from_root_bytes(&[], arena, &policy)
            .expect("empty root fits policy")
            .0
    }

    macro_rules! operation_case {
        ($name:ident, $success:ident, $dimension:expr, $needed:expr, $operation:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let ctx = operation_context(&arena, $dimension, $needed - 1);
                let result: Result<(), CodecError> = ($operation)(&ctx);
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == $dimension));
            }
            #[test]
            fn $success() {
                let arena = DecodeArena::new();
                let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
                let result: Result<(), CodecError> = ($operation)(&ctx);
                assert!(result.is_ok(), "service profile admits operation");
            }
        };
    }

    operation_case!(
        reserve_retained_vec_refuses_before_growth,
        reserve_retained_vec_succeeds_under_service_profile,
        ResourceDimension::RetainedBytes,
        2,
        |ctx: &DecodeContext<'_>| {
            let mut values = Vec::<u8>::new();
            let result = ctx.reserve_retained_vec(&mut values, 2, "test retained growth");
            if result.is_err() {
                assert_eq!(values.capacity(), 0);
            }
            result
        }
    );
    operation_case!(
        retained_vec_refuses_before_allocation,
        retained_vec_succeeds_under_service_profile,
        ResourceDimension::RetainedBytes,
        2,
        |ctx: &DecodeContext<'_>| ctx.retained_vec::<u8>(2, "test retained vec").map(|_| ())
    );
    operation_case!(
        reserve_retained_admitted_vec_refuses_before_growth,
        reserve_retained_admitted_vec_succeeds_under_service_profile,
        ResourceDimension::RetainedBytes,
        2,
        |ctx: &DecodeContext<'_>| {
            let mut values = Vec::<u8>::new();
            let result =
                ctx.reserve_retained_admitted_vec(&mut values, 2, "test retained admitted vec");
            if result.is_err() {
                assert_eq!(values.capacity(), 0);
            }
            result
        }
    );
    operation_case!(
        reserve_scoped_vec_refuses_before_growth,
        reserve_scoped_vec_succeeds_under_service_profile,
        ResourceDimension::MaterializedBytes,
        2,
        |ctx: &DecodeContext<'_>| {
            let mut reservation = ctx.reserve_scoped(0, "test scoped vec")?;
            let mut values = Vec::<u8>::new();
            let result =
                ctx.reserve_scoped_vec(&mut reservation, &mut values, 2, "test scoped vec");
            if result.is_err() {
                assert_eq!(values.capacity(), 0);
            }
            result
        }
    );
    operation_case!(
        reserve_temporary_vec_refuses_before_growth,
        reserve_temporary_vec_succeeds_under_service_profile,
        ResourceDimension::MaterializedBytes,
        2,
        |ctx: &DecodeContext<'_>| {
            let mut values = Vec::<u8>::new();
            let result = ctx
                .reserve_temporary_vec(&mut values, 2, "test temporary vec")
                .map(|_| ())
                .map_err(CodecError::from);
            if result.is_err() {
                assert_eq!(values.capacity(), 0);
            }
            result
        }
    );
    operation_case!(
        collect_retained_texts_refuses_at_byte_limit,
        collect_retained_texts_succeeds_under_service_profile,
        ResourceDimension::RetainedBytes,
        super::u64_from_index(std::mem::size_of::<String>() + 2),
        |ctx: &DecodeContext<'_>| ctx
            .collect_retained_texts(["ab"], "test retained texts")
            .map(|_| ())
    );
    operation_case!(
        collect_scoped_texts_refuses_at_byte_limit,
        collect_scoped_texts_succeeds_under_service_profile,
        ResourceDimension::MaterializedBytes,
        super::u64_from_index(std::mem::size_of::<String>() + 2),
        |ctx: &DecodeContext<'_>| ctx
            .collect_scoped_texts(["ab"], "test scoped texts")
            .map(|_| ())
    );
    operation_case!(
        format_retained_with_work_refuses_before_formatting,
        format_retained_with_work_succeeds_under_service_profile,
        ResourceDimension::WorkUnits,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .format_retained(format_args!("ab"), "test formatted work")
            .map(|_| ())
    );
    operation_case!(
        format_scoped_text_with_work_refuses_before_formatting,
        format_scoped_text_with_work_succeeds_under_service_profile,
        ResourceDimension::WorkUnits,
        2,
        |ctx: &DecodeContext<'_>| {
            let mut reservation = ctx.reserve_scoped(0, "test scoped formatted work")?;
            ctx.format_scoped_text(
                &mut reservation,
                format_args!("ab"),
                "test scoped formatted work",
            )
            .map(|_| ())
        }
    );
    operation_case!(
        reserve_record_vec_refuses_before_growth,
        reserve_record_vec_succeeds_under_service_profile,
        ResourceDimension::Entities,
        2,
        |ctx: &DecodeContext<'_>| {
            let mut values = Vec::<u8>::new();
            let result = ctx.reserve_record_vec(&mut values, 2, 0, "test record vec");
            if result.is_err() {
                assert_eq!(values.capacity(), 0);
            }
            result
        }
    );

    #[test]
    fn copy_admitted_text_preserves_utf8() {
        assert_eq!(
            DecodeContext::copy_admitted_text("aé", "test admitted text").expect("admitted text"),
            "aé"
        );
    }
    #[test]
    fn copy_temporary_slice_refuses_before_allocation_and_clone() {
        #[derive(Debug)]
        struct ObservedClone<'a>(&'a std::cell::Cell<usize>);
        impl Clone for ObservedClone<'_> {
            fn clone(&self) -> Self {
                self.0.set(self.0.get() + 1);
                Self(self.0)
            }
        }
        let cloned = std::cell::Cell::new(0);
        let arena = DecodeArena::new();
        let need = super::u64_from_index(std::mem::size_of::<ObservedClone<'_>>());
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, need - 1);
        let error = ctx
            .copy_temporary_slice(&[ObservedClone(&cloned)], "test scoped copy")
            .expect_err("copy exceeds scoped storage");
        assert_eq!(error.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(cloned.get(), 0);
    }

    #[test]
    fn copy_temporary_slice_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty service root is admitted");
        let (copy, reservation) = ctx
            .copy_temporary_slice(&[1u8, 2], "test scoped copy")
            .expect("copy fits service profile");
        assert_eq!(copy, [1, 2]);
        drop(reservation);
    }

    operation_case!(
        collect_retained_vec_refuses_before_first_allocation,
        collect_retained_vec_succeeds_under_service_profile,
        ResourceDimension::RetainedBytes,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .collect_retained_vec([1u16], "test retained collection")
            .map(|_| ())
    );
    operation_case!(
        copy_slice_with_work_refuses_before_copy,
        copy_slice_with_work_succeeds_under_service_profile,
        ResourceDimension::WorkUnits,
        2,
        |ctx: &DecodeContext<'_>| ctx
            .copy_slice(&[1u8, 2], "test work copy")
            .map(|_| ())
    );

    #[test]
    fn admitted_string_reserve_preserves_prefix_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        ctx.charge_retained(2, "test admitted text slots")
            .expect("service admits text");
        let mut text = String::from("a");
        DecodeContext::reserve_admitted_string(&mut text, 2, "test admitted text slots")
            .expect("text allocation");
        assert_eq!(text, "a");
        assert!(text.capacity() >= 3);
    }
    #[test]
    fn admitted_string_reserve_follows_one_below_limit_refusal_before_growth() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 1);
        let mut text = String::new();
        let result = ctx
            .charge_retained(2, "test admitted text slots")
            .and_then(|()| {
                DecodeContext::reserve_admitted_string(&mut text, 2, "test admitted text slots")
            });
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
        );
        assert_eq!(text.capacity(), 0);
    }

    #[test]
    fn push_retained_vec_refuses_one_below_storage_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 1);
        let mut values = Vec::<u16>::new();
        let error = ctx
            .push_retained_vec(&mut values, 7, "test retained push")
            .expect_err("test operation refuses");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
        );
        assert_eq!(values.capacity(), 0);
        assert!(values.is_empty());
    }

    #[test]
    fn push_retained_vec_keeps_value_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut values = Vec::<u16>::new();
        ctx.push_retained_vec(&mut values, 7, "test retained push")
            .expect("test operation succeeds");
        assert_eq!(values, [7]);
    }

    #[test]
    fn scoped_tree_set_refuses_one_below_storage_before_insertion() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 3);
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped tree")
            .expect("test operation succeeds");
        let mut values = BTreeSet::new();
        let error = ctx
            .insert_scoped_btree_set(
                &mut reservation,
                &mut values,
                7u8,
                "test scoped lookup",
                "test scoped tree",
            )
            .expect_err("test operation refuses");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
        assert!(values.is_empty());
    }

    #[test]
    fn scoped_tree_set_preserves_unique_values_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped tree")
            .expect("test operation succeeds");
        let mut values = BTreeSet::new();
        assert!(ctx
            .insert_scoped_btree_set(
                &mut reservation,
                &mut values,
                7u8,
                "test scoped lookup",
                "test scoped tree"
            )
            .expect("test operation succeeds"));
        assert!(!ctx
            .insert_scoped_btree_set(
                &mut reservation,
                &mut values,
                7u8,
                "test scoped lookup",
                "test scoped tree"
            )
            .expect("test operation succeeds"));
        assert_eq!(values, BTreeSet::from([7]));
    }

    #[test]
    fn scoped_tree_map_refuses_one_below_storage_before_insertion() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 7);
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped tree")
            .expect("test operation succeeds");
        let mut values = BTreeMap::new();
        let error = ctx
            .insert_scoped_btree_map_if_vacant(
                &mut reservation,
                &mut values,
                7u8,
                9u8,
                "test scoped lookup",
                "test scoped tree",
            )
            .expect_err("test operation refuses");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
        assert!(values.is_empty());
    }

    #[test]
    fn scoped_tree_map_preserves_first_entry_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped tree")
            .expect("test operation succeeds");
        let mut values = BTreeMap::new();
        assert!(ctx
            .insert_scoped_btree_map_if_vacant(
                &mut reservation,
                &mut values,
                7u8,
                9u8,
                "test scoped lookup",
                "test scoped tree"
            )
            .expect("test operation succeeds"));
        assert!(!ctx
            .insert_scoped_btree_map_if_vacant(
                &mut reservation,
                &mut values,
                7u8,
                11u8,
                "test scoped lookup",
                "test scoped tree"
            )
            .expect("test operation succeeds"));
        assert_eq!(values, BTreeMap::from([(7, 9)]));
    }

    #[test]
    fn scoped_group_refuses_one_below_storage_before_allocation() {
        let arena = DecodeArena::new();
        let need = super::u64_from_index(
            std::mem::size_of::<(u8, Vec<u16>)>() + std::mem::size_of::<u16>() + 3,
        );
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, need - 1);
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped group")
            .expect("test operation succeeds");
        let mut groups = BTreeMap::new();
        let built = std::cell::Cell::new(false);
        let error = ctx
            .push_scoped_btree_group(
                &mut reservation,
                &mut groups,
                1u8,
                || {
                    built.set(true);
                    7u16
                },
                3,
                "test scoped group",
            )
            .expect_err("test operation refuses");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
        assert!(groups.is_empty());
        assert!(!built.get());
    }

    #[test]
    fn scoped_group_preserves_member_order_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped group")
            .expect("test operation succeeds");
        let mut groups = BTreeMap::new();
        ctx.push_scoped_btree_group(
            &mut reservation,
            &mut groups,
            1u8,
            || 7u16,
            3,
            "test scoped group",
        )
        .expect("test operation succeeds");
        ctx.push_scoped_btree_group(
            &mut reservation,
            &mut groups,
            1u8,
            || 9u16,
            3,
            "test scoped group",
        )
        .expect("test operation succeeds");
        assert_eq!(groups[&1], [7, 9]);
    }

    #[test]
    fn scoped_string_refuses_one_below_storage_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 1);
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped string")
            .expect("test operation succeeds");
        let mut text = String::new();
        let error = ctx
            .reserve_scoped_string(&mut reservation, &mut text, 2, "test scoped string")
            .expect_err("test operation refuses");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
        assert_eq!(text.capacity(), 0);
    }

    #[test]
    fn scoped_string_keeps_prefix_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped string")
            .expect("test operation succeeds");
        let mut text = String::from("a");
        ctx.reserve_scoped_string(&mut reservation, &mut text, 2, "test scoped string")
            .expect("test operation succeeds");
        text.push_str("bc");
        assert_eq!(text, "abc");
    }

    fn manual_group_with_limit(
        configure: impl FnOnce(&mut crate::decode::DecodePolicy),
    ) -> Result<(), crate::CodecError> {
        let arena = crate::decode::DecodeArena::new();
        let mut policy = crate::decode::DecodePolicy::service();
        configure(&mut policy);
        let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test operation succeeds");
        let mut reservation = ctx.reserve_scoped(0, "NX feature operation group indexes")?;
        let mut grouped = std::collections::BTreeMap::new();
        ctx.push_scoped_btree_group(
            &mut reservation,
            &mut grouped,
            "operation",
            || 1u32,
            0,
            "NX feature operation group index",
        )?;
        ctx.push_scoped_btree_group(
            &mut reservation,
            &mut grouped,
            "operation",
            || 2u32,
            0,
            "NX feature operation group index",
        )?;
        assert_eq!(grouped["operation"], [1, 2]);
        Ok(())
    }

    #[test]
    fn manual_operation_group_refuses_collection_limit() {
        let error = manual_group_with_limit(|policy| policy.limits.max_collection_items = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn manual_operation_group_refuses_scoped_limit() {
        let error = manual_group_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn manual_operation_group_refuses_work_limit() {
        let error = manual_group_with_limit(|policy| policy.limits.max_work_units = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::WorkUnits));
    }

    #[test]
    fn jt_rendered_node_path_refuses_retained_limit() {
        let arena = crate::decode::DecodeArena::new();
        let mut policy = crate::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 4;
        let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let error = ctx
            .join_display_retained([12, 34].iter(), "-", "nx JT rendered node path")
            .expect_err("five text bytes exceed the four-byte retained limit");
        assert!(matches!(
            error,
            crate::CodecError::ResourceLimit(limit)
                if limit.dimension == crate::decode::ResourceDimension::RetainedBytes
                    && limit.operation == "nx JT rendered node path"
        ));
        {
            let arena = crate::decode::DecodeArena::new();
            let (service, _) = crate::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &crate::decode::DecodePolicy::service(),
            )
            .expect("test decode context");
            assert_eq!(
                service
                    .join_display_retained([12, 34].iter(), "-", "nx JT rendered node path")
                    .expect("test operation succeeds"),
                "12-34"
            );
        }
    }

    #[test]
    fn jt_tessellation_channel_bytes_refuse_collection_limit() {
        let arena = crate::decode::DecodeArena::new();
        let mut policy = crate::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let mut bytes = Vec::<u8>::new();
        let error = ctx
            .reserve_retained_vec(&mut bytes, 3, "nx JT tessellation colors")
            .expect_err("three color bytes exceed two collection items");
        assert!(matches!(
            error,
            crate::CodecError::ResourceLimit(limit)
                if limit.dimension == crate::decode::ResourceDimension::CollectionItems
                    && limit.operation == "nx JT tessellation colors"
        ));
        assert!(bytes.is_empty());
    }

    #[test]
    fn retained_vector_slots_do_not_charge_entities() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
        let mut values = Vec::<u8>::new();
        ctx.reserve_retained_vec(&mut values, 1, "test retained slots")
            .expect("vector slot is not an entity");
        assert!(values.is_empty());
    }

    #[test]
    fn collect_scoped_btree_groups_refuses_before_allocating_first_group() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = crate::decode::u64_from_index(
            std::mem::size_of::<(u8, Vec<u8>)>() + std::mem::size_of::<u8>(),
        ) - 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        let mut yielded = 0;
        let values = [(1u8, 2u8), (1, 3)].into_iter().inspect(|_| yielded += 1);
        assert!(
            matches!(ctx.collect_scoped_btree_groups(values, "test scoped groups"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
        assert_eq!(yielded, 1);
    }

    #[test]
    fn collect_scoped_btree_groups_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test operation succeeds");
        let (groups, _reservation) = ctx
            .collect_scoped_btree_groups([(1u8, 2u8), (1, 3)], "test scoped groups")
            .expect("test operation succeeds");
        assert_eq!(groups[&1], [2, 3]);
    }

    #[test]
    fn collect_scoped_btree_map_refuses_before_allocating_first_entry() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes =
            crate::decode::u64_from_index(std::mem::size_of::<(u8, u8)>()) - 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        let mut yielded = 0;
        let values = [(1u8, 2u8), (1, 3)].into_iter().inspect(|_| yielded += 1);
        assert!(
            matches!(ctx.collect_scoped_btree_map(values, "test scoped map"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
        assert_eq!(yielded, 1);
    }

    #[test]
    fn collect_scoped_btree_map_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test operation succeeds");
        let (entries, _reservation) = ctx
            .collect_scoped_btree_map([(1u8, 2u8), (1, 3)], "test scoped map")
            .expect("test operation succeeds");
        assert_eq!(entries[&1], 3);
    }

    fn last_record_with_limit(
        configure: impl FnOnce(&mut crate::decode::DecodePolicy),
    ) -> Result<(), crate::CodecError> {
        let records = [("operation", 1u32), ("operation", 2u32)];
        let arena = crate::decode::DecodeArena::new();
        let mut policy = crate::decode::DecodePolicy::service();
        configure(&mut policy);
        let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test operation succeeds");
        let (index, _reservation) =
            ctx.collect_scoped_btree_map(records, "NX last-record index")?;
        assert_eq!(index["operation"], 2);
        Ok(())
    }

    #[test]
    fn last_record_index_refuses_collection_limit() {
        let error = last_record_with_limit(|policy| policy.limits.max_collection_items = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn last_record_index_refuses_scoped_limit() {
        let error = last_record_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn last_record_index_refuses_work_limit() {
        let error = last_record_with_limit(|policy| policy.limits.max_work_units = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::WorkUnits));
    }

    fn grouped_records_with_limit(
        configure: impl FnOnce(&mut crate::decode::DecodePolicy),
    ) -> Result<(), crate::CodecError> {
        let records = [("first".to_owned(), 1), ("first".to_owned(), 2)];
        let arena = crate::decode::DecodeArena::new();
        let mut policy = crate::decode::DecodePolicy::service();
        configure(&mut policy);
        let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test operation succeeds");
        let (grouped, _reservation) = ctx.collect_scoped_btree_groups(
            records.iter().map(|record| (record.0.as_str(), record)),
            "NX operation record index",
        )?;
        assert_eq!(
            grouped["first"]
                .iter()
                .map(|record| record.1)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        Ok(())
    }

    #[test]
    fn operation_record_index_refuses_collection_limit() {
        let error = grouped_records_with_limit(|policy| policy.limits.max_collection_items = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn operation_record_index_refuses_scoped_limit() {
        let error = grouped_records_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn operation_record_index_refuses_work_limit() {
        let error = grouped_records_with_limit(|policy| policy.limits.max_work_units = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::WorkUnits));
    }

    #[test]
    fn admit_retained_btree_record_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            crate::decode::u64_from_index(std::mem::size_of::<(String, u16)>() + 3) - 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        assert!(
            matches!(ctx.admit_retained_btree_record::<String, u16>(3, "test retained tree record"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn admit_retained_btree_record_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test operation succeeds");
        ctx.admit_retained_btree_record::<String, u16>(3, "test retained tree record")
            .expect("test operation succeeds");
    }

    fn attribute_lookup_with_limit(
        configure: impl FnOnce(&mut crate::decode::DecodePolicy),
    ) -> Result<(), crate::CodecError> {
        let records = [("first", 1_u8), ("second", 2_u8)];
        let arena = crate::decode::DecodeArena::new();
        let mut policy = crate::decode::DecodePolicy::service();
        configure(&mut policy);
        let (ctx, _) = crate::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;

        let (indexed, _index_reservation) = ctx.collect_scoped_btree_map(
            records.iter().map(|record| (record.0, record)),
            "NX Parasolid attribute record index",
        )?;
        let (grouped, _group_reservation) = ctx.collect_scoped_btree_groups(
            records.iter().map(|record| (record.0, record)),
            "NX Parasolid attribute use groups",
        )?;
        assert_eq!(indexed.len(), 2);
        assert_eq!(grouped.len(), 2);
        Ok(())
    }

    #[test]
    fn attribute_lookup_refuses_collection_limit() {
        let error = attribute_lookup_with_limit(|policy| policy.limits.max_collection_items = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn attribute_lookup_refuses_scoped_limit() {
        let error = attribute_lookup_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn attribute_lookup_refuses_work_limit() {
        let error = attribute_lookup_with_limit(|policy| policy.limits.max_work_units = 0)
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
        if limit.dimension == crate::decode::ResourceDimension::WorkUnits));
    }
    #[test]
    fn append_formatted_retained_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
        let mut output = String::new();
        let error = ctx
            .append_formatted_retained(
                &mut output,
                format_args!("{}", 123),
                "test formatted append",
            )
            .expect_err("three bytes exceed two");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes));
        assert!(output.is_empty());
        assert_eq!(output.capacity(), 0);
    }

    #[test]
    fn append_formatted_retained_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut output = String::from("prefix:");
        ctx.append_formatted_retained(
            &mut output,
            format_args!("{}", 123),
            "test formatted append",
        )
        .expect("service profile admits text");
        assert_eq!(output, "prefix:123");
    }

    #[test]
    fn push_scoped_vec_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 1);
        let mut values = Vec::<u16>::new();
        let mut storage = ctx
            .reserve_scoped(0, "test scoped push")
            .expect("test reservation");
        let error = ctx
            .push_scoped_vec(&mut storage, &mut values, 7, "test scoped push")
            .expect_err("one below required storage");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
        assert!(values.is_empty());
        assert_eq!(values.capacity(), 0);
    }

    #[test]
    fn push_scoped_vec_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut values = Vec::<u16>::new();
        let mut storage = ctx
            .reserve_scoped(0, "test scoped push")
            .expect("test reservation");
        ctx.push_scoped_vec(&mut storage, &mut values, 7, "test scoped push")
            .expect("service admission");
        assert_eq!(values, [7]);
    }

    #[test]
    fn collect_btree_set_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, 0);

        let error = ctx
            .collect_btree_set([7u16], "test ordered set")
            .expect_err("one below required storage");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn collect_btree_set_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");

        let result = ctx
            .collect_btree_set([7u16, 9, 7], "test ordered set")
            .expect("service admission");
        assert_eq!(result, BTreeSet::from([7, 9]));
    }

    #[test]
    fn push_btree_group_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, 1);
        let mut values = BTreeMap::<u8, Vec<u16>>::new();
        let error = ctx
            .push_btree_group(&mut values, 1, 7, "test group", "test member")
            .expect_err("one below required storage");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
        );
        assert!(values.is_empty());
    }

    #[test]
    fn push_btree_group_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut values = BTreeMap::<u8, Vec<u16>>::new();
        ctx.push_btree_group(&mut values, 1, 7, "test group", "test member")
            .expect("service admission");
        assert_eq!(values[&1], [7]);
        ctx.push_btree_group(&mut values, 1, 9, "test group", "test member")
            .expect("second member");
        assert_eq!(values[&1], [7, 9]);
    }

    #[test]
    fn insert_btree_group_set_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, 1);
        let mut values = BTreeMap::<u8, BTreeSet<u16>>::new();
        let error = ctx
            .insert_btree_group_set(&mut values, 1, 7, "test group", "test member")
            .expect_err("one below required storage");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
        );
        assert!(values.is_empty());
    }

    #[test]
    fn insert_btree_group_set_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut values = BTreeMap::<u8, BTreeSet<u16>>::new();
        ctx.insert_btree_group_set(&mut values, 1, 7, "test group", "test member")
            .expect("service admission");
        assert_eq!(values[&1], BTreeSet::from([7]));
        ctx.insert_btree_group_set(&mut values, 1, 7, "test group", "test member")
            .expect("duplicate member");
        assert_eq!(values[&1], BTreeSet::from([7]));
    }

    #[test]
    fn retained_admitted_vec_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 3);
        assert!(
            matches!(ctx.retained_admitted_vec::<u16>(2, "test retained admitted storage"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn retained_admitted_vec_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut values = ctx
            .retained_admitted_vec(2, "test retained admitted storage")
            .expect("service admission");
        values.extend([7u16, 9]);
        assert_eq!(values, [7, 9]);
    }

    #[test]
    fn scoped_admitted_vec_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 3);
        assert!(
            matches!(ctx.scoped_admitted_vec::<u16>(2, "test scoped admitted storage"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
    }

    #[test]
    fn scoped_admitted_vec_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let (mut values, _reservation) = ctx
            .scoped_admitted_vec(2, "test scoped admitted storage")
            .expect("service admission");
        values.extend([7u16, 9]);
        assert_eq!(values, [7, 9]);
    }

    #[test]
    fn charge_formatted_retained_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 2);
        assert!(
            matches!(ctx.charge_formatted_retained(format_args!("a{}", 12), "test formatted admission"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes && limit.additional == 3)
        );
    }

    #[test]
    fn charge_formatted_retained_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        ctx.charge_formatted_retained(format_args!("a{}", 12), "test formatted admission")
            .expect("service admission");
    }

    #[test]
    fn insert_retained_hash_set_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 1);
        let mut values = HashSet::new();
        assert!(
            matches!(ctx.insert_retained_hash_set(&mut values, 7u16, "test retained set"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
        );
        assert!(values.is_empty());
        assert_eq!(values.capacity(), 0);
    }

    #[test]
    fn insert_retained_hash_set_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut values = HashSet::new();
        assert!(ctx
            .insert_retained_hash_set(&mut values, 7u16, "test retained set")
            .expect("service admission"));
        assert!(!ctx
            .insert_retained_hash_set(&mut values, 7u16, "test retained set")
            .expect("duplicate"));
        assert_eq!(values, HashSet::from([7]));
    }

    #[test]
    fn insert_scoped_btree_value_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 1);
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped value")
            .expect("empty reserve");
        let mut values = BTreeSet::new();
        assert!(
            matches!(ctx.insert_scoped_btree_value(&mut reservation, &mut values, 7u16, "test scoped value"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
        assert!(values.is_empty());
    }

    #[test]
    fn insert_scoped_btree_value_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped value")
            .expect("empty reserve");
        let mut values = BTreeSet::new();
        assert!(ctx
            .insert_scoped_btree_value(&mut reservation, &mut values, 7u16, "test scoped value")
            .expect("service admission"));
        assert!(!ctx
            .insert_scoped_btree_value(&mut reservation, &mut values, 7u16, "test scoped value")
            .expect("duplicate"));
        assert_eq!(values, BTreeSet::from([7]));
    }

    #[test]
    fn extend_hash_set_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, 0);
        let mut values = HashSet::new();
        assert!(
            matches!(ctx.extend_hash_set(&mut values, [7u16], "test extend set"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems)
        );
        assert!(values.is_empty());
        assert_eq!(values.capacity(), 0);
    }

    #[test]
    fn extend_hash_set_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut values = HashSet::from([7u16]);
        ctx.extend_hash_set(&mut values, [7, 9, 9], "test extend set")
            .expect("service admission");
        assert_eq!(values, HashSet::from([7, 9]));
    }

    #[test]
    fn reserve_scoped_collection_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 3);
        assert!(
            matches!(ctx.reserve_scoped_collection::<u16>(2, "test scoped collection"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes && limit.additional == 4)
        );
    }

    #[test]
    fn reserve_scoped_collection_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let reservation = ctx
            .reserve_scoped_collection::<u16>(2, "test scoped collection")
            .expect("service admission");
        drop(reservation);
    }
    #[test]
    fn push_hash_group_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, 1);
        let mut groups = HashMap::<u8, Vec<u8>>::new();
        let error = ctx
            .push_hash_group(&mut groups, 1, 7, "group index", "group value")
            .expect_err("two slots exceed one");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "group value"));
        assert!(groups.is_empty());
        assert_eq!(groups.capacity(), 0);
    }

    #[test]
    fn push_hash_group_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut groups = HashMap::<u8, Vec<u8>>::new();
        ctx.push_hash_group(&mut groups, 1, 7, "group index", "group value")
            .expect("first value");
        ctx.push_hash_group(&mut groups, 1, 8, "group index", "group value")
            .expect("same group");
        assert_eq!(groups.get(&1), Some(&vec![7, 8]));
    }
    #[test]
    fn resize_retained_bytes_refuses_one_below_need_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 1);
        let mut values = Vec::new();
        let error = ctx
            .resize_retained_bytes(&mut values, 2, 7, "test retained resize")
            .expect_err("one below required storage");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.additional == 4));
        assert!(values.is_empty());
        assert_eq!(values.capacity(), 0);
    }

    #[test]
    fn resize_retained_bytes_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test context");
        let mut values = Vec::new();
        ctx.resize_retained_bytes(&mut values, 2, 7, "test retained resize")
            .expect("service admission");
        assert_eq!(values, [7, 7]);
        ctx.resize_retained_bytes(&mut values, 4, 9, "test retained resize")
            .expect("growth");
        assert_eq!(values, [7, 7, 9, 9]);
        ctx.resize_retained_bytes(&mut values, 1, 0, "test retained resize")
            .expect("truncate");
        assert_eq!(values, [7]);
    }
}
