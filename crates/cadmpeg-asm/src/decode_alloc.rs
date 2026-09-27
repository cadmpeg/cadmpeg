// SPDX-License-Identifier: Apache-2.0
//! Charged collection allocation for ASM decode paths.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

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
