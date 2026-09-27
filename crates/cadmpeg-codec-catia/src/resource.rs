// SPDX-License-Identifier: Apache-2.0
//! Charged fallible growth for CATIA decode collections.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

use cadmpeg_core::decode::{DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit};
use cadmpeg_core::CodecError;

fn allocation_failed(
    used: usize,
    capacity: usize,
    additional: usize,
    operation: &'static str,
) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::Codec(operation),
        reason: ResourceFailure::AllocationFailed,
        limit: capacity as u64,
        used: used as u64,
        additional: additional as u64,
        operation,
    })
}

pub(crate) fn push<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    values.push(value);
    Ok(())
}

pub(crate) fn push_back<T>(
    ctx: &DecodeContext<'_>,
    values: &mut VecDeque<T>,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    values.push_back(value);
    Ok(())
}

pub(crate) fn reserve_vec<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(additional as u64, operation)?;
    values
        .try_reserve(additional)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), additional, operation))
}

pub(crate) fn copy_slice<T: Clone>(
    ctx: &DecodeContext<'_>,
    values: &[T],
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut copy = Vec::new();
    reserve_vec(ctx, &mut copy, values.len(), operation)?;
    copy.extend_from_slice(values);
    Ok(copy)
}

pub(crate) fn reserve_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    values: &mut HashSet<T>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(count as u64, operation)?;
    values
        .try_reserve(count)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), count, operation))
}

pub(crate) fn reserve_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(count as u64, operation)?;
    values
        .try_reserve(count)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), count, operation))
}

pub(crate) fn insert_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    values: &mut HashSet<T>,
    value: T,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if values.contains(&value) {
        return Ok(false);
    }
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    Ok(values.insert(value))
}

pub(crate) fn insert_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<Option<V>, CodecError> {
    if !values.contains_key(&key) {
        ctx.charge_collection_items(1, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    }
    Ok(values.insert(key, value))
}
