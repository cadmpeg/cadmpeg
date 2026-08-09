// SPDX-License-Identifier: Apache-2.0
//! Parse typed Design body-presentation and browser-node records.

use std::collections::HashMap;

use cadmpeg_core::le::{u32_at, u64_at};

use crate::bytes::{is_guid_prefix, lp_utf16_bounded, lp_utf16_bytes, take_reference};
use crate::design::decode::sketch::{
    indexed_record_offsets, parse_genesis_entity_header, parse_settled_entity_header,
};
use crate::design::presentation::{
    is_physical_material_token, APPEARANCE_LIBRARY_ID, BODY_PRESENTATION_TYPE_GUID,
    BODY_PRESENTATION_TYPE_VERSION, BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION,
    BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION, BROWSER_NODE_TYPE_GUID,
    BROWSER_NODE_TYPE_VERSION, MODERN_APPEARANCE_LIBRARY_IDS, PHYSICAL_MATERIAL_LIBRARY_ID,
};

const GUID_LEN: usize = 36;
const MAX_ENVELOPE_GAP: usize = 8;

/// One typed browser-node record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserNodeRecord {
    pub record_index: u32,
    pub guid: String,
    pub entity_suffix: u64,
    pub hidden_offset: u64,
    pub hidden: bool,
}

/// Material members of one body-presentation envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PresentationMaterial {
    pub node_guid: String,
    pub physical_token: String,
    pub physical_token_offset: u64,
    pub visual_guid: String,
    pub visual_guid_offset: u64,
    pub visual_preset: Option<String>,
    pub visual_preset_offset: Option<u64>,
}

/// One body record, its exact owner header, and its browser-node join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BodyPresentation {
    pub byte_offset: u64,
    pub entity_suffix: u64,
    pub entity_id: String,
    pub entity_id_offset: u64,
    pub browser_node: Option<BrowserNodeRecord>,
    pub material: Option<PresentationMaterial>,
}

/// Decode every browser node whose dynamic class resolves to the registered
/// browser-node type. The record body is ten zero bytes, an LP-UTF16 GUID, the
/// hidden flag, `01 01`, and the owning Design entity suffix.
pub(crate) fn browser_node_records(
    bytes: &[u8],
    stream_types: &HashMap<u32, (&str, u32)>,
) -> Vec<BrowserNodeRecord> {
    let mut out = Vec::new();
    for start in indexed_record_offsets(bytes) {
        let Some((type_guid, version)) = record_type(bytes, start, stream_types) else {
            continue;
        };
        if !type_guid.eq_ignore_ascii_case(BROWSER_NODE_TYPE_GUID)
            || version != BROWSER_NODE_TYPE_VERSION
            || bytes.get(start + 11..start + 21) != Some(&[0; 10])
        {
            continue;
        }
        let Some((guid, after_guid)) = lp_utf16_bounded(bytes, start + 21, GUID_LEN..=GUID_LEN)
        else {
            continue;
        };
        if !is_guid_prefix(&guid)
            || bytes.get(after_guid + 1..after_guid + 3) != Some(&[0x01, 0x01])
        {
            continue;
        }
        let Some(hidden @ (0 | 1)) = bytes.get(after_guid).copied() else {
            continue;
        };
        let Some(entity_suffix) = u64_at(bytes, after_guid + 3) else {
            continue;
        };
        out.push(BrowserNodeRecord {
            record_index: u32_at(bytes, start + 7).expect("indexed record has a u32 index"),
            guid,
            entity_suffix,
            hidden_offset: after_guid as u64,
            hidden: hidden == 1,
        });
    }
    out
}

