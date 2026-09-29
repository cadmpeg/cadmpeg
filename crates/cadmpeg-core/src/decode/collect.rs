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

impl DecodeContext<'_> {
    /// Reserves vector slots and retains their element storage.
    pub fn reserve_retained_vec<T>(&self, values: &mut Vec<T>, count: usize, operation: &'static str) -> Result<(), CodecError> {
        let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_retained(u64_from_index(bytes), operation)?;
        self.reserve_vec(values, count, operation)
    }

    /// Appends a value after admitting its slot and retained element storage.
    pub fn push_retained_vec<T>(&self, values: &mut Vec<T>, value: T, operation: &'static str) -> Result<(), CodecError> {
        self.reserve_retained_vec(values, 1, operation)?;
        values.push(value);
        Ok(())
    }

    /// Creates a vector with charged slots and retained element storage.
    pub fn retained_vec<T>(&self, count: usize, operation: &'static str) -> Result<Vec<T>, CodecError> {
        let mut values = Vec::new();
        self.reserve_retained_vec(&mut values, count, operation)?;
        Ok(values)
    }

    /// Retains vector storage whose slots were admitted in aggregate.
    pub fn reserve_retained_admitted_vec<T>(&self, values: &mut Vec<T>, count: usize, operation: &'static str) -> Result<(), CodecError> {
        let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_retained(u64_from_index(bytes), operation)?;
        Self::reserve_admitted_vec(values, count, operation)
    }

    /// Grows scoped storage and admits additional vector slots.
    pub fn reserve_scoped_vec<T>(&self, reservation: &mut ScopedReservation<'_>, values: &mut Vec<T>, count: usize, operation: &'static str) -> Result<(), CodecError> {
        let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        reservation.grow(u64_from_index(bytes))?;
        self.reserve_vec(values, count, operation)
    }

    /// Reserves temporary vector storage with a typed optional-session refusal.
    pub fn reserve_temporary_vec_optional_limit<'ctx, T>(ctx: Option<&'ctx Self>, values: &mut Vec<T>, count: usize, operation: &'static str) -> Result<Option<ScopedReservation<'ctx>>, ResourceLimit> {
        let count_u64 = u64_from_index(count);
        let reservation = if let Some(ctx) = ctx {
            ctx.charge_collection_items_limit(count_u64, operation)?;
            let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| ResourceLimit {
                dimension: ResourceDimension::MaterializedBytes,
                reason: super::ResourceFailure::BudgetExceeded,
                limit: ctx.policy().limits.max_materialized_bytes,
                used: 0,
                additional: u64::MAX,
                operation,
            })?;
            let reservation = ctx.reserve_scoped_limit(u64_from_index(bytes), operation)?;
            Some(reservation)
        } else { None };
        values.try_reserve_exact(count).map_err(|_| ResourceLimit::allocation_failed(ResourceDimension::CollectionItems, ctx.map_or(u64::MAX, |ctx| ctx.policy().limits.max_collection_items), count_u64, operation))?;
        Ok(reservation)
    }

    /// Reserves temporary vector storage and returns its live byte reservation.
    pub fn reserve_temporary_vec<T>(&self, values: &mut Vec<T>, count: usize, operation: &'static str) -> Result<ScopedReservation<'_>, CodecError> {
        let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        let reservation = self.reserve_scoped(u64_from_index(bytes), operation)?;
        self.reserve_vec(values, count, operation)?;
        Ok(reservation)
    }

    /// Copies text whose byte storage was admitted in aggregate.
    pub fn copy_admitted_text(text: &str, operation: &'static str) -> Result<String, CodecError> {
        let mut copy = String::new();
        copy.try_reserve_exact(text.len()).map_err(|_| CodecError::ResourceLimit(ResourceLimit::allocation_failed(ResourceDimension::RetainedBytes, u64::MAX, u64_from_index(text.len()), operation)))?;
        copy.push_str(text);
        Ok(copy)
    }

    /// Collects retained text and charges both vector storage and text bytes.
    pub fn collect_retained_texts<'text>(&self, values: impl IntoIterator<Item = &'text str>, operation: &'static str) -> Result<Vec<String>, CodecError> {
        let mut copies = Vec::new();
        for value in values {
            self.reserve_retained_vec(&mut copies, 1, operation)?;
            copies.push(self.copy_retained_text(value, operation)?);
        }
        Ok(copies)
    }

    /// Copies scoped text values and returns the reservation for their storage.
    pub fn collect_scoped_texts<'text>(&self, values: impl IntoIterator<Item = &'text str>, operation: &'static str) -> Result<(Vec<String>, ScopedReservation<'_>), CodecError> {
        let mut copies = Vec::new();
        let mut reservation = self.reserve_scoped(0, operation)?;
        for value in values {
            self.reserve_scoped_vec(&mut reservation, &mut copies, 1, operation)?;
            copies.push(self.copy_scoped_text(value, &mut reservation, operation)?);
        }
        Ok((copies, reservation))
    }

    /// Formats retained text after charging its byte count as work.
    pub fn format_retained_with_work(&self, args: fmt::Arguments<'_>, operation: &'static str) -> Result<String, CodecError> {
        let length = self.formatted_length(args, operation)?;
        self.charge_work(u64_from_index(length), operation)?;
        self.format_retained(args, operation)
    }

    /// Formats text in an existing scope after charging its byte count as work.
    pub fn format_scoped_text_with_work(&self, reservation: &mut ScopedReservation<'_>, args: fmt::Arguments<'_>, operation: &'static str) -> Result<String, CodecError> {
        let length = self.formatted_length(args, operation)?;
        self.charge_work(u64_from_index(length), operation)?;
        reservation.grow(u64_from_index(length))?;
        let mut text = String::new();
        text.try_reserve_exact(length).map_err(|_| self.allocation_failed(ResourceDimension::MaterializedBytes, length, operation))?;
        fmt::write(&mut text, args).map_err(CodecError::malformed)?;
        Ok(text)
    }

    /// Admits records, retains their storage and additional text, and reserves slots.
    pub fn reserve_record_vec<T>(&self, records: &mut Vec<T>, count: usize, text_bytes: u64, operation: &'static str) -> Result<(), CodecError> {
        self.charge_entities(u64_from_index(count), operation)?;
        self.charge_retained(text_bytes, operation)?;
        self.reserve_retained_vec(records, count, operation)
    }

    /// Copies a temporary slice under an optional session and keeps its reservation live.
    pub fn copy_temporary_slice_optional_limit<'ctx, T: Clone>(ctx: Option<&'ctx Self>, values: &[T], operation: &'static str) -> Result<(Vec<T>, Option<ScopedReservation<'ctx>>), ResourceLimit> {
        let mut copy = Vec::new();
        let reservation = Self::reserve_temporary_vec_optional_limit(ctx, &mut copy, values.len(), operation)?;
        copy.extend_from_slice(values);
        Ok((copy, reservation))
    }

    /// Collects retained slots before adding each value.
    pub fn collect_retained_vec<T>(&self, values: impl IntoIterator<Item = T>, operation: &'static str) -> Result<Vec<T>, CodecError> {
        let mut collected = Vec::new();
        for value in values {
            self.reserve_retained_vec(&mut collected, 1, operation)?;
            collected.push(value);
        }
        Ok(collected)
    }

    /// Copies a slice after admitting its work and collection slots.
    pub fn copy_slice_with_work<T: Clone>(&self, values: &[T], operation: &'static str) -> Result<Vec<T>, CodecError> {
        self.charge_work(u64_from_index(values.len()), operation)?;
        self.copy_slice(values, operation)
    }

    /// Reserves text bytes charged by aggregate admission.
    pub fn reserve_admitted_string(text: &mut String, additional: usize, operation: &'static str) -> Result<(), CodecError> {
        text.try_reserve(additional).map_err(|_| CodecError::ResourceLimit(ResourceLimit::allocation_failed(ResourceDimension::RetainedBytes, u64::MAX, u64_from_index(additional), operation)))
    }

    /// Inserts a new scoped tree key after charging lookup work and node storage.
    pub fn insert_scoped_btree_set<T: Ord>(&self, reservation: &mut ScopedReservation<'_>, values: &mut BTreeSet<T>, value: T, work_operation: &'static str, operation: &'static str) -> Result<bool, CodecError> {
        self.charge_work(u64_from_index(values.len()), work_operation)?;
        if values.contains(&value) { return Ok(false); }
        let bytes = std::mem::size_of::<T>().checked_mul(4).ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_collection_items(1, operation)?;
        reservation.grow(u64_from_index(bytes))?;
        Ok(values.insert(value))
    }

    /// Inserts a vacant scoped tree entry after charging lookup work and node storage.
    pub fn insert_scoped_btree_map_if_vacant<K: Ord, V>(&self, reservation: &mut ScopedReservation<'_>, values: &mut BTreeMap<K, V>, key: K, value: V, work_operation: &'static str, operation: &'static str) -> Result<bool, CodecError> {
        self.charge_work(u64_from_index(values.len()), work_operation)?;
        match values.entry(key) {
            std::collections::btree_map::Entry::Occupied(_) => Ok(false),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let bytes = std::mem::size_of::<(K, V)>().checked_mul(4).ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
                self.charge_collection_items(1, operation)?;
                reservation.grow(u64_from_index(bytes))?;
                entry.insert(value);
                Ok(true)
            }
        }
    }

    fn collection_allocation_failed(&self, count: usize, operation: &'static str) -> CodecError {
        CodecError::ResourceLimit(ResourceLimit::allocation_failed(
            ResourceDimension::CollectionItems,
            self.policy().limits.max_collection_items,
            u64_from_index(count),
            operation,
        ))
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
        self.charge_collection_items(u64_from_index(additional), operation)?;
        values
            .try_reserve(additional)
            .map_err(|_| self.collection_allocation_failed(additional, operation))
    }

    /// Creates a charged vector when a session is present and an admitted vector otherwise.
    pub fn collection_vec_optional<T>(
        ctx: Option<&Self>,
        count: usize,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        match ctx {
            Some(ctx) => ctx.collection_vec(count, operation),
            None => Self::admitted_vec(count, operation),
        }
    }

    /// Reserves vector slots and charges them when a session is present.
    pub fn reserve_vec_optional<T>(
        ctx: Option<&Self>,
        values: &mut Vec<T>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        match ctx {
            Some(ctx) => ctx.reserve_vec(values, additional, operation),
            None => Self::reserve_admitted_vec(values, additional, operation),
        }
    }

    /// Copies bytes retained by an optional decode session.
    pub fn copy_retained_optional(
        ctx: Option<&Self>,
        bytes: &[u8],
        operation: &'static str,
    ) -> Result<Vec<u8>, CodecError> {
        match ctx {
            Some(ctx) => ctx.copy_retained(bytes, operation),
            None => {
                let mut copy = Self::admitted_vec(bytes.len(), operation)?;
                copy.extend_from_slice(bytes);
                Ok(copy)
            }
        }
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

    /// Reserves a vector for an optional session after aggregate admission.
    pub fn optional_admitted_vec<T>(
        ctx: Option<&Self>,
        count: usize,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        if ctx.is_some() {
            Self::admitted_vec(count, operation)
        } else {
            Ok(Vec::new())
        }
    }

    /// Reserves additional slots for an optional session after admission.
    pub fn reserve_optional_admitted_vec<T>(
        ctx: Option<&Self>,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if ctx.is_some() {
            Self::reserve_admitted_vec(values, count, operation)?;
        }
        Ok(())
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
    pub fn copy_admitted_slice<T: Clone>(
        values: &[T],
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut copy = Vec::new();
        Self::reserve_admitted_vec(&mut copy, values.len(), operation)?;
        copy.extend_from_slice(values);
        Ok(copy)
    }

    /// Copies rows whose slots were charged by aggregate admission.
    pub fn copy_admitted_rows<T: Clone>(
        values: &[T],
        row_len: usize,
        operation: &'static str,
    ) -> Result<Vec<Vec<T>>, CodecError> {
        let mut rows = Vec::new();
        Self::reserve_admitted_vec(&mut rows, values.len().div_ceil(row_len), operation)?;
        for row in values.chunks(row_len) {
            rows.push(Self::copy_admitted_slice(row, operation)?);
        }
        Ok(rows)
    }

    /// Copies a slice after charging its collection slots.
    pub fn copy_slice<T: Clone>(
        &self,
        values: &[T],
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
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
    pub fn copy_retained_slice<T: Clone>(
        &self,
        values: &[T],
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let bytes = values
            .len()
            .checked_mul(std::mem::size_of::<T>().max(1))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_retained(u64_from_index(bytes), operation)?;
        self.copy_slice(values, operation)
    }

    /// Copies retained rows and charges row and item slots.
    pub fn copy_retained_rows<T: Clone>(
        &self,
        rows: &[Vec<T>],
        row_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<Vec<Vec<T>>, CodecError> {
        let bytes = rows
            .len()
            .checked_mul(std::mem::size_of::<Vec<T>>())
            .ok_or_else(|| self.refuse_codec_limit(row_operation, u64::MAX, u64::MAX))?;
        self.charge_retained(u64_from_index(bytes), row_operation)?;
        let mut copy = self.collection_vec(rows.len(), row_operation)?;
        for row in rows {
            copy.push(self.copy_retained_slice(row, item_operation)?);
        }
        Ok(copy)
    }

    /// Copies a retained set after charging storage and entries.
    pub fn copy_retained_set<T: Copy + Eq + Hash>(
        &self,
        values: &HashSet<T>,
        operation: &'static str,
    ) -> Result<HashSet<T>, CodecError> {
        let bytes = values
            .len()
            .checked_mul(std::mem::size_of::<T>().max(1))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<HashSet<T>>()))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_retained(u64_from_index(bytes), operation)?;
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

    /// Inserts a B-tree set item, charging it when a session is present.
    pub fn insert_btree_set_optional<T: Ord>(
        ctx: Option<&Self>,
        values: &mut BTreeSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        match ctx {
            Some(ctx) => ctx.insert_btree_set(values, value, operation),
            None => Ok(values.insert(value)),
        }
    }

    /// Inserts a B-tree map entry, charging it when a session is present.
    pub fn insert_btree_map_optional<K: Ord, V>(
        ctx: Option<&Self>,
        values: &mut BTreeMap<K, V>,
        key: K,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError> {
        match ctx {
            Some(ctx) => ctx.insert_btree_map(values, key, value, operation),
            None => Ok(values.insert(key, value)),
        }
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

    fn temporary_hash_bytes<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        std::mem::size_of::<T>()
            .max(1)
            .checked_add(32)
            .and_then(|size| size.checked_mul(count))
            .map(u64_from_index)
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))
    }

    /// Reserves a scoped hash set and returns its live reservation.
    pub fn temporary_set<T: Eq + Hash>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(HashSet<T>, ScopedReservation<'_>), CodecError> {
        let reservation =
            self.reserve_scoped(self.temporary_hash_bytes::<T>(count, operation)?, operation)?;
        self.charge_collection_items(u64_from_index(count), operation)?;
        let mut values = HashSet::new();
        values.try_reserve(count).map_err(|_| {
            self.allocation_failed(ResourceDimension::MaterializedBytes, count, operation)
        })?;
        Ok((values, reservation))
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

    /// Formats a retained string after charging its exact byte count.
    pub fn format_retained(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let length = self.formatted_length(args, operation)?;
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
        let length = self.formatted_length(args, operation)?;
        let reservation = self.reserve_scoped(u64_from_index(length), operation)?;
        let mut text = String::new();
        text.try_reserve_exact(length).map_err(|_| {
            self.allocation_failed(ResourceDimension::MaterializedBytes, length, operation)
        })?;
        fmt::write(&mut text, args).map_err(CodecError::malformed)?;
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
        struct Count(Option<usize>);
        impl Write for Count {
            fn write_str(&mut self, value: &str) -> fmt::Result {
                self.0 = self.0.and_then(|total| total.checked_add(value.len()));
                self.0.map(|_| ()).ok_or(fmt::Error)
            }
        }
        let mut count = Count(Some(0));
        fmt::write(&mut count, args)
            .map_err(|_| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        count
            .0
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))
    }
}

#[cfg(test)]
mod tests {
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
        collection_vec_optional_charges_before_allocation,
        2,
        |ctx: &DecodeContext<'_>| DecodeContext::collection_vec_optional::<u8>(
            Some(ctx),
            2,
            "test optional vec"
        )
        .map(|_| ()),
        |ctx: &DecodeContext<'_>| DecodeContext::collection_vec_optional::<u8>(
            Some(ctx),
            2,
            "test optional vec"
        )
        .map(|_| ())
    );
    collection_case!(
        reserve_vec_optional_charges_before_growth,
        2,
        |ctx: &DecodeContext<'_>| DecodeContext::reserve_vec_optional(
            Some(ctx),
            &mut Vec::<u8>::new(),
            2,
            "test optional reserve"
        ),
        |ctx: &DecodeContext<'_>| DecodeContext::reserve_vec_optional(
            Some(ctx),
            &mut Vec::<u8>::new(),
            2,
            "test optional reserve"
        )
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
    collection_case!(
        insert_btree_set_optional_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| DecodeContext::insert_btree_set_optional(
            Some(ctx),
            &mut BTreeSet::new(),
            1_u8,
            "test optional btree set"
        )
        .map(|_| ()),
        |ctx: &DecodeContext<'_>| DecodeContext::insert_btree_set_optional(
            Some(ctx),
            &mut BTreeSet::new(),
            1_u8,
            "test optional btree set"
        )
        .map(|_| ())
    );
    collection_case!(
        insert_btree_map_optional_charges_before_growth,
        1,
        |ctx: &DecodeContext<'_>| DecodeContext::insert_btree_map_optional(
            Some(ctx),
            &mut BTreeMap::new(),
            1_u8,
            2_u8,
            "test optional btree map"
        )
        .map(|_| ()),
        |ctx: &DecodeContext<'_>| DecodeContext::insert_btree_map_optional(
            Some(ctx),
            &mut BTreeMap::new(),
            1_u8,
            2_u8,
            "test optional btree map"
        )
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
    admitted_case!(
        optional_admitted_vec_follows_prior_admission,
        |ctx: &DecodeContext<'_>| DecodeContext::optional_admitted_vec::<u8>(
            Some(ctx),
            2,
            "test admitted"
        )
        .map(|_| ())
    );
    admitted_case!(
        reserve_optional_admitted_vec_follows_prior_admission,
        |ctx: &DecodeContext<'_>| DecodeContext::reserve_optional_admitted_vec(
            Some(ctx),
            &mut Vec::<u8>::new(),
            2,
            "test admitted"
        )
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
        copy_retained_optional_charges_before_allocation,
        3,
        |ctx: &DecodeContext<'_>| DecodeContext::copy_retained_optional(
            Some(ctx),
            b"abc",
            "test optional retained"
        )
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
    fn operation_context(arena: &DecodeArena, dimension: ResourceDimension, limit: u64) -> DecodeContext<'_> {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = limit,
            ResourceDimension::Entities => policy.limits.max_entities = limit,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
            _ => panic!("test needs a byte, entity or work dimension"),
        }
        DecodeContext::from_root_bytes(&[], arena, &policy).expect("empty root fits policy").0
    }

    macro_rules! operation_case {
        ($name:ident, $dimension:expr, $needed:expr, $operation:expr) => {
            #[test]
            fn $name() {
                let arena = DecodeArena::new();
                let ctx = operation_context(&arena, $dimension, $needed - 1);
                let result: Result<(), CodecError> = ($operation)(&ctx);
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == $dimension));
                let arena = DecodeArena::new();
                let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
                let result: Result<(), CodecError> = ($operation)(&ctx);
                assert!(result.is_ok(), "service profile admits operation");
            }
        };
    }

    operation_case!(reserve_retained_vec_refuses_before_growth, ResourceDimension::RetainedBytes, 2,
        |ctx: &DecodeContext<'_>| { let mut values = Vec::<u8>::new(); let result = ctx.reserve_retained_vec(&mut values, 2, "test retained growth"); if result.is_err() { assert_eq!(values.capacity(), 0); } result });
    operation_case!(retained_vec_refuses_before_allocation, ResourceDimension::RetainedBytes, 2,
        |ctx: &DecodeContext<'_>| ctx.retained_vec::<u8>(2, "test retained vec").map(|_| ()));
    operation_case!(reserve_retained_admitted_vec_refuses_before_growth, ResourceDimension::RetainedBytes, 2,
        |ctx: &DecodeContext<'_>| { let mut values = Vec::<u8>::new(); let result = ctx.reserve_retained_admitted_vec(&mut values, 2, "test retained admitted vec"); if result.is_err() { assert_eq!(values.capacity(), 0); } result });
    operation_case!(reserve_scoped_vec_refuses_before_growth, ResourceDimension::MaterializedBytes, 2,
        |ctx: &DecodeContext<'_>| { let mut reservation = ctx.reserve_scoped(0, "test scoped vec")?; let mut values = Vec::<u8>::new(); let result = ctx.reserve_scoped_vec(&mut reservation, &mut values, 2, "test scoped vec"); if result.is_err() { assert_eq!(values.capacity(), 0); } result });
    operation_case!(reserve_temporary_vec_refuses_before_growth, ResourceDimension::MaterializedBytes, 2,
        |ctx: &DecodeContext<'_>| { let mut values = Vec::<u8>::new(); let result = ctx.reserve_temporary_vec(&mut values, 2, "test temporary vec").map(|_| ()); if result.is_err() { assert_eq!(values.capacity(), 0); } result });
    operation_case!(reserve_temporary_vec_optional_limit_refuses_before_growth, ResourceDimension::MaterializedBytes, 2,
        |ctx: &DecodeContext<'_>| { let mut values = Vec::<u8>::new(); let result = DecodeContext::reserve_temporary_vec_optional_limit(Some(ctx), &mut values, 2, "test geometry vec").map(|_| ()).map_err(CodecError::from); if result.is_err() { assert_eq!(values.capacity(), 0); } result });
    operation_case!(collect_retained_texts_refuses_at_byte_limit, ResourceDimension::RetainedBytes, super::u64_from_index(std::mem::size_of::<String>() + 2),
        |ctx: &DecodeContext<'_>| ctx.collect_retained_texts(["ab"], "test retained texts").map(|_| ()));
    operation_case!(collect_scoped_texts_refuses_at_byte_limit, ResourceDimension::MaterializedBytes, super::u64_from_index(std::mem::size_of::<String>() + 2),
        |ctx: &DecodeContext<'_>| ctx.collect_scoped_texts(["ab"], "test scoped texts").map(|_| ()));
    operation_case!(format_retained_with_work_refuses_before_formatting, ResourceDimension::WorkUnits, 2,
        |ctx: &DecodeContext<'_>| ctx.format_retained_with_work(format_args!("ab"), "test formatted work").map(|_| ()));
    operation_case!(format_scoped_text_with_work_refuses_before_formatting, ResourceDimension::WorkUnits, 2,
        |ctx: &DecodeContext<'_>| { let mut reservation = ctx.reserve_scoped(0, "test scoped formatted work")?; ctx.format_scoped_text_with_work(&mut reservation, format_args!("ab"), "test scoped formatted work").map(|_| ()) });
    operation_case!(reserve_record_vec_refuses_before_growth, ResourceDimension::Entities, 2,
        |ctx: &DecodeContext<'_>| { let mut values = Vec::<u8>::new(); let result = ctx.reserve_record_vec(&mut values, 2, 0, "test record vec"); if result.is_err() { assert_eq!(values.capacity(), 0); } result });

    #[test]
    fn copy_admitted_text_preserves_utf8() {
        assert_eq!(DecodeContext::copy_admitted_text("aé", "test admitted text").expect("admitted text"), "aé");
    }
    operation_case!(copy_temporary_slice_optional_limit_refuses_before_copy, ResourceDimension::MaterializedBytes, 2,
        |ctx: &DecodeContext<'_>| DecodeContext::copy_temporary_slice_optional_limit(Some(ctx), &[1u8, 2], "test temporary copy").map(|_| ()).map_err(CodecError::from));
    operation_case!(collect_retained_vec_refuses_before_first_allocation, ResourceDimension::RetainedBytes, 2,
        |ctx: &DecodeContext<'_>| ctx.collect_retained_vec([1u16], "test retained collection").map(|_| ()));
    operation_case!(copy_slice_with_work_refuses_before_copy, ResourceDimension::WorkUnits, 2,
        |ctx: &DecodeContext<'_>| ctx.copy_slice_with_work(&[1u8, 2], "test work copy").map(|_| ()));

    #[test]
    fn admitted_string_reserve_preserves_prefix_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        ctx.charge_retained(2, "test admitted text slots").expect("service admits text");
        let mut text = String::from("a");
        DecodeContext::reserve_admitted_string(&mut text, 2, "test admitted text slots").expect("text allocation");
        assert_eq!(text, "a");
        assert!(text.capacity() >= 3);
    }
    #[test]
    fn admitted_string_reserve_follows_one_below_limit_refusal_before_growth() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 1);
        let mut text = String::new();
        let result = ctx.charge_retained(2, "test admitted text slots").and_then(|()| DecodeContext::reserve_admitted_string(&mut text, 2, "test admitted text slots"));
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes));
        assert_eq!(text.capacity(), 0);
    }

    #[test]
    fn push_retained_vec_refuses_one_below_storage_before_allocation() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::RetainedBytes, 1);
        let mut values = Vec::<u16>::new();
        let error = ctx.push_retained_vec(&mut values, 7, "test retained push").unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes));
        assert_eq!(values.capacity(), 0);
        assert!(values.is_empty());
    }

    #[test]
    fn push_retained_vec_keeps_value_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut values = Vec::<u16>::new();
        ctx.push_retained_vec(&mut values, 7, "test retained push").unwrap();
        assert_eq!(values, [7]);
    }

    #[test]
    fn scoped_tree_set_refuses_one_below_storage_before_insertion() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 3);
        let mut reservation = ctx.reserve_scoped(0, "test scoped tree").unwrap();
        let mut values = BTreeSet::new();
        let error = ctx.insert_scoped_btree_set(&mut reservation, &mut values, 7u8, "test scoped lookup", "test scoped tree").unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes));
        assert!(values.is_empty());
    }

    #[test]
    fn scoped_tree_set_preserves_unique_values_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut reservation = ctx.reserve_scoped(0, "test scoped tree").unwrap();
        let mut values = BTreeSet::new();
        assert!(ctx.insert_scoped_btree_set(&mut reservation, &mut values, 7u8, "test scoped lookup", "test scoped tree").unwrap());
        assert!(!ctx.insert_scoped_btree_set(&mut reservation, &mut values, 7u8, "test scoped lookup", "test scoped tree").unwrap());
        assert_eq!(values, BTreeSet::from([7]));
    }

    #[test]
    fn scoped_tree_map_refuses_one_below_storage_before_insertion() {
        let arena = DecodeArena::new();
        let ctx = operation_context(&arena, ResourceDimension::MaterializedBytes, 7);
        let mut reservation = ctx.reserve_scoped(0, "test scoped tree").unwrap();
        let mut values = BTreeMap::new();
        let error = ctx.insert_scoped_btree_map_if_vacant(&mut reservation, &mut values, 7u8, 9u8, "test scoped lookup", "test scoped tree").unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes));
        assert!(values.is_empty());
    }

    #[test]
    fn scoped_tree_map_preserves_first_entry_under_service_profile() {
        let arena = DecodeArena::new();
        let ctx = context(&arena, DecodePolicy::service().limits.max_collection_items);
        let mut reservation = ctx.reserve_scoped(0, "test scoped tree").unwrap();
        let mut values = BTreeMap::new();
        assert!(ctx.insert_scoped_btree_map_if_vacant(&mut reservation, &mut values, 7u8, 9u8, "test scoped lookup", "test scoped tree").unwrap());
        assert!(!ctx.insert_scoped_btree_map_if_vacant(&mut reservation, &mut values, 7u8, 11u8, "test scoped lookup", "test scoped tree").unwrap());
        assert_eq!(values, BTreeMap::from([(7, 9)]));
    }

}
