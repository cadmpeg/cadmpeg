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

use cadmpeg_asm::kernel_header::RefWidth;
use std::ops::RangeInclusive;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

/// Read a signed little-endian integer with a four- or eight-byte width.
pub(crate) fn int_at(bytes: &[u8], offset: usize, width: RefWidth) -> Option<i64> {
    match width {
        RefWidth::Four => Some(i64::from(View::i32_le_at(bytes, offset)?)),
        RefWidth::Eight => View::i64_le_at(bytes, offset),
    }
}

/// Read a u32-length-prefixed byte slice and return it with the end offset.
fn lp_u32_bytes_at(bytes: &[u8], offset: usize) -> Option<(&[u8], usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, offset)?).ok()?;
    let start = offset.checked_add(4)?;
    let end = start.checked_add(length)?;
    Some((bytes.get(start..end)?, end))
}

/// Take a u32-length-prefixed byte slice, advancing only on success.
fn take_lp_u32_bytes<'a>(bytes: &'a [u8], position: &mut usize) -> Option<&'a [u8]> {
    let mut view = View::over_retained(bytes);
    view.seek(*position)?;
    let length = usize::try_from(view.u32_le()?).ok()?;
    let value = view.take(length)?;
    *position = view.position();
    Some(value)
}

/// Decode `count` UTF-16LE code units at `offset` and return the end offset.
pub(crate) fn utf16le_at(bytes: &[u8], offset: usize, count: usize) -> Option<(String, usize)> {
    View::utf16le_at(bytes, offset, count)
}

/// Read consecutive little-endian `f64` values at `offset`.
pub(crate) fn f64s_at(bytes: &[u8], offset: usize, count: usize) -> Option<Vec<f64>> {
    let mut view = View::over_retained(bytes);
    view.seek(offset)?;
    view.read_counted(u64::try_from(count).ok()?, 8, View::f64_le)
}

/// Read `N` consecutive little-endian `f64` values at `offset`, or `None`
/// when one is not finite.
pub(crate) fn finite_reals_at<const N: usize>(
    bytes: &[u8],
    offset: usize,
) -> Option<[FiniteReal; N]> {
    let mut admitted = [FiniteReal::ZERO; N];
    for (slot, value) in admitted.iter_mut().zip(f64s_at(bytes, offset, N)?) {
        *slot = FiniteReal::new(value)?;
    }
    Some(admitted)
}

