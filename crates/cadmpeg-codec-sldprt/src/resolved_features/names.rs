//! Object name and class declaration records.

use super::{is_class_token, CLASS_MARKER, NAME_MARKER};
use crate::records::ObjectId;
use crate::records::{FeatureInputClass, FeatureInputName, FeatureInputOperandKind};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::text::NonBlankString;

fn retained_text(
    ctx: &DecodeContext<'_>,
    text: &str,
    operation: &'static str,
) -> Result<String, cadmpeg_core::CodecError> {
    let mut retained = String::new();
    ctx.reserve_retained_string(&mut retained, text.len(), operation)?;
    retained.push_str(text);
    Ok(retained)
}

fn record_id(
    ctx: &DecodeContext<'_>,
    family: &'static str,
    lane_key: &str,
    offset: usize,
) -> Result<String, cadmpeg_core::CodecError> {
    let digits = if offset == 0 { 1 } else { offset.ilog10() as usize + 1 };
    let length = "sldprt:feature-input:".len()
        .checked_add(family.len())
        .and_then(|length| length.checked_add(1))
        .and_then(|length| length.checked_add(lane_key.len()))
        .and_then(|length| length.checked_add(1))
        .and_then(|length| length.checked_add(digits))
        .ok_or_else(|| ctx.refuse_codec_limit("retain SLDPRT feature input record ID", u64::MAX - 1, u64::MAX))?;
    let mut id = String::new();
    ctx.reserve_retained_string(&mut id, length, "retain SLDPRT feature input record ID")?;
    std::fmt::Write::write_fmt(
        &mut id,
        format_args!("sldprt:feature-input:{family}#{lane_key}:{offset}"),
    )
    .map_err(|_| cadmpeg_core::CodecError::malformed("cannot format SLDPRT feature input record ID"))?;
    Ok(id)
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
    for (ordinal, (offset, object_id, units)) in payload_names(payload).enumerate() {
        let mut length = 0usize;
        let mut valid = true;
        for character in std::char::decode_utf16(utf16_units(units)) {
            let Ok(character) = character else { valid = false; break; };
            let Some(next) = length.checked_add(character.len_utf8()) else {
                return Err(ctx.refuse_codec_limit("retain SLDPRT feature input name", u64::MAX - 1, u64::MAX));
            };
            length = next;
        }
        if !valid { continue; }
        let mut value = String::new();
        ctx.reserve_retained_string(&mut value, length, "retain SLDPRT feature input name")?;
        for character in std::char::decode_utf16(utf16_units(units)) {
            let Ok(character) = character else { valid = false; break; };
            value.push(character);
        }
        if !valid { continue; }
        let id = record_id(ctx, "name", lane_key, offset)?;
        let parent = retained_text(ctx, parent, "retain SLDPRT feature input name parent")?;
        ctx.reserve_collection_vec(&mut names, 1, "collect SLDPRT feature input names")?;
        names.push(FeatureInputName {
            id,
            parent,
            ordinal: ordinal as u32,
            offset: offset as u64,
            object_id,
            value,
        });
    }
    Ok(names)
}

fn decimal_matches(text: &str, value: usize) -> bool {
    !text.is_empty()
        && (text == "0" || !text.starts_with('0'))
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && text.parse::<usize>() == Ok(value)
}

fn native_id_matches(id: &str, family: &str, lane_key: &str, offset: usize) -> bool {
    id.strip_prefix(family)
        .and_then(|tail| tail.strip_prefix(lane_key))
        .and_then(|tail| tail.strip_prefix(':'))
        .is_some_and(|tail| decimal_matches(tail, offset))
}

pub(crate) fn utf16_units(units: &[u8]) -> impl Iterator<Item = u16> + '_ {
    (0..units.len() / 2).filter_map(|index| View::u16_le_at(units, index * 2))
}

fn payload_names(payload: &[u8]) -> impl Iterator<Item = (usize, Option<ObjectId>, &[u8])> {
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
            let length = usize::from(*payload.get(offset + NAME_MARKER.len())?);
            if !(1..=128).contains(&length) {
                return None;
            }
            let start = offset + NAME_MARKER.len() + 1;
            let end = start.checked_add(length.checked_mul(2)?)?;
            let units = payload.get(start..end)?;
            let value = std::char::decode_utf16(utf16_units(units));
            if value.into_iter().any(|character| match character {
                Ok(character) => character.is_control(),
                Err(_) => true,
            }) {
                return None;
            }
            let object_id = end
                .checked_add(8)
                .and_then(|position| View::u32_le_at(payload, position))
                .and_then(|value| ObjectId::try_from(value).ok());
            Some((offset, object_id, units))
        })
}

