//! Object name and class declaration records.

use super::{CLASS_MARKER, NAME_MARKER};
use crate::classification::native_object_class;
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputName, FeatureInputOperandKind,
};

pub(super) fn operand_kind_name(kind: FeatureInputOperandKind) -> String {
    match kind {
        FeatureInputOperandKind::D6 => "d6".into(),
        FeatureInputOperandKind::E1 => "e1".into(),
        FeatureInputOperandKind::Native(tag) => {
            let [first, second] = tag.to_le_bytes();
            format!("{first:02x}{second:02x}")
        }
    }
}

pub(crate) fn object_names(payload: &[u8], parent: &str) -> Vec<FeatureInputName> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    let mut name_marker = NAME_MARKER.to_vec();
    if let Some(token) = name_class_token(payload) {
        name_marker[..2].copy_from_slice(&token.to_le_bytes());
    }
    payload
        .windows(name_marker.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == name_marker).then_some(offset))
        .filter_map(|offset| {
            let length = usize::from(*payload.get(offset + NAME_MARKER.len())?);
            if !(1..=128).contains(&length) {
                return None;
            }
            let start = offset + NAME_MARKER.len() + 1;
            let end = start.checked_add(length.checked_mul(2)?)?;
            let units = payload
                .get(start..end)?
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>();
            let value = String::from_utf16(&units).ok()?;
            let object_id = end.checked_add(8).and_then(|offset| {
                Some(u32::from_le_bytes(
                    payload.get(offset..offset + 4)?.try_into().ok()?,
                ))
            });
            (!value.chars().any(char::is_control)).then_some((offset, object_id, value))
        })
        .enumerate()
        .map(|(ordinal, (offset, object_id, value))| FeatureInputName {
            id: format!("sldprt:feature-input:name#{lane_key}:{offset}"),
            parent: parent.to_string(),
            ordinal: ordinal as u32,
            offset: offset as u64,
            object_id,
            value,
        })
        .collect()
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
            let length = usize::from(u16::from_le_bytes(
                payload.get(offset + 4..offset + 6)?.try_into().ok()?,
            ));
            if !(1..=128).contains(&length) {
                return None;
            }
            let name = payload.get(offset + 6..offset + 6 + length)?;
            if !name.iter().all(u8::is_ascii_graphic) {
                return None;
            }
            let token_offset = offset + 6 + length;
            let token = u16::from_le_bytes(
                payload
                    .get(token_offset..token_offset + 2)?
                    .try_into()
                    .ok()?,
            );
            if token & 0x8000 == 0 || token == 0xffff {
                return None;
            }
            if payload.get(token_offset + 2..token_offset + 5) != Some(&[0xff, 0xfe, 0xff]) {
                return None;
            }
            let units = usize::from(*payload.get(token_offset + 5)?);
            (1..=128).contains(&units).then_some(token)
        })
}

pub(crate) fn class_declarations(payload: &[u8], parent: &str) -> Vec<FeatureInputClass> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    payload
        .windows(CLASS_MARKER.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == CLASS_MARKER).then_some(offset))
        .filter_map(|offset| {
            let length = usize::from(u16::from_le_bytes(
                payload.get(offset + 4..offset + 6)?.try_into().ok()?,
            ));
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
            Some((offset, std::str::from_utf8(bytes).ok()?.to_string()))
        })
        .enumerate()
        .map(|(ordinal, (offset, name))| {
            let role = class_role(&name);
            FeatureInputClass {
                id: format!("sldprt:feature-input:class#{lane_key}:{offset}"),
                parent: parent.to_string(),
                ordinal: ordinal as u32,
                offset: offset as u64,
                name,
                role,
            }
        })
        .collect()
}

fn class_role(name: &str) -> FeatureInputClassRole {
    native_object_class(name).role
}

pub(super) fn configuration(section: &str) -> Option<String> {
    let start = section.find("Config-")? + "Config-".len();
    let tail = &section[start..];
    let end = tail
        .find("-ResolvedFeatures")
        .or_else(|| tail.find('/'))
        .unwrap_or(tail.len());
    (!tail[..end].is_empty()).then(|| tail[..end].to_string())
}