/// Read a u32-length-prefixed ASCII string whose length lies in `bounds`,
/// decoding it as strict UTF-8. Returns the string and the offset past it.
pub(crate) fn lp_ascii_strict(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Option<(String, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !bounds.contains(&length) {
        return None;
    }
    let (raw, end) = lp_u32_bytes_at(bytes, at)?;
    Some((std::str::from_utf8(raw).ok()?.to_owned(), end))
}

/// Read a bounded strict-UTF-8 string after admitting its retained bytes.
pub(crate) fn lp_ascii_strict_charged(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Result<Option<(String, usize)>, CodecError> {
    let Some(length_u32) = View::u32_le_at(bytes, at) else {
        return Ok(None);
    };
    let Ok(length) = usize::try_from(length_u32) else {
        return Ok(None);
    };
    if !bounds.contains(&length) {
        return Ok(None);
    }
    let Some((raw, end)) = lp_u32_bytes_at(bytes, at) else {
        return Ok(None);
    };
    ctx.charge_work(u64::from(length_u32), "decode F3D ASCII string")?;
    let Ok(value) = std::str::from_utf8(raw) else {
        return Ok(None);
    };
    ctx.charge_retained(u64::from(length_u32), "retain F3D ASCII string")?;
    let mut owned = String::new();
    owned
        .try_reserve(length)
        .map_err(|_| ctx.refuse_codec_limit("retain F3D ASCII string", 0, u64::from(length_u32)))?;
    owned.push_str(value);
    Ok(Some((owned, end)))
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
    let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
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
    let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !bounds.contains(&count) {
        return None;
    }
    utf16le_at(bytes, at.checked_add(4)?, count)
}

/// Decode a length-prefixed UTF-16 string while admitting its retained UTF-8 bytes.
pub(crate) fn lp_utf16_bounded_charged(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Result<Option<(String, usize)>, CodecError> {
    let Some(count_u32) = View::u32_le_at(bytes, at) else {
        return Ok(None);
    };
    let Ok(count) = usize::try_from(count_u32) else {
        return Ok(None);
    };
    if !bounds.contains(&count) {
        return Ok(None);
    }
    let Some(start) = at.checked_add(4) else {
        return Ok(None);
    };
    let Some(end) = count.checked_mul(2).and_then(|length| start.checked_add(length)) else {
        return Ok(None);
    };
    let Some(raw) = bytes.get(start..end) else {
        return Ok(None);
    };
    let units = || {
        let mut view = View::over_retained(raw);
        std::iter::from_fn(move || view.u16_le())
    };
    ctx.charge_work(u64::from(count_u32) * 2, "decode F3D UTF-16 string")?;
    let mut utf8_len = 0usize;
    for decoded in char::decode_utf16(units()) {
        let Ok(character) = decoded else {
            return Ok(None);
        };
        utf8_len = utf8_len.checked_add(character.len_utf8()).ok_or_else(|| {
            ctx.refuse_codec_limit("decode F3D UTF-16 string", 0, u64::MAX)
        })?;
    }
    let utf8_len_u64 = u64::try_from(utf8_len)
        .map_err(|_| ctx.refuse_codec_limit("decode F3D UTF-16 string", 0, u64::MAX))?;
    ctx.charge_retained(utf8_len_u64, "retain F3D UTF-16 string")?;
    let mut value = String::new();
    value
        .try_reserve(utf8_len)
        .map_err(|_| ctx.refuse_codec_limit("retain F3D UTF-16 string", 0, utf8_len_u64))?;
    for decoded in char::decode_utf16(units()) {
        let Ok(character) = decoded else {
            return Ok(None);
        };
        value.push(character);
    }
    Ok(Some((value, end)))
}

#[cfg(test)]
mod charged_string_tests {
    use super::{lp_ascii_strict_charged, lp_utf16_bounded_charged};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn bounded_utf16_string_refuses_retained_limit() {
        let bytes = [2, 0, 0, 0, b'A', 0, b'B', 0];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let error = lp_utf16_bounded_charged(&ctx, &bytes, 0, 0..=1024).unwrap_err();
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "retain F3D UTF-16 string"
        ));
    }


    #[test]
    fn bounded_ascii_string_refuses_retained_limit() {
        let bytes = [2, 0, 0, 0, b'A', b'B'];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let error = lp_ascii_strict_charged(&ctx, &bytes, 0, 0..=128).unwrap_err();
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "retain F3D ASCII string"
        ));
    }
}

/// Take a u32-length-prefixed strict-UTF-8 string, advancing `at` past it on
/// success.
pub(crate) fn take_lp_utf8(bytes: &[u8], at: &mut usize) -> Option<String> {
    String::from_utf8(take_lp_u32_bytes(bytes, at)?.to_vec()).ok()
}

pub(crate) fn take_lp_utf8_charged(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: &mut usize,
) -> Result<Option<String>, CodecError> {
    let Some(raw) = take_lp_u32_bytes(bytes, at) else {
        return Ok(None);
    };
    let Ok(value) = std::str::from_utf8(raw) else {
        return Ok(None);
    };
    let length = u64::try_from(raw.len())
        .map_err(|_| ctx.refuse_codec_limit("retain F3D UTF-8 string", 0, u64::MAX))?;
    ctx.charge_retained(length, "retain F3D UTF-8 string")?;
    let mut owned = String::new();
    owned
        .try_reserve(raw.len())
        .map_err(|_| ctx.refuse_codec_limit("retain F3D UTF-8 string", 0, length))?;
    owned.push_str(value);
    Ok(Some(owned))
}

/// Advance `at` past a u32-length-prefixed byte string, reading none of it.
///
/// `None` is the refusal a declared length the record cannot carry states.
/// The caller that reads no content states its skip through this, so a
/// truncation moves `at` nowhere and is a refusal rather than a short read
/// every following offset inherits.
pub(crate) fn skip_lp_u32_bytes(bytes: &[u8], at: &mut usize) -> Option<()> {
    let mut view = View::over_retained(bytes);
    view.seek(*at)?;
    let length = usize::try_from(view.u32_le()?).ok()?;
    view.skip(length)?;
    *at = view.position();
    Some(())
}

/// One reference member of a Fusion segment record
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)
/// "**References.**").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reference {
    /// The null reference.
    Null,
    /// A target in the current segment.
    Local {
        target: u64,
        inline_type_guid: Option<String>,
    },
    /// A target in another segment.
    CrossSegment {
        target: u64,
        inline_type_guid: Option<String>,
        segment: u32,
    },
    /// A named target in another document.
    CrossDocument {
        target: u64,
        inline_type_guid: Option<String>,
        segment: u32,
        link_name: String,
    },
}

