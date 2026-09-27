// SPDX-License-Identifier: Apache-2.0
//! Charged collection allocation for ASM decode paths.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

pub(crate) fn counted_vec<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let count_u64 = u64::try_from(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_collection_items(count_u64, operation)?;
    let mut values = Vec::new();
    values
        .try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, count_u64))?;
    Ok(values)
}

pub(crate) fn push_vec<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    reserve_vec_slot(ctx, values, operation)?;
    values.push(value);
    Ok(())
}

pub(crate) fn reserve_vec_slot<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(())
}

pub(crate) fn collect_vec<T>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut out = Vec::new();
    for value in values {
        push_vec(ctx, &mut out, value, operation)?;
    }
    Ok(out)
}

pub(crate) fn collect_hash_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    operation: &'static str,
) -> Result<HashSet<T>, CodecError> {
    let mut out = HashSet::new();
    for value in values {
        if !out.contains(&value) {
            ctx.charge_collection_items(1, operation)?;
            out.try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
            out.insert(value);
        }
    }
    Ok(out)
}

pub(crate) fn insert_hash_set<T: Eq + Hash>(
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
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(values.insert(value))
}

pub(crate) fn collect_hash_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = (K, V)>,
    operation: &'static str,
) -> Result<HashMap<K, V>, CodecError> {
    let mut out = HashMap::new();
    for (key, value) in values {
        if !out.contains_key(&key) {
            ctx.charge_collection_items(1, operation)?;
            out.try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
        }
        out.insert(key, value);
    }
    Ok(out)
}

pub(crate) fn insert_hash_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<Option<V>, CodecError> {
    reserve_hash_map_entry(ctx, values, &key, operation)?;
    Ok(values.insert(key, value))
}

pub(crate) fn reserve_hash_map_entry<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    key: &K,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !values.contains_key(key) {
        ctx.charge_collection_items(1, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    }
    Ok(())
}

pub(crate) trait CountedIteratorExt: Iterator + Sized {
    fn collect_counted_vec(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Vec<Self::Item>, CodecError> {
        collect_vec(ctx, self, operation)
    }

    fn collect_counted_set(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<HashSet<Self::Item>, CodecError>
    where
        Self::Item: Eq + Hash,
    {
        collect_hash_set(ctx, self, operation)
    }

    fn collect_counted_map<K: Eq + Hash, V>(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<HashMap<K, V>, CodecError>
    where
        Self: Iterator<Item = (K, V)>,
    {
        collect_hash_map(ctx, self, operation)
    }

    fn try_collect_counted_vec<T, E: From<CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Vec<T>, E>
    where
        Self: Iterator<Item = Result<T, E>>,
    {
        try_collect_vec(ctx, self, operation)
    }
}

impl<I: Iterator> CountedIteratorExt for I {}

pub(crate) fn try_collect_vec<T, E: From<CodecError>>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = Result<T, E>>,
    operation: &'static str,
) -> Result<Vec<T>, E> {
    let mut out = Vec::new();
    for value in values {
        push_vec(ctx, &mut out, value?, operation)?;
    }
    Ok(out)
}
