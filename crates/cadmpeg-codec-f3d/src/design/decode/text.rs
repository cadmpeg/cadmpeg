// SPDX-License-Identifier: Apache-2.0
//! Charged text reads shared by Design record decoders.

use std::ops::RangeInclusive;
use std::fmt::Write;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;

/// Read an ASCII-only length-prefixed field without copying its contents.
pub(in crate::design::decode) fn lp_ascii_filtered_view(
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

/// Validate a borrowed three-digit class tag before making its fixed-size copy.
pub(in crate::design::decode) fn class_tag_from_view(
    value: &str,
) -> Result<crate::records::references::DesignClassTag, String> {
    if value.len() != 3 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("class_tag must contain three ASCII digits".into());
    }
    crate::records::references::DesignClassTag::try_from(value.to_owned())
}

/// Copy an admitted ASCII field into retained text after charging its bytes.
pub(in crate::design::decode) fn copy_ascii_retained(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    String::from_utf8(ctx.copy_retained(value.as_bytes(), operation)?)
        .map_err(|_| CodecError::malformed("F3D ASCII field must be UTF-8"))
}

pub(in crate::design::decode) fn design_record_id_charged(
    ctx: &DecodeContext<'_>,
    stream: &str,
    suffix: &'static str,
    offset: u64,
    charge_operation: &'static str,
    allocation_operation: &'static str,
) -> Result<String, CodecError> {
    let mut id = super::sketch::native_scope_charged(ctx, stream)?;
    let digits = usize::try_from(offset.checked_ilog10().unwrap_or(0) + 1).map_err(|_| {
        ctx.refuse_codec_limit(allocation_operation, 0, 1)
    })?;
    let additional = suffix.len().checked_add(digits).ok_or_else(|| {
        ctx.refuse_codec_limit(allocation_operation, 0, 1)
    })?;
    ctx.charge_retained(
        u64::try_from(additional).map_err(|_| {
            ctx.refuse_codec_limit(allocation_operation, 0, 1)
        })?,
        charge_operation,
    )?;
    id.try_reserve(additional).map_err(|_| {
        ctx.refuse_codec_limit(allocation_operation, 0, 1)
    })?;
    id.push_str(suffix);
    write!(id, "{offset}").map_err(|_| {
        ctx.refuse_codec_limit(allocation_operation, 0, 1)
    })?;
    Ok(id)
}

/// Validate an exact 36-code-unit relaxed GUID in UTF-16LE without copying it.
pub(in crate::design::decode) fn fixed_guid_end(bytes: &[u8], count_at: usize) -> Option<usize> {
    (View::u32_le_at(bytes, count_at)? == 36).then_some(())?;
    let start = count_at.checked_add(4)?;
    let end = start.checked_add(72)?;
    bytes
        .get(start..end)?
        .chunks_exact(2)
        .all(|unit| {
            unit[1] == 0
                && (unit[0].is_ascii_alphanumeric() || matches!(unit[0], b'-' | b'_'))
        })
        .then_some(end)
}

/// Match an ASCII literal encoded as a counted UTF-16LE field without copying it.
pub(in crate::design::decode) fn fixed_utf16_ascii_eq(
    bytes: &[u8],
    count_at: usize,
    expected: &str,
) -> Option<usize> {
    if !expected.is_ascii() || usize::try_from(View::u32_le_at(bytes, count_at)?).ok()? != expected.len() {
        return None;
    }
    let start = count_at.checked_add(4)?;
    let end = start.checked_add(expected.len().checked_mul(2)?)?;
    bytes
        .get(start..end)?
        .chunks_exact(2)
        .zip(expected.bytes())
        .all(|(unit, expected_byte)| unit == [expected_byte, 0])
        .then_some(end)
}

