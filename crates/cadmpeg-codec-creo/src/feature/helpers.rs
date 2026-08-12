// SPDX-License-Identifier: Apache-2.0
//! Shared feature-byte helpers used by definition and row decoders.

use cadmpeg_core::decode::bounded_len;

use crate::psb;
use crate::scalar;

pub(crate) fn decode_exact_scalars(
    payload: &[u8],
    slot_count: usize,
    cache: &scalar::ScalarCache,
) -> Option<Vec<f64>> {
    // Each slot decodes at least one payload byte and the whole payload must be
    // consumed, so a valid slot count cannot exceed the payload length.
    bounded_len(slot_count as u64, 1, payload.len())?;
    let mut values = Vec::with_capacity(slot_count);
    let mut cursor = psb::Cursor::new(payload);
    for _ in 0..slot_count {
        values.push(cursor.take_with(|data, pos| scalar::decode_in_lane(data, pos, cache))?);
    }
    (cursor.pos() == payload.len()).then_some(values)
}

pub(crate) fn decode_optional_scalars(
    payload: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> (Vec<Option<f64>>, Vec<Vec<u8>>) {
    let mut values = Vec::with_capacity(count);
    let mut bodies = Vec::with_capacity(count);
    let mut cursor = 0;
    for _ in 0..count {
        if cursor >= payload.len() || payload.get(cursor) == Some(&psb::token::NAMED_RECORD) {
            values.push(None);
            bodies.push(Vec::new());
            continue;
        }
        let start = cursor;
        if let Some((value, next)) = scalar::decode_in_lane(payload, cursor, cache) {
            values.push(Some(value));
            cursor = next;
        } else {
            values.push(None);
            cursor += 1;
        }
        bodies.push(payload[start..cursor].to_vec());
    }
    (values, bodies)
}

pub(crate) fn find_bytes(payload: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    payload
        .get(start..end)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| start + relative)
}
