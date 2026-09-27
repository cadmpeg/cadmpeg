// SPDX-License-Identifier: Apache-2.0
//! Resource admission for native validation copies.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_ir::NativeConvertError;
use serde::Serialize;

#[derive(Clone, Copy)]
pub(super) enum NativeAdmission<'ctx, 'arena> {
    Cadir,
    Decode(&'ctx DecodeContext<'arena>),
}

impl<'ctx, 'arena> NativeAdmission<'ctx, 'arena> {
    pub(super) fn context(self) -> Option<&'ctx DecodeContext<'arena>> {
        match self {
            Self::Cadir => None,
            Self::Decode(ctx) => Some(ctx),
        }
    }
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
    admission: NativeAdmission<'_, '_>,
    records: impl Iterator<Item = &'a T> + Clone,
    operation: &'static str,
) -> Result<(), NativeConvertError> {
    if let Some(ctx) = admission.context() {
        let bytes = count_copy(ctx, records, operation)?;
        ctx.charge_retained(bytes, operation)?;
    }
    Ok(())
}

pub(super) fn admit_temporary_clones<'a, 'ctx, T: Serialize + 'a>(
    admission: NativeAdmission<'ctx, '_>,
    records: impl Iterator<Item = &'a T> + Clone,
    operation: &'static str,
) -> Result<Option<ScopedReservation<'ctx>>, NativeConvertError> {
    match admission.context() {
        Some(ctx) => {
            let bytes = count_copy(ctx, records, operation)?;
            Ok(Some(ctx.reserve_scoped(bytes, operation)?))
        }
        None => Ok(None),
    }
}

/// Admit scratch storage before validation constructs candidate collections.
pub(super) fn admit_validation_candidates<'ctx>(
    admission: NativeAdmission<'ctx, '_>,
    source_units: usize,
    operation: &'static str,
) -> Result<Option<ScopedReservation<'ctx>>, NativeConvertError> {
    let Some(ctx) = admission.context() else {
        return Ok(None);
    };
    let count = u64::try_from(source_units)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(count, operation)?;
    ctx.charge_collection_items(count, operation)?;
    let bytes = count
        .checked_mul(64)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    Ok(Some(ctx.reserve_scoped(bytes, operation)?))
}
