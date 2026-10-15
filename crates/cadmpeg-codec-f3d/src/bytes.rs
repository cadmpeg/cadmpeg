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

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

pub(crate) mod utf16;
use utf16::Utf16View;

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

/// Read a fixed little-endian `f64` lane into stack storage.
pub(crate) fn f64s_at<const N: usize>(bytes: &[u8], offset: usize) -> Option<[f64; N]> {
    let mut view = View::over_retained(bytes);
    view.seek(offset)?;
    let mut values = [0.0; N];
    for value in &mut values {
        *value = view.f64_le()?;
    }
    Some(values)
}

/// Read `N` consecutive little-endian `f64` values at `offset`, or `None`
/// when one is not finite.
pub(crate) fn finite_reals_at<const N: usize>(
    bytes: &[u8],
    offset: usize,
) -> Option<[FiniteReal; N]> {
    let mut admitted = [FiniteReal::ZERO; N];
    for (slot, value) in admitted.iter_mut().zip(f64s_at::<N>(bytes, offset)?) {
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
) -> Option<(&str, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !bounds.contains(&length) {
        return None;
    }
    let (raw, end) = lp_u32_bytes_at(bytes, at)?;
    Some((std::str::from_utf8(raw).ok()?, end))
}

/// Read a bounded strict-UTF-8 string and copy it.
pub(crate) fn lp_ascii_strict_charged(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Option<(String, usize)> {
    lp_ascii_strict(bytes, at, bounds).map(|(text, end)| (text.to_owned(), end))
}

/// Read an ASCII-only length-prefixed field without copying its contents.
pub(crate) fn lp_ascii_filtered_view(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
    allowed: fn(&u8) -> bool,
) -> Option<(&str, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !bounds.contains(&length) {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(length)?;
    let raw = bytes.get(start..end)?;
    if !raw.iter().all(allowed) {
        return None;
    }
    Some((std::str::from_utf8(raw).ok()?, end))
}

/// Read the byte span of a bounded counted UTF-16 field.
fn lp_utf16_raw(bytes: &[u8], at: usize, bounds: RangeInclusive<usize>) -> Option<(&[u8], usize)> {
    let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !bounds.contains(&count) {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(count.checked_mul(2)?)?;
    Some((bytes.get(start..end)?, end))
}

/// Read validated UTF-16 text without allocating a decoded copy.
pub(crate) fn lp_utf16_bounded_view(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Option<(Utf16View<'_>, usize)> {
    let (raw, end) = lp_utf16_raw(bytes, at, bounds)?;
    Some((Utf16View::new(raw)?, end))
}

/// Decode a counted UTF-16 field into an owned string.
pub(crate) fn lp_utf16_bounded(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Option<(String, usize)> {
    let (text, end) = lp_utf16_bounded_view(bytes, at, bounds)?;
    Some((text.to_string(), end))
}

/// Take a u32-length-prefixed strict-UTF-8 string, advancing `at` past it on
/// success.
pub(crate) fn take_lp_utf8(bytes: &[u8], at: &mut usize) -> Option<String> {
    String::from_utf8(take_lp_u32_bytes(bytes, at)?.to_vec()).ok()
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
pub(crate) enum Reference<G, L> {
    /// The null reference.
    Null,
    /// A target in the current segment.
    Local {
        target: u64,
        inline_type_guid: Option<G>,
    },
    /// A target in another segment.
    CrossSegment {
        target: u64,
        inline_type_guid: Option<G>,
        segment: u32,
    },
    /// A named target in another document.
    CrossDocument {
        target: u64,
        inline_type_guid: Option<G>,
        segment: u32,
        link_name: L,
    },
}

impl<G: AsRef<str>, L> Reference<G, L> {
    /// The target and inline type of a local reference.
    pub(crate) fn local(&self) -> Option<(u64, Option<&str>)> {
        match self {
            Self::Local {
                target,
                inline_type_guid,
            } => Some((*target, inline_type_guid.as_ref().map(AsRef::as_ref))),
            _ => None,
        }
    }

    /// Consume a local reference and return its target and inline type.
    pub(crate) fn into_local(self) -> Option<(u64, Option<G>)> {
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
}

impl Reference<String, String> {
    /// The link name of a cross-document reference.
    pub(crate) fn link_name(&self) -> Option<&str> {
        match self {
            Self::CrossDocument { link_name, .. } => Some(link_name),
            _ => None,
        }
    }
}

/// Take one reference and copy the text it keeps.
pub(crate) fn take_reference_charged(
    bytes: &[u8],
    at: &mut usize,
) -> Option<Reference<String, String>> {
    let reference = take_reference(bytes, at)?;
    let copy_guid = |guid: Option<&str>| guid.map(str::to_owned);
    Some(match reference {
        Reference::Null => Reference::Null,
        Reference::Local {
            target,
            inline_type_guid,
        } => Reference::Local {
            target,
            inline_type_guid: copy_guid(inline_type_guid),
        },
        Reference::CrossSegment {
            target,
            inline_type_guid,
            segment,
        } => Reference::CrossSegment {
            target,
            inline_type_guid: copy_guid(inline_type_guid),
            segment,
        },
        Reference::CrossDocument {
            target,
            inline_type_guid,
            segment,
            link_name,
        } => Reference::CrossDocument {
            target,
            inline_type_guid: copy_guid(inline_type_guid),
            segment,
            link_name: link_name.to_string(),
        },
    })
}

/// Take one reference, advancing `at` past every byte it owns.
///
/// The width is not fixed: a null reference is one byte, a same-segment
/// reference eleven, an inline-typed same-segment reference 51, a cross-segment
/// reference fifteen, and a cross-document reference carries an asset GUID, a
/// type GUID, a link name, and an optional version tail. Any arithmetic that
/// assumes one width desynchronizes on the first nonstandard reference.
pub(crate) fn take_reference<'a>(
    bytes: &'a [u8],
    at: &mut usize,
) -> Option<Reference<&'a str, Utf16View<'a>>> {
    let utf16 = |at, bounds| -> Option<(Utf16View<'a>, usize)> {
        let (raw, end) = lp_utf16_raw(bytes, at, bounds)?;
        Utf16View::new(raw).map(|text| (text, end))
    };
    let ascii = |at, bounds: RangeInclusive<usize>| -> Option<(&'a str, usize)> {
        let (raw, end) = lp_u32_bytes_at(bytes, at)?;
        if !RangeInclusive::contains(&bounds, &raw.len()) {
            return None;
        }
        std::str::from_utf8(raw).ok().map(|text| (text, end))
    };
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
    let inline_type_guid = if View::u32_le_at(bytes, cursor) == Some(36) {
        let (guid, end) = ascii(cursor, 36..=36)?;
        if !is_guid_hyphenated(guid) {
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
            let (_, end) = utf16(cursor, 0..=64)?;
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
                    let (guid, end) = ascii(cursor, 36..=36)?;
                    if !is_guid_hyphenated(guid) {
                        return None;
                    }
                    let (link_name, end) = utf16(end, 0..=256)?;
                    cursor = end;
                    match *bytes.get(cursor)? {
                        0 => cursor += 1,
                        1 => {
                            let (_, end) = utf16(cursor + 1, 36..=36)?;
                            let (_, end) = utf16(end, 0..=256)?;
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
pub(crate) fn lp_utf16_bytes(value: &str) -> Result<Vec<u8>, CodecError> {
    let units: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let count = u32::try_from(units.len() / 2)
        .map_err(|_| CodecError::malformed("f3d UTF-16 code-unit count exceeds u32"))?;
    let mut out = count.to_le_bytes().to_vec();
    out.extend(units);
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixed_real_lanes_read_finite_values() {
        let values = [1.0_f64, -2.0, 3.0];
        let bytes: Vec<_> = values.into_iter().flat_map(f64::to_le_bytes).collect();
        assert_eq!(super::f64s_at::<3>(&bytes, 0), Some(values));
        assert_eq!(super::f64s_at::<3>(&bytes[..23], 0), None);
        assert_eq!(super::f64s_at::<3>(&bytes, usize::MAX), None);
        assert_eq!(
            super::finite_reals_at::<3>(&bytes, 0)
                .unwrap()
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            values
        );
        let bytes = f64::NAN.to_le_bytes();
        assert_eq!(super::finite_reals_at::<1>(&bytes, 0), None);
    }
    #[test]
    fn borrowed_text_readers_preserve_boundaries_and_unicode() {
        let ascii = [3, 0, 0, 0, b'1', b'2', b'3'];
        for (text, end) in [
            super::lp_ascii_strict(&ascii, 0, 3..=3).unwrap(),
            super::lp_ascii_filtered_view(&ascii, 0, 3..=3, u8::is_ascii_digit).unwrap(),
        ] {
            assert_eq!(text, "123");
            assert_eq!(end, ascii.len());
            assert_eq!(text.as_ptr(), ascii[4..].as_ptr());
        }
        assert!(super::lp_ascii_strict(&ascii[..6], 0, 3..=3).is_none());
        assert!(
            super::lp_ascii_filtered_view(&ascii, usize::MAX, 3..=3, u8::is_ascii_digit).is_none()
        );
        let mut bytes = Vec::new();
        crate::test_support::lp_utf16(&mut bytes, " A😀 ");
        let (text, end) = super::lp_utf16_bounded_view(&bytes, 0, 0..=20).unwrap();
        assert!(text.eq_str(" A😀 "));
        assert_eq!(text.len(), " A😀 ".len());
        assert_eq!(end, bytes.len());
        assert!(super::lp_utf16_bounded_view(&bytes[..bytes.len() - 1], 0, 0..=20).is_none());
        assert!(super::lp_utf16_bounded_view(&bytes, usize::MAX, 0..=20).is_none());
        assert!(super::lp_utf16_bounded_view(&[1, 0, 0, 0, 0, 216], 0, 0..=20).is_none());
    }

    #[test]
    fn charged_reference_keeps_link_name_and_cursor() {
        let mut bytes = crate::test_support::cross_document_reference(7, "link");
        *bytes.last_mut().unwrap() = 1;
        crate::test_support::lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
        crate::test_support::lp_utf16(&mut bytes, "urn:version:discarded");
        let mut at = 0;
        let reference = super::take_reference_charged(&bytes, &mut at).unwrap();
        assert_eq!(reference.link_name(), Some("link"));
        assert_eq!(reference.target(), Some(7));
        assert_eq!(at, bytes.len());
        let mut cross_segment = vec![1];
        cross_segment.extend_from_slice(&7_u64.to_le_bytes());
        cross_segment.push(1);
        cross_segment.extend_from_slice(&0_u32.to_le_bytes());
        crate::test_support::lp_utf16(&mut cross_segment, "11111111-2222-3333-4444-555555555555");
        cross_segment.push(1);
        let mut at = 0;
        let reference = super::take_reference_charged(&cross_segment, &mut at).unwrap();
        assert!(matches!(
            reference,
            super::Reference::CrossSegment {
                target: 7,
                segment: 0,
                inline_type_guid: None
            }
        ));
        assert_eq!(at, cross_segment.len());
    }

    #[test]
    fn reference_inline_guid_probe_borrows_source_text() {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&7_u64.to_le_bytes());
        bytes.extend_from_slice(&36_u32.to_le_bytes());
        bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        bytes.extend_from_slice(&[0, 0]);
        let mut at = 0;
        let reference = super::take_reference(&bytes, &mut at).unwrap();
        let (target, guid) = reference.local().unwrap();
        assert_eq!(target, 7);
        assert_eq!(guid.unwrap().as_ptr(), bytes[13..].as_ptr());
        assert_eq!(at, bytes.len());
    }
}