impl Reference {
    /// The target and inline type of a local reference.
    pub(crate) fn local(&self) -> Option<(u64, Option<&str>)> {
        match self {
            Self::Local {
                target,
                inline_type_guid,
            } => Some((*target, inline_type_guid.as_deref())),
            _ => None,
        }
    }

    /// The owned target and inline type of a local reference.
    pub(crate) fn into_local(self) -> Option<(u64, Option<String>)> {
        match self {
            Self::Local {
                target,
                inline_type_guid,
            } => Some((target, inline_type_guid)),
            _ => None,
        }
    }

    /// The target of a non-null reference.
    pub(crate) fn target(&self) -> Option<u64> {
        match self {
            Self::Null => None,
            Self::Local { target, .. }
            | Self::CrossSegment { target, .. }
            | Self::CrossDocument { target, .. } => Some(*target),
        }
    }

    /// The link name of a cross-document reference.
    pub(crate) fn link_name(&self) -> Option<&str> {
        match self {
            Self::CrossDocument { link_name, .. } => Some(link_name),
            _ => None,
        }
    }
}

/// Take one reference, advancing `at` past every byte it owns.
///
/// The width is not fixed: a null reference is one byte, a same-segment
/// reference eleven, an inline-typed same-segment reference 51, a cross-segment
/// reference fifteen, and a cross-document reference carries an asset GUID, a
/// type GUID, a link name, and an optional version tail. Any arithmetic that
/// assumes one width desynchronizes on the first nonstandard reference.
pub(crate) fn take_reference(bytes: &[u8], at: &mut usize) -> Option<Reference> {
    let mut cursor = *at;
    let present = *bytes.get(cursor)?;
    cursor += 1;
    if present == 0 {
        *at = cursor;
        return Some(Reference::Null);
    }
    if present != 1 {
        return None;
    }
    let target = View::u64_le_at(bytes, cursor)?;
    cursor += 8;
    // One container generation writes the target's type GUID inline, between
    // the entity ID and the `cross_document` flag.
    let inline_type_guid = if View::u32_le_at(bytes, cursor) == Some(36) {
        let (guid, end) = lp_ascii_filtered(bytes, cursor, 36..=36, u8::is_ascii_graphic)?;
        if !is_guid_hyphenated(&guid) {
            return None;
        }
        cursor = end;
        Some(guid)
    } else {
        None
    };
    let reference = match *bytes.get(cursor)? {
        0 => {
            cursor += 1;
            match *bytes.get(cursor)? {
                0 => {
                    cursor += 1;
                    Reference::Local {
                        target,
                        inline_type_guid,
                    }
                }
                1 => {
                    let segment = View::u32_le_at(bytes, cursor + 1)?;
                    cursor += 5;
                    Reference::CrossSegment {
                        target,
                        inline_type_guid,
                        segment,
                    }
                }
                _ => return None,
            }
        }
        1 => {
            cursor += 1;
            let segment = View::u32_le_at(bytes, cursor)?;
            cursor += 4;
            let (_asset, end) = lp_utf16_bounded(bytes, cursor, 0..=64)?;
            cursor = end;
            match *bytes.get(cursor)? {
                1 => {
                    cursor += 1;
                    Reference::CrossSegment {
                        target,
                        inline_type_guid,
                        segment,
                    }
                }
                0 => {
                    cursor += 1;
                    let (guid, end) =
                        lp_ascii_filtered(bytes, cursor, 36..=36, u8::is_ascii_graphic)?;
                    if !is_guid_hyphenated(&guid) {
                        return None;
                    }
                    let (link_name, end) = lp_utf16_bounded(bytes, end, 0..=256)?;
                    cursor = end;
                    match *bytes.get(cursor)? {
                        0 => cursor += 1,
                        1 => {
                            let (_property_key, end) =
                                lp_utf16_bounded(bytes, cursor + 1, 36..=36)?;
                            let (_version_urn, end) = lp_utf16_bounded(bytes, end, 0..=256)?;
                            cursor = end;
                        }
                        _ => return None,
                    }
                    Reference::CrossDocument {
                        target,
                        inline_type_guid,
                        segment,
                        link_name,
                    }
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    *at = cursor;
    Some(reference)
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

#[cfg(test)]
mod tests {
    #[test]
    fn lp_utf8_string_refuses_retained_limit() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"text");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 3;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let error = super::take_lp_utf8_charged(&ctx, &bytes, &mut 0)
            .expect_err("encoded string must exceed retained budget");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D UTF-8 string"));
    }
}
