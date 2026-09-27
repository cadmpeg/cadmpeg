// SPDX-License-Identifier: Apache-2.0
//! Charged text reads shared by Design record decoders.

use std::ops::RangeInclusive;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

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
