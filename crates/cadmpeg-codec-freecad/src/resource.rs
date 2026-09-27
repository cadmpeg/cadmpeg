// SPDX-License-Identifier: Apache-2.0
//! Fallible collection admission for FreeCAD decoding.

use cadmpeg_core::decode::{DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit};
use cadmpeg_core::CodecError;
use std::collections::HashSet;
use std::hash::Hash;

pub(crate) fn collection_vec<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    ctx.charge_collection_items(count as u64, operation)?;
    reserved_vec(ctx, count, operation)
}

pub(crate) fn reserved_vec<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut items = Vec::new();
    items.try_reserve_exact(count).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::CollectionItems,
            reason: ResourceFailure::AllocationFailed,
            limit: ctx.policy().limits.max_collection_items,
            used: 0,
            additional: count as u64,
            operation,
        })
    })?;
    Ok(items)
}

pub(crate) fn optional_collection_vec<T>(
    ctx: &DecodeContext<'_>,
    present: bool,
    count: usize,
    operation: &'static str,
) -> Result<Option<Vec<T>>, CodecError> {
    if present {
        Ok(Some(collection_vec(ctx, count, operation)?))
    } else {
        Ok(None)
    }
}

pub(crate) fn decode_reserved_vec<T>(
    ctx: Option<&DecodeContext<'_>>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    match ctx {
        Some(ctx) => reserved_vec(ctx, count, operation),
        None => Ok(Vec::new()),
    }
}

pub(crate) fn reserve_charged_vec_items<T>(
    ctx: Option<&DecodeContext<'_>>,
    items: &mut Vec<T>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(ctx) = ctx {
        items.try_reserve(count).map_err(|_| collection_allocation_failed(ctx, count as u64, operation))?;
    }
    Ok(())
}

pub(crate) fn reserve_vec_items<T>(
    ctx: &DecodeContext<'_>,
    items: &mut Vec<T>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(count as u64, operation)?;
    items.try_reserve(count).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::CollectionItems,
            reason: ResourceFailure::AllocationFailed,
            limit: ctx.policy().limits.max_collection_items,
            used: 0,
            additional: count as u64,
            operation,
        })
    })
}

pub(crate) fn retained_string(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    String::from_utf8(ctx.copy_retained(value.as_bytes(), operation)?)
        .map_err(|_| CodecError::Malformed("retained text lost UTF-8 encoding".into()))
}

pub(crate) fn retained_suffix(
    ctx: &DecodeContext<'_>,
    value: &str,
    suffix: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut output = retained_string(ctx, value, operation)?;
    ctx.charge_retained(suffix.len() as u64, operation)?;
    output.try_reserve(suffix.len()).map_err(|_| {
        CodecError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::RetainedBytes,
            reason: ResourceFailure::AllocationFailed,
            limit: ctx.policy().limits.max_retained_bytes,
            used: 0,
            additional: suffix.len() as u64,
            operation,
        })
    })?;
    output.push_str(suffix);
    Ok(output)
}

pub(crate) fn insert_hash_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    items: &mut HashSet<T>,
    value: T,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if items.contains(&value) {
        return Ok(false);
    }
    ctx.charge_collection_items(1, operation)?;
    items.try_reserve(1).map_err(|_| collection_allocation_failed(ctx, 1, operation))?;
    Ok(items.insert(value))
}

pub(crate) fn collection_allocation_failed(
    ctx: &DecodeContext<'_>,
    count: u64,
    operation: &'static str,
) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::CollectionItems,
        reason: ResourceFailure::AllocationFailed,
        limit: ctx.policy().limits.max_collection_items,
        used: 0,
        additional: count,
        operation,
    })
}
