// SPDX-License-Identifier: Apache-2.0
//! Admit retained native byte-copy work before copying.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

pub(crate) fn copy_retained(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<Vec<u8>, CodecError> {
    ctx.charge_work(u64_from_index(bytes.len()), operation)?;
    ctx.copy_retained(bytes, operation)
}

pub(crate) fn concat_retained(
    ctx: &DecodeContext<'_>,
    parts: &[Vec<u8>],
    operation: &'static str,
) -> Result<Vec<u8>, CodecError> {
    ctx.charge_work(u64_from_index(parts.len()), operation)?;
    let bytes = parts
        .iter()
        .try_fold(0u64, |bytes, part| {
            bytes.checked_add(u64_from_index(part.len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(bytes, operation)?;
    ctx.concat_retained(parts, operation)
}