/// Decode every typed body presentation in one Design stream.
pub(crate) fn body_presentations(
    bytes: &[u8],
    stream_types: &HashMap<u32, (&str, u32)>,
    entity_types: &HashMap<u64, (&str, u32)>,
) -> Vec<BodyPresentation> {
    let nodes = browser_node_records(bytes, stream_types);
    let mut starts = indexed_record_offsets(bytes)
        .filter(|start| {
            record_type(bytes, *start, stream_types).is_some_and(|(type_guid, version)| {
                type_guid.eq_ignore_ascii_case(BODY_PRESENTATION_TYPE_GUID)
                    && version == BODY_PRESENTATION_TYPE_VERSION
            })
        })
        .collect::<Vec<_>>();
    starts.sort_unstable();
    let node_starts = indexed_record_offsets(bytes)
        .filter(|start| {
            record_type(bytes, *start, stream_types).is_some_and(|(type_guid, version)| {
                type_guid.eq_ignore_ascii_case(BROWSER_NODE_TYPE_GUID)
                    && version == BROWSER_NODE_TYPE_VERSION
            })
        })
        .collect::<Vec<_>>();

    let mut out = Vec::new();
    for (ordinal, start) in starts.iter().copied().enumerate() {
        let record_end = starts.get(ordinal + 1).copied().unwrap_or(bytes.len());
        let envelope_end = node_starts
            .iter()
            .copied()
            .find(|node_start| *node_start > start && *node_start < record_end)
            .unwrap_or(record_end);
        let Some((entity_suffix, entity_id, _, header_end)) =
            parse_settled_entity_header(bytes, start)
                .or_else(|| parse_genesis_entity_header(bytes, start))
        else {
            continue;
        };
        let entity_id_offset = header_end
            .checked_sub(entity_id.encode_utf16().count() * 2)
            .expect("entity header end follows its UTF-16 payload");
        let material =
            presentation_material(bytes, header_end, envelope_end, entity_suffix, entity_types);
        let matching_nodes = material
            .as_ref()
            .map(|material| {
                nodes
                    .iter()
                    .filter(|node| {
                        node.entity_suffix == entity_suffix
                            && node.guid.eq_ignore_ascii_case(&material.node_guid)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let browser_node = match matching_nodes.as_slice() {
            [node] => Some((*node).clone()),
            _ => None,
        };
        out.push(BodyPresentation {
            byte_offset: start as u64,
            entity_suffix,
            entity_id,
            entity_id_offset: entity_id_offset as u64,
            browser_node,
            material,
        });
    }
    out
}

fn presentation_material(
    bytes: &[u8],
    start: usize,
    end: usize,
    entity_suffix: u64,
    entity_types: &HashMap<u64, (&str, u32)>,
) -> Option<PresentationMaterial> {
    let physical_marker = lp_utf16_bytes(PHYSICAL_MATERIAL_LIBRARY_ID);
    let legacy_marker = lp_utf16_bytes(APPEARANCE_LIBRARY_ID);
    let modern_marker = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[0]);
    let modern_trailer = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[1]);
    let mut candidates = Vec::new();
    for physical_at in find_all(bytes, start, end, &physical_marker) {
        let Some((physical_guid_at, physical_guid)) = preceding_lp_utf16(bytes, start, physical_at)
        else {
            continue;
        };
        let Some(node_tail_at) = physical_guid_at.checked_sub(11) else {
            continue;
        };
        if bytes.get(node_tail_at..node_tail_at + 11) != Some(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]) {
            continue;
        }
        let Some((_, node_guid)) = preceding_lp_utf16(bytes, start, node_tail_at) else {
            continue;
        };
        if physical_guid.len() != GUID_LEN
            || node_guid.len() != GUID_LEN
            || !is_guid_prefix(&physical_guid)
            || !is_guid_prefix(&node_guid)
        {
            continue;
        }
        let Some(token_at) = skip_zeros(bytes, physical_at + physical_marker.len(), end) else {
            continue;
        };
        let Some((physical_token, after_token)) = lp_utf16_bounded(bytes, token_at, 1..=256) else {
            continue;
        };
        if !is_physical_material_token(&physical_token) || after_token > end {
            continue;
        }
        let mut reference_at = after_token;
        let Some(brep_container_entity) = local_reference(bytes, &mut reference_at) else {
            continue;
        };
        if local_reference_value(bytes, &mut reference_at) != Some(LocalReference::Null) {
            continue;
        }
        let Some(scene_node_entity) = local_reference(bytes, &mut reference_at) else {
            continue;
        };
        if entity_types
            .get(&brep_container_entity)
            .is_none_or(|(guid, version)| {
                !guid.eq_ignore_ascii_case(BREP_CONTAINER_TYPE_GUID)
                    || *version != BREP_CONTAINER_TYPE_VERSION
            })
            || entity_suffix.checked_add(1) != Some(scene_node_entity)
            || entity_types
                .get(&scene_node_entity)
                .is_none_or(|(guid, version)| {
                    !guid.eq_ignore_ascii_case(BODY_SCENE_NODE_TYPE_GUID)
                        || *version != BODY_SCENE_NODE_TYPE_VERSION
                })
        {
            continue;
        }
        let Some((_, after_name)) = lp_utf16_bounded(bytes, reference_at, 0..=256) else {
            continue;
        };
        let Some(visual_at) = record_tail_visual_offset(bytes, after_name, end) else {
            continue;
        };
        let Some((visual_guid, after_visual)) = lp_utf16_bounded(bytes, visual_at, 1..=256) else {
            continue;
        };
        if visual_guid.len() < GUID_LEN || !is_guid_prefix(&visual_guid) {
            continue;
        }
        let Some(visual_marker_at) = skip_zeros(bytes, after_visual, end) else {
            continue;
        };
        let (after_visual_marker, legacy) = if bytes
            .get(visual_marker_at..visual_marker_at + legacy_marker.len())
            == Some(legacy_marker.as_slice())
        {
            (visual_marker_at + legacy_marker.len(), true)
        } else if bytes.get(visual_marker_at..visual_marker_at + modern_marker.len())
            == Some(modern_marker.as_slice())
        {
            let Some(trailer_at) = skip_zeros(bytes, visual_marker_at + modern_marker.len(), end)
            else {
                continue;
            };
            if bytes.get(trailer_at..trailer_at + modern_trailer.len())
                != Some(modern_trailer.as_slice())
            {
                continue;
            }
            (trailer_at + modern_trailer.len(), false)
        } else {
            continue;
        };
        let visual_preset = legacy
            .then(|| {
                let at = skip_zeros(bytes, after_visual_marker, end)?;
                let (value, _) = lp_utf16_bounded(bytes, at, 1..=256)?;
                value.starts_with("Prism-").then_some((at, value))
            })
            .flatten();
        candidates.push(PresentationMaterial {
            node_guid,
            physical_token,
            physical_token_offset: (token_at + 4) as u64,
            visual_guid: visual_guid[..GUID_LEN].to_owned(),
            visual_guid_offset: (visual_at + 4) as u64,
            visual_preset: visual_preset.as_ref().map(|(_, value)| value.clone()),
            visual_preset_offset: visual_preset.map(|(at, _)| (at + 4) as u64),
        });
    }
    match candidates.as_slice() {
        [material] => Some(material.clone()),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalReference {
    Null,
    Target(u64),
}

fn local_reference_value(bytes: &[u8], at: &mut usize) -> Option<LocalReference> {
    let reference = take_reference(bytes, at)?;
    if reference.segment.is_some() || reference.link_name.is_some() {
        return None;
    }
    Some(match reference.target {
        Some(target) => LocalReference::Target(target),
        None => LocalReference::Null,
    })
}

fn local_reference(bytes: &[u8], at: &mut usize) -> Option<u64> {
    match local_reference_value(bytes, at)? {
        LocalReference::Target(target) if target != 0 => Some(target),
        LocalReference::Null | LocalReference::Target(_) => None,
    }
}

fn record_tail_visual_offset(bytes: &[u8], name_end: usize, end: usize) -> Option<usize> {
    const OPACITY_ONE: [u8; 4] = 1.0f32.to_le_bytes();
    for marker_at in name_end..name_end.saturating_add(40).min(end) {
        if bytes.get(marker_at..marker_at + 2) != Some(&[0x01, 0x01]) {
            continue;
        }
        let gap = bytes.get(name_end..marker_at)?;
        let zeros_only = gap.iter().all(|byte| *byte == 0);
        let opacity_tail = gap.len() >= OPACITY_ONE.len()
            && gap[gap.len() - OPACITY_ONE.len()..] == OPACITY_ONE
            && gap[..gap.len() - OPACITY_ONE.len()]
                .iter()
                .all(|byte| *byte == 0);
        if zeros_only || opacity_tail {
            return skip_zeros_capped(bytes, marker_at + 2, end, 12);
        }
    }
    None
}

fn record_type<'a>(
    bytes: &[u8],
    start: usize,
    stream_types: &HashMap<u32, (&'a str, u32)>,
) -> Option<(&'a str, u32)> {
    let class_tag = std::str::from_utf8(bytes.get(start + 4..start + 7)?)
        .ok()?
        .parse::<u32>()
        .ok()?;
    stream_types.get(&class_tag).copied()
}

fn find_all<'a>(
    bytes: &'a [u8],
    start: usize,
    end: usize,
    needle: &'a [u8],
) -> impl Iterator<Item = usize> + 'a {
    let range = bytes.get(start..end).unwrap_or_default();
    memchr::memmem::find_iter(range, needle).map(move |offset| start + offset)
}

fn skip_zeros(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    skip_zeros_capped(bytes, start, end, MAX_ENVELOPE_GAP)
}

fn skip_zeros_capped(bytes: &[u8], start: usize, end: usize, cap: usize) -> Option<usize> {
    let mut at = start;
    while at < end && at - start < cap && bytes.get(at) == Some(&0) {
        at += 1;
    }
    (at <= end).then_some(at)
}

fn preceding_lp_utf16(bytes: &[u8], start: usize, marker_at: usize) -> Option<(usize, String)> {
    let mut candidates = Vec::new();
    for gap in 0..=MAX_ENVELOPE_GAP {
        let Some(end) = marker_at.checked_sub(gap) else {
            continue;
        };
        if end < start
            || bytes
                .get(end..marker_at)
                .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
        {
            continue;
        }
        let scan_start = end.saturating_sub(4 + 256 * 2).max(start);
        for at in scan_start..end {
            let Some((value, after)) = lp_utf16_bounded(bytes, at, 1..=256) else {
                continue;
            };
            if after == end {
                candidates.push((at, value));
            }
        }
    }
    candidates.sort_by_key(|(at, _)| *at);
    candidates.dedup();
    match candidates.as_slice() {
        [candidate] => Some(candidate.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_ascii(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    fn push_utf16(out: &mut Vec<u8>, value: &str) {
        out.extend(lp_utf16_bytes(value));
    }

    fn push_reference(out: &mut Vec<u8>, target: u64) {
        out.push(1);
        out.extend_from_slice(&target.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
    }

    #[test]
    fn typed_presentation_joins_its_exact_browser_node() {
        let body_tag = 256u32;
        let node_tag = 257u32;
        let entity = (1u64 << 40) + 42;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE";
        let mut bytes = Vec::new();
        push_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        push_utf16(&mut bytes, &format!("0_{entity}"));
        push_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        push_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        push_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        push_utf16(&mut bytes, "PrismMaterial-001");
        push_reference(&mut bytes, 7);
        bytes.push(0);
        push_reference(&mut bytes, entity + 1);
        push_utf16(&mut bytes, "Body");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        push_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            push_utf16(&mut bytes, marker);
        }
        push_ascii(&mut bytes, &node_tag.to_string());
        bytes.extend_from_slice(&43u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        push_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let types = HashMap::from([
            (body_tag, (BODY_PRESENTATION_TYPE_GUID, 19)),
            (node_tag, (BROWSER_NODE_TYPE_GUID, 2)),
        ]);
        let entity_types = HashMap::from([
            (7, (BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION)),
            (
                entity + 1,
                (BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION),
            ),
        ]);
        let presentations = body_presentations(&bytes, &types, &entity_types);
        assert_eq!(presentations.len(), 1);
        let presentation = &presentations[0];
        assert_eq!(presentation.entity_suffix, entity);
        assert_eq!(presentation.entity_id, format!("0_{entity}"));
        assert_eq!(presentation.browser_node.as_ref().unwrap().guid, node_guid);
        let material = presentation.material.as_ref().unwrap();
        assert_eq!(material.node_guid, node_guid);
        assert_eq!(material.physical_token, "PrismMaterial-001");
        assert_eq!(material.visual_guid, visual_guid);
        assert_eq!(material.visual_preset, None);
    }

    #[test]
    fn presentation_refuses_an_unregistered_node_shaped_byte_run() {
        let body_tag = 256u32;
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let mut bytes = Vec::new();
        push_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        push_utf16(&mut bytes, "0_42");
        push_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        push_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        push_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        push_utf16(&mut bytes, "PrismMaterial-001");
        push_reference(&mut bytes, 7);
        bytes.push(0);
        push_reference(&mut bytes, entity + 1);
        push_utf16(&mut bytes, "Body");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        push_utf16(&mut bytes, "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE");
        push_utf16(&mut bytes, APPEARANCE_LIBRARY_ID);
        push_utf16(&mut bytes, "Prism-001");
        push_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let types = HashMap::from([(body_tag, (BODY_PRESENTATION_TYPE_GUID, 19))]);
        let entity_types = HashMap::from([
            (7, (BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION)),
            (
                entity + 1,
                (BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION),
            ),
        ]);
        let presentations = body_presentations(&bytes, &types, &entity_types);
        assert_eq!(presentations.len(), 1);
        assert!(presentations[0].browser_node.is_none());
        assert_eq!(
            presentations[0]
                .material
                .as_ref()
                .and_then(|material| material.visual_preset.as_deref()),
            Some("Prism-001")
        );
    }
}
