// SPDX-License-Identifier: Apache-2.0
//! Resource admission for native validation copies.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_ir::NativeConvertError;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;

pub(super) fn collect_index_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    count: usize,
    items: impl Iterator<Item = T>,
    operation: &'static str,
) -> Result<HashSet<T>, NativeConvertError> {
    ctx.charge_collection_items(
        u64::try_from(count)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    let mut result = HashSet::new();
    result
        .try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    result.extend(items);
    Ok(result)
}

pub(super) fn collect_index_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    count: usize,
    items: impl Iterator<Item = (K, V)>,
    operation: &'static str,
) -> Result<HashMap<K, V>, NativeConvertError> {
    ctx.charge_collection_items(
        u64::try_from(count)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    let mut result = HashMap::new();
    result
        .try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    result.extend(items);
    Ok(result)
}

struct FormattedByteCount(usize);

impl fmt::Write for FormattedByteCount {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
}

pub(super) fn invalid_owner(
    ctx: &DecodeContext<'_>,
    message: fmt::Arguments<'_>,
) -> Result<NativeConvertError, NativeConvertError> {
    let mut count = FormattedByteCount(0);
    fmt::write(&mut count, message).map_err(|_| {
        ctx.refuse_codec_limit("format SLDPRT native validation error", u64::MAX - 1, u64::MAX)
    })?;
    let mut text = String::new();
    ctx.reserve_retained_string(
        &mut text,
        count.0,
        "format SLDPRT native validation error",
    )?;
    fmt::write(&mut text, message).map_err(|_| {
        NativeConvertError::InvalidOwner("cannot format SLDPRT native validation error".into())
    })?;
    Ok(NativeConvertError::InvalidOwner(text))
}

struct SerializedByteCount {
    bytes: u64,
    overflowed: bool,
}

impl std::io::Write for SerializedByteCount {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let next = u64::try_from(buffer.len())
            .ok()
            .and_then(|length| self.bytes.checked_add(length));
        let Some(next) = next else {
            self.overflowed = true;
            return Err(std::io::Error::other("native clone byte count overflow"));
        };
        self.bytes = next;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn count_copy<'a, T: Serialize + 'a>(
    ctx: &DecodeContext<'_>,
    mut records: impl Iterator<Item = &'a T> + Clone,
    operation: &'static str,
) -> Result<u64, NativeConvertError> {
    let scan = records.size_hint().1.unwrap_or(records.size_hint().0);
    ctx.charge_work(
        u64::try_from(scan)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    let count = records.clone().count();
    ctx.charge_collection_items(
        u64::try_from(count)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    let mut counter = SerializedByteCount {
        bytes: 0,
        overflowed: false,
    };
    let counted = records.try_for_each(|record| serde_json::to_writer(&mut counter, record));
    if counter.overflowed {
        return Err(ctx
            .refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            .into());
    }
    counted?;
    Ok(counter.bytes)
}

pub(super) fn admit_retained_clones<'a, T: Serialize + 'a>(
    ctx: &DecodeContext<'_>,
    records: impl Iterator<Item = &'a T> + Clone,
    operation: &'static str,
) -> Result<(), NativeConvertError> {
    let bytes = count_copy(ctx, records, operation)?;
    ctx.charge_retained(bytes, operation)?;
    Ok(())
}

pub(super) fn collect_retained_clones<'a, T: Clone + Serialize + 'a>(
    ctx: &DecodeContext<'_>,
    records: impl Iterator<Item = &'a T> + Clone,
    operation: &'static str,
) -> Result<Vec<T>, NativeConvertError> {
    let count = records.clone().count();
    admit_retained_clones(ctx, records.clone(), operation)?;
    let mut result = Vec::new();
    result
        .try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    result.extend(records.cloned());
    Ok(result)
}

pub(super) fn admit_temporary_clones<'a, 'ctx, T: Serialize + 'a>(
    ctx: &'ctx DecodeContext<'_>,
    records: impl Iterator<Item = &'a T> + Clone,
    operation: &'static str,
) -> Result<ScopedReservation<'ctx>, NativeConvertError> {
    let bytes = count_copy(ctx, records, operation)?;
    Ok(ctx.reserve_scoped(bytes, operation)?)
}

pub(crate) fn collect_temporary_clones<'a, 'ctx, T: Clone + Serialize + 'a>(
    ctx: &'ctx DecodeContext<'_>,
    records: impl Iterator<Item = &'a T> + Clone,
    operation: &'static str,
) -> Result<(Vec<T>, ScopedReservation<'ctx>), NativeConvertError> {
    let count = records.clone().count();
    let reservation = admit_temporary_clones(ctx, records.clone(), operation)?;
    let mut result = Vec::new();
    result
        .try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    result.extend(records.cloned());
    Ok((result, reservation))
}

/// Admit scratch storage before validation constructs candidate collections.
pub(super) fn admit_validation_candidates<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    source_units: usize,
    operation: &'static str,
) -> Result<ScopedReservation<'ctx>, NativeConvertError> {
    let count = u64::try_from(source_units)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(count, operation)?;
    ctx.charge_collection_items(count, operation)?;
    let bytes = count
        .checked_mul(64)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    Ok(ctx.reserve_scoped(bytes, operation)?)
}
