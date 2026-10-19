// SPDX-License-Identifier: Apache-2.0
//! Shared feature-byte helpers used by definition and row decoders.

use cadmpeg_core::decode::bounded_len;

use crate::psb;
use crate::scalar;

pub(super) fn decode_exact_scalars(
    payload: &[u8],
    slot_count: usize,
    cache: &scalar::ScalarCache,
) -> Option<Vec<f64>> {
    // Each slot decodes at least one payload byte and the whole payload must be
    // consumed, so a valid slot count cannot exceed the payload length.
    bounded_len(
        cadmpeg_core::decode::u64_from_index(slot_count),
        1,
        payload.len(),
    )?;
    let mut values = Vec::new();
    let mut cursor = psb::Cursor::new(payload);
    for _ in 0..slot_count {
        let value = cursor.take_with(|data, pos| scalar::decode_in_lane(data, pos, cache))?;
        values.push(value);
    }
    (cursor.pos() == payload.len()).then_some(values)
}

pub(super) fn find_bytes(payload: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    cadmpeg_core::bytes::find_in(payload, needle, start, end)
}