fn payload_classes(payload: &[u8]) -> impl Iterator<Item = (usize, &str)> {
    payload
        .windows(CLASS_MARKER.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == CLASS_MARKER).then_some(offset))
        .filter_map(|offset| {
            let length = usize::from(View::u16_le_at(payload, offset + 4)?);
            if !(1..=128).contains(&length) {
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

pub(crate) fn class_declarations_match(
    payload: &[u8],
    parent: &str,
    classes: &[FeatureInputClass],
) -> bool {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    let mut expected = payload_classes(payload).enumerate();
    let matches = classes.iter().enumerate().all(|(ordinal, actual)| {
        expected.next().is_some_and(|(index, (offset, name))| {
            index == ordinal
                && native_id_matches(&actual.id, "sldprt:feature-input:class#", lane_key, offset)
                && actual.parent == parent
                && u32::try_from(ordinal) == Ok(actual.ordinal)
                && u64::try_from(offset) == Ok(actual.offset)
                && actual.name == name
        })
    });
    matches && expected.next().is_none()
}

pub(crate) fn object_names_structure_match(
    payload: &[u8],
    parent: &str,
    names: &[FeatureInputName],
) -> bool {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    let mut expected = payload_names(payload).enumerate();
    let matches = names.iter().enumerate().all(|(ordinal, actual)| {
        expected
            .next()
            .is_some_and(|(index, (offset, object_id, _))| {
                index == ordinal
                    && native_id_matches(&actual.id, "sldprt:feature-input:name#", lane_key, offset)
                    && actual.parent == parent
                    && u32::try_from(ordinal) == Ok(actual.ordinal)
                    && u64::try_from(offset) == Ok(actual.offset)
                    && actual.object_id == object_id
            })
    });
    matches && expected.next().is_none()
}

pub(crate) fn first_object_name_value_mismatch<'a>(
    payload: &'a [u8],
    names: &'a [FeatureInputName],
) -> Option<(usize, &'a FeatureInputName, &'a [u8])> {
    names
        .iter()
        .zip(payload_names(payload))
        .enumerate()
        .find_map(|(index, (actual, (_, _, units)))| {
            let expected = std::char::decode_utf16(utf16_units(units));
            if actual.value.chars().eq(expected.filter_map(Result::ok)) {
                None
            } else {
                Some((index, actual, units))
            }
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
            if !(1..=128).contains(&length) {
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
            (1..=128).contains(&units).then_some(token)
        })
}

pub(crate) fn class_declarations(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    parent: &str,
) -> Result<Vec<FeatureInputClass>, cadmpeg_core::CodecError> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    let mut classes = Vec::new();
    for (ordinal, (offset, name)) in payload_classes(payload).enumerate() {
        let id = record_id(ctx, "class", lane_key, offset)?;
        let parent = retained_text(ctx, parent, "retain SLDPRT feature input class parent")?;
        let name = retained_text(ctx, name, "retain SLDPRT feature input class name")?;
        ctx.reserve_collection_vec(&mut classes, 1, "collect SLDPRT feature input classes")?;
        classes.push(FeatureInputClass {
            id,
            parent,
            ordinal: ordinal as u32,
            offset: offset as u64,
            name,
        });
    }
    Ok(classes)
}

pub(super) fn configuration(
    ctx: &DecodeContext<'_>,
    section: &str,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    let Some(start) = section.find("Config-") else { return Ok(None); };
    let start = start + "Config-".len();
    let tail = &section[start..];
    let end = tail
        .find("-ResolvedFeatures")
        .or_else(|| tail.find('/'))
        .unwrap_or(tail.len());
    if tail[..end].is_empty() {
        return Ok(None);
    }
    Ok(Some(retained_text(ctx, &tail[..end], "retain SLDPRT feature input configuration")?))
}

#[cfg(test)]
mod names_tests;