pub(super) fn lp_utf16_bounded_charged(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Result<Option<(String, usize)>, CodecError> {
    let Some(count) = View::u32_le_at(bytes, at).and_then(|count| usize::try_from(count).ok()) else {
        return Ok(None);
    };
    if !bounds.contains(&count) {
        return Ok(None);
    }
    let Some(start) = at.checked_add(4) else {
        return Ok(None);
    };
    let Some(end) = count.checked_mul(2).and_then(|bytes| start.checked_add(bytes)) else {
        return Ok(None);
    };
    let Some(raw) = bytes.get(start..end) else {
        return Ok(None);
    };
    let mut view = View::over_retained(raw);
    let mut utf8_len = 0usize;
    for decoded in std::char::decode_utf16(std::iter::from_fn(|| view.u16_le())) {
        let Ok(character) = decoded else {
            return Ok(None);
        };
        utf8_len = utf8_len.checked_add(character.len_utf8()).ok_or_else(|| {
            ctx.refuse_codec_limit("f3d Design UTF-16 length", 0, 1)
        })?;
    }
    ctx.charge_retained(
        u64::try_from(utf8_len).map_err(|_| {
            ctx.refuse_codec_limit("f3d Design UTF-16 length", 0, 1)
        })?,
        "f3d Design UTF-16 text",
    )?;
    let mut text = String::new();
    text.try_reserve(utf8_len).map_err(|_| {
        ctx.refuse_codec_limit("f3d Design UTF-16 allocation", 0, 1)
    })?;
    let mut view = View::over_retained(raw);
    for decoded in std::char::decode_utf16(std::iter::from_fn(|| view.u16_le())) {
        let Ok(character) = decoded else {
            return Ok(None);
        };
        text.push(character);
    }
    Ok(Some((text, end)))
}

pub(super) fn lp_utf16_bounded_scoped<'a>(
    ctx: &'a DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Result<Option<(String, usize, ScopedReservation<'a>)>, CodecError> {
    let Some(count) = View::u32_le_at(bytes, at).and_then(|count| usize::try_from(count).ok()) else {
        return Ok(None);
    };
    if !bounds.contains(&count) {
        return Ok(None);
    }
    let Some(start) = at.checked_add(4) else {
        return Ok(None);
    };
    let Some(end) = count.checked_mul(2).and_then(|width| start.checked_add(width)) else {
        return Ok(None);
    };
    let Some(raw) = bytes.get(start..end) else {
        return Ok(None);
    };
    let mut view = View::over_retained(raw);
    let mut utf8_len = 0usize;
    for decoded in std::char::decode_utf16(std::iter::from_fn(|| view.u16_le())) {
        let Ok(character) = decoded else {
            return Ok(None);
        };
        utf8_len = utf8_len.checked_add(character.len_utf8()).ok_or_else(|| {
            ctx.refuse_codec_limit("f3d Design temporary UTF-16 length", 0, 1)
        })?;
    }
    let reservation = ctx.reserve_scoped(
        u64::try_from(utf8_len).map_err(|_| {
            ctx.refuse_codec_limit("f3d Design temporary UTF-16 length", 0, 1)
        })?,
        "f3d Design temporary UTF-16 text",
    )?;
    let mut text = String::new();
    text.try_reserve(utf8_len).map_err(|_| {
        ctx.refuse_codec_limit("f3d Design temporary UTF-16 allocation", 0, 1)
    })?;
    let mut view = View::over_retained(raw);
    for decoded in std::char::decode_utf16(std::iter::from_fn(|| view.u16_le())) {
        let Ok(character) = decoded else {
            return Ok(None);
        };
        text.push(character);
    }
    Ok(Some((text, end, reservation)))
}

#[cfg(test)]
mod tests {
    use super::{class_tag_from_view, fixed_utf16_ascii_eq, lp_ascii_filtered_view};

    #[test]
    fn fixed_utf16_ascii_match_agrees_with_decoded_text() {
        for (value, expected) in [
            ("Thicken", "Thicken"),
            ("Thicken", "Shell"),
            ("Shell", "Shell"),
            ("Thickén", "Thicken"),
        ] {
            let mut bytes = u32::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes()
                .to_vec();
            for unit in value.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            let decoded = crate::bytes::lp_utf16_bounded(&bytes, 0, expected.len()..=expected.len())
                .and_then(|(text, end)| (text == expected).then_some(end));
            assert_eq!(fixed_utf16_ascii_eq(&bytes, 0, expected), decoded);
            bytes.pop();
            assert_eq!(fixed_utf16_ascii_eq(&bytes, 0, expected), None);
        }
    }

    #[test]
    fn borrowed_ascii_reader_matches_owned_reader() {
        let fields: [&[u8]; 6] = [b"", b"123", b"EntityGenesis", b"a-b_", b"\0", b"\x80"];
        for field in fields {
            let mut bytes = u32::try_from(field.len()).unwrap().to_le_bytes().to_vec();
            bytes.extend_from_slice(field);
            for bounds in [0..=2000, 3..=3] {
                for allowed in [
                    u8::is_ascii_graphic as fn(&u8) -> bool,
                    u8::is_ascii_digit as fn(&u8) -> bool,
                ] {
                    let original =
                        crate::bytes::lp_ascii_filtered(&bytes, 0, bounds.clone(), allowed);
                    let borrowed = lp_ascii_filtered_view(&bytes, 0, bounds.clone(), allowed)
                        .map(|(value, end)| (value.to_owned(), end));
                    assert_eq!(borrowed, original);
                    bytes.pop();
                    assert_eq!(lp_ascii_filtered_view(&bytes, 0, bounds.clone(), allowed), None);
                    bytes.push(*field.last().unwrap_or(&0));
                }
            }
        }
    }

    #[test]
    fn borrowed_class_tag_conversion_matches_owned_conversion() {
        for value in ["123", "000", "12", "1234", "12a", "éé"] {
            assert_eq!(
                class_tag_from_view(value),
                crate::records::references::DesignClassTag::try_from(value.to_owned())
            );
        }
    }
}
