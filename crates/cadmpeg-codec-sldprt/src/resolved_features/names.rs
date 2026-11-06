//! Object name and class declaration records.

use super::{is_class_token, CLASS_MARKER, NAME_MARKER};
use crate::records::ObjectId;
use crate::records::{FeatureInputClass, FeatureInputName, FeatureInputOperandKind};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::text::NonBlankString;

fn record_id(family: &'static str, lane_key: &str, offset: usize) -> String {
    format!("sldprt:feature-input:{family}#{lane_key}:{offset}")
}

pub(super) fn operand_kind_name(kind: FeatureInputOperandKind) -> NonBlankString {
    match kind {
        FeatureInputOperandKind::D6 => cadmpeg_core::nonblank_literal!("d6"),
        FeatureInputOperandKind::E1 => cadmpeg_core::nonblank_literal!("e1"),
        FeatureInputOperandKind::Native(tag) => {
            let [first, second] = tag.value().to_le_bytes();
            NonBlankString::prefixed(
                cadmpeg_core::text::NonWhitespaceChar::hex_digit(first >> 4),
                format_args!("{:x}{second:02x}", first & 0x0f),
            )
        }
    }
}

pub(crate) fn object_names(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    parent: &str,
) -> Result<Vec<FeatureInputName>, cadmpeg_core::CodecError> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    let mut names = Vec::new();
    for (offset, object_id, units) in payload_name_candidates(payload) {
        let decoded = match ctx.utf16le_text(
            units,
            units.len() / 2,
            false,
            "decode SLDPRT feature input name",
        ) {
            Ok(text) => text,
            Err(cadmpeg_core::CodecError::Malformed(_)) => continue,
            Err(error) => return Err(error),
        };
        if decoded.chars().any(char::is_control) {
            continue;
        }
        let ordinal = u32::try_from(names.len()).map_err(|_| {
            cadmpeg_core::CodecError::malformed("SLDPRT feature input name ordinal overflows u32")
        })?;
        names.push(FeatureInputName {
            feature_ref: None,
            id: record_id("name", lane_key, offset),
            parent: parent.to_owned(),
            ordinal,
            sequence: ordinal,
            offset: u64_from_index(offset),
            object_id,
            value: decoded,
        });
    }
    Ok(names)
}

fn payload_name_candidates(
    payload: &[u8],
) -> impl Iterator<Item = (usize, Option<ObjectId>, &[u8])> {
    let mut name_marker = [0; 5];
    name_marker.copy_from_slice(NAME_MARKER);
    if let Some(token) = name_class_token(payload) {
        name_marker[..2].copy_from_slice(&token.to_le_bytes());
    }
    payload
        .windows(name_marker.len())
        .enumerate()
        .filter_map(move |(offset, marker)| (marker == name_marker).then_some(offset))
        .filter_map(move |offset| {
            let (start, end) = name_span(payload, offset)?;
            let units = payload.get(start..end)?;
            let object_id = end
                .checked_add(8)
                .and_then(|position| View::u32_le_at(payload, position))
                .and_then(|value| ObjectId::try_from(value).ok());
            Some((offset, object_id, units))
        })
}

pub(crate) fn name_span(payload: &[u8], offset: usize) -> Option<(usize, usize)> {
    let length_at = offset.checked_add(NAME_MARKER.len())?;
    let first = *payload.get(length_at)?;
    let (length, start) = if first == u8::MAX {
        (
            usize::from(View::u16_le_at(payload, length_at.checked_add(1)?)?),
            length_at.checked_add(3)?,
        )
    } else {
        (usize::from(first), length_at.checked_add(1)?)
    };
    if length == 0 {
        return None;
    }
    let end = start.checked_add(length.checked_mul(2)?)?;
    payload.get(start..end)?;
    Some((start, end))
}

pub(crate) fn record_name_end(payload: &[u8], name: &FeatureInputName) -> Option<usize> {
    let offset = usize::try_from(name.offset).ok()?;
    if let Some((start, end)) = name_span(payload, offset) {
        let units = name.value.encode_utf16();
        if units
            .flat_map(u16::to_le_bytes)
            .eq(payload[start..end].iter().copied())
        {
            return Some(end);
        }
    }
    let units = name.value.encode_utf16().count();
    offset
        .checked_add(name_prefix_len(units))?
        .checked_add(units.checked_mul(2)?)
}

pub(crate) fn append_name_length(payload: &mut Vec<u8>, length: u8) {
    payload.push(length);
    if length == u8::MAX {
        payload.extend_from_slice(&u16::from(length).to_le_bytes());
    }
}

pub(crate) fn name_prefix_len(units: usize) -> usize {
    NAME_MARKER.len() + 1 + usize::from(units >= usize::from(u8::MAX)) * 2
}

pub(super) fn payload_classes(payload: &[u8]) -> impl Iterator<Item = (usize, &str)> {
    payload
        .windows(CLASS_MARKER.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == CLASS_MARKER).then_some(offset))
        .filter_map(|offset| {
            let length = usize::from(View::u16_le_at(payload, offset + 4)?);
            if length == 0 {
                return None;
            }
            let bytes = payload.get(offset + 6..offset + 6 + length)?;
            if !bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return None;
            }
            Some((offset, std::str::from_utf8(bytes).ok()?))
        })
}

/// Lane-scoped repeated-class token carried by every feature-name record.
///
/// The token is established by the first name record in the lane: the first
/// class declaration directly followed by a repeated-class token and the
/// UTF-16 name prefix `ff fe ff`.
fn name_class_token(payload: &[u8]) -> Option<u16> {
    payload
        .windows(CLASS_MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == CLASS_MARKER)
        .find_map(|(offset, _)| {
            let length = usize::from(View::u16_le_at(payload, offset + 4)?);
            if length == 0 {
                return None;
            }
            let name = payload.get(offset + 6..offset + 6 + length)?;
            if !name.iter().all(u8::is_ascii_graphic) {
                return None;
            }
            let token_offset = offset + 6 + length;
            let token = View::u16_le_at(payload, token_offset)?;
            if !is_class_token(token) {
                return None;
            }
            if payload.get(token_offset + 2..token_offset + 5) != Some(&[0xff, 0xfe, 0xff]) {
                return None;
            }
            let units = usize::from(*payload.get(token_offset + 5)?);
            (units != 0).then_some(token)
        })
}

pub(crate) fn class_declarations(
    payload: &[u8],
    parent: &str,
) -> Result<Vec<FeatureInputClass>, cadmpeg_core::CodecError> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    let mut classes = Vec::new();
    for (ordinal, (offset, name)) in payload_classes(payload).enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            cadmpeg_core::CodecError::malformed("SLDPRT feature input class ordinal overflows u32")
        })?;
        classes.push(FeatureInputClass {
            feature_ref: None,
            id: record_id("class", lane_key, offset),
            parent: parent.to_owned(),
            ordinal,
            sequence: ordinal,
            offset: u64_from_index(offset),
            name: name.to_owned(),
        });
    }
    Ok(classes)
}

pub(super) fn configuration(section: &str) -> Option<String> {
    let start = section.find("Config-")? + "Config-".len();
    let tail = &section[start..];
    let end = tail
        .find("-ResolvedFeatures")
        .or_else(|| tail.find('/'))
        .unwrap_or(tail.len());
    if tail[..end].is_empty() {
        return None;
    }
    Some(tail[..end].to_owned())
}

#[cfg(test)]
mod names_tests;
