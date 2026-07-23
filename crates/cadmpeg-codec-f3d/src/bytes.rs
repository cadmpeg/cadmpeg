// SPDX-License-Identifier: Apache-2.0
//! Length-prefixed string readers and GUID predicates shared by the record
//! decoders.
//!
//! Each reader takes a u32 length prefix and returns the decoded string with
//! the offset past its payload. The length bounds and charset policy differ
//! between record streams and are load-bearing: they decide which byte windows
//! parse as strings during heuristic scans. Callers pass their exact policy
//! through the `bounds` and `allowed` parameters rather than sharing one
//! unified policy.

use std::ops::RangeInclusive;

use cadmpeg_ir::wire::le::{lp_u32_bytes_at, take_lp_u32_bytes, u32_at, utf16le_at};

/// Read a u32-length-prefixed ASCII string whose length lies in `bounds`,
/// decoding it as strict UTF-8. Returns the string and the offset past it.
pub(crate) fn lp_ascii_strict(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Option<(String, usize)> {
    let length = usize::try_from(u32_at(bytes, at)?).ok()?;
    if !bounds.contains(&length) {
        return None;
    }
    let (raw, end) = lp_u32_bytes_at(bytes, at)?;
    Some((std::str::from_utf8(raw).ok()?.to_owned(), end))
}

/// Read a u32-length-prefixed ASCII string whose length lies in `bounds` and
/// whose every byte satisfies `allowed`, decoding the payload lossily. Returns
/// the string and the offset past it, or `None` when a byte is rejected.
pub(crate) fn lp_ascii_filtered(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
    allowed: fn(&u8) -> bool,
) -> Option<(String, usize)> {
    let length = usize::try_from(u32_at(bytes, at)?).ok()?;
    if !bounds.contains(&length) {
        return None;
    }
    let (raw, end) = lp_u32_bytes_at(bytes, at)?;
    raw.iter()
        .all(allowed)
        .then(|| (String::from_utf8_lossy(raw).into_owned(), end))
}

/// Read a u32-count-prefixed UTF-16LE string whose code-unit count lies in
/// `bounds`, decoding it strictly. Returns the string and the offset past its
/// code units.
pub(crate) fn lp_utf16_bounded(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Option<(String, usize)> {
    let count = usize::try_from(u32_at(bytes, at)?).ok()?;
    if !bounds.contains(&count) {
        return None;
    }
    utf16le_at(bytes, at.checked_add(4)?, count)
}

/// Take a u32-length-prefixed strict-UTF-8 string, advancing `at` past it on
/// success.
pub(crate) fn take_lp_utf8(bytes: &[u8], at: &mut usize) -> Option<String> {
    String::from_utf8(take_lp_u32_bytes(bytes, at)?.to_vec()).ok()
}

/// Take a u32-length-prefixed strict-UTF-8 string whose byte length is at most
/// `max`, advancing `at`. The cursor advances past the four-byte prefix even
/// when the length exceeds `max` or the payload is not valid UTF-8.
pub(crate) fn take_lp_utf8_capped(bytes: &[u8], at: &mut usize, max: usize) -> Option<String> {
    let count = usize::try_from(u32_at(bytes, *at)?).ok()?;
    *at = at.checked_add(4)?;
    if count > max {
        return None;
    }
    let end = at.checked_add(count)?;
    let value = std::str::from_utf8(bytes.get(*at..end)?).ok()?.to_owned();
    *at = end;
    Some(value)
}

/// Whether `value` is a 36-character hyphenated hexadecimal GUID.
pub(crate) fn is_guid_hyphenated(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// Whether `value` is 36 to 38 characters of alphanumerics, `-`, and `_`.
pub(crate) fn is_guid_relaxed(value: &str) -> bool {
    matches!(value.len(), 36..=38)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Whether the first 36 bytes of `value` form a hyphenated hexadecimal GUID.
pub(crate) fn is_guid_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 36
        && bytes[..36].iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// Encode `value` as a u32-code-unit-prefixed UTF-16LE byte sequence, the form
/// used both as a search needle and by test fixtures.
pub(crate) fn lp_utf16_bytes(value: &str) -> Vec<u8> {
    let units: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = ((units.len() / 2) as u32).to_le_bytes().to_vec();
    out.extend(units);
    out
}
