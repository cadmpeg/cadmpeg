// SPDX-License-Identifier: Apache-2.0
//! Charged fallible growth for CATIA decode collections.

use std::collections::{HashSet, VecDeque};
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
