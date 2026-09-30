// SPDX-License-Identifier: Apache-2.0
//! Resource admission for native validation copies.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use crate::records::charged_clone::CloneCharged;
use cadmpeg_ir::NativeConvertError;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt;

pub(super) fn collect_index_set<'a>(
    ctx: &DecodeContext<'_>,
    count: usize,
    items: impl Iterator<Item = &'a str>,
    operation: &'static str,
) -> Result<HashSet<&'a str>, NativeConvertError> {
    ctx.charge_collection_items(
        u64::try_from(count)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    let mut result = HashSet::new();
    result
        .try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    for key in items {
        let work = cadmpeg_core::decode::u64_from_index(key.len()).checked_mul(2).and_then(|work| work.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, operation)?;
        result.insert(key);
    }
    Ok(result)
}

pub(super) fn collect_index_map<'a, V>(
    ctx: &DecodeContext<'_>,
    count: usize,
    items: impl Iterator<Item = (&'a str, V)>,
    operation: &'static str,
) -> Result<HashMap<&'a str, V>, NativeConvertError> {
    ctx.charge_collection_items(
        u64::try_from(count)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    let mut result = HashMap::new();
    result
        .try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    for (key, value) in items {
        let work = cadmpeg_core::decode::u64_from_index(key.len()).checked_mul(2).and_then(|work| work.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, operation)?;
        result.insert(key, value);
    }
    Ok(result)
}

struct FormattedByteCount<'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    bytes: usize,
    failure: Option<cadmpeg_core::CodecError>,
}

impl fmt::Write for FormattedByteCount<'_, '_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if let Err(error) = self.ctx.charge_work(cadmpeg_core::decode::u64_from_index(text.len()), "format SLDPRT native validation error") {
            self.failure = Some(error);
            return Err(fmt::Error);
        }
        self.bytes = self.bytes.checked_add(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
}

pub(super) fn invalid_owner(
    ctx: &DecodeContext<'_>,
    message: fmt::Arguments<'_>,
) -> Result<NativeConvertError, NativeConvertError> {
    let mut count = FormattedByteCount { ctx, bytes: 0, failure: None };
    if fmt::write(&mut count, message).is_err() {
        return Err(count.failure.unwrap_or_else(|| {
            ctx.refuse_codec_limit("format SLDPRT native validation error", u64::MAX - 1, u64::MAX)
        }).into());
    }
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(count.bytes), "format SLDPRT native validation error")?;
    let mut text = String::new();
    crate::text_admission::reserve_retained_string(ctx, 
        &mut text,
        count.bytes,
        "format SLDPRT native validation error",
    )?;
    fmt::write(&mut text, message).map_err(|_| {
        NativeConvertError::InvalidOwner("cannot format SLDPRT native validation error".into())
    })?;
    Ok(NativeConvertError::InvalidOwner(text))
}

struct SerializedByteCount<'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    operation: &'static str,
    refusal: Option<cadmpeg_core::CodecError>,
    bytes: u64,
    overflowed: bool,
}

impl std::io::Write for SerializedByteCount<'_, '_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if let Err(refusal) = self.ctx.charge_work(cadmpeg_core::decode::u64_from_index(buffer.len()), self.operation) {
            self.refusal = Some(refusal);
            return Err(std::io::Error::other("native copy work limit exceeded"));
        }
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

fn count_records(
    ctx: &DecodeContext<'_>,
    mut records: impl Iterator,
    operation: &'static str,
) -> Result<usize, NativeConvertError> {
    let scan = records.size_hint().1.unwrap_or(records.size_hint().0);
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(scan), operation)?;
    let mut count = 0usize;
    loop {
        ctx.charge_work(1, operation)?;
        if records.next().is_none() {
            return Ok(count);
        }
        count = count.checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    }
}

fn count_copy<'a, T: Serialize + 'a>(
    ctx: &DecodeContext<'_>,
    mut records: impl Iterator<Item = &'a T> + Clone,
    operation: &'static str,
) -> Result<u64, NativeConvertError> {
    let count = count_records(ctx, records.clone(), operation)?;
    ctx.charge_collection_items(
        u64::try_from(count)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    let mut counter = SerializedByteCount {
        ctx,
        operation,
        refusal: None,
        bytes: 0,
        overflowed: false,
    };
    let counted = records.try_for_each(|record| serde_json::to_writer(&mut counter, record));
    if let Some(refusal) = counter.refusal {
        return Err(refusal.into());
    }
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

pub(super) fn collect_retained_clones<'a, T: CloneCharged + Serialize + 'a>(
    ctx: &DecodeContext<'_>,
    records: impl Iterator<Item = &'a T> + Clone,
    operation: &'static str,
) -> Result<Vec<T>, NativeConvertError> {
    let count = count_records(ctx, records.clone(), operation)?;
    admit_retained_clones(ctx, records.clone(), operation)?;
    let mut result = Vec::new();
    result
        .try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    for record in records {
        result.push(record.clone_charged(ctx, operation)?);
    }
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

pub(crate) fn collect_temporary_clones<'a, 'ctx, T: CloneCharged + Serialize + 'a>(
    ctx: &'ctx DecodeContext<'_>,
    records: impl Iterator<Item = &'a T> + Clone,
    operation: &'static str,
) -> Result<(Vec<T>, ScopedReservation<'ctx>), NativeConvertError> {
    let count = count_records(ctx, records.clone(), operation)?;
    let reservation = admit_temporary_clones(ctx, records.clone(), operation)?;
    let mut result = Vec::new();
    result
        .try_reserve(count)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    for record in records {
        result.push(record.clone_charged(ctx, operation)?);
    }
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
