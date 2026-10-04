// SPDX-License-Identifier: Apache-2.0
//! Shared feature-byte helpers used by definition and row decoders.

use cadmpeg_core::decode::{bounded_len, DecodeContext};
use cadmpeg_core::CodecError;

use crate::psb;
use crate::scalar;

pub(super) fn decode_exact_scalars(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    slot_count: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<Vec<f64>>, CodecError> {
    // Each slot decodes at least one payload byte and the whole payload must be
    // consumed, so a valid slot count cannot exceed the payload length.
    if bounded_len(
        cadmpeg_core::decode::u64_from_index(slot_count),
        1,
        payload.len(),
    )
    .is_none()
    {
        return Ok(None);
    }
    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, slot_count, "creo feature scalar values")?;
    let mut cursor = psb::Cursor::new(payload);
    for _ in 0..slot_count {
        let Some(value) = cursor.take_with(|data, pos| scalar::decode_in_lane(data, pos, cache))
        else {
            return Ok(None);
        };
        values.push(value);
    }
    Ok((cursor.pos() == payload.len()).then_some(values))
}
