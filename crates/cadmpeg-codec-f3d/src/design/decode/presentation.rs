// SPDX-License-Identifier: Apache-2.0
//! Parse typed Design body-presentation and browser-node records.

use std::collections::HashMap;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::{is_guid_prefix, lp_utf16_bounded, lp_utf16_bytes, take_reference};
use crate::design::decode::meta::typed_primary_frames;
use crate::design::decode::sketch::{parse_genesis_entity_header, parse_settled_entity_header};
use crate::design::presentation::{
    is_physical_material_token, visual_token, APPEARANCE_LIBRARY_ID,
    BODY_PRESENTATION_BASE_TYPE_GUID, BODY_PRESENTATION_MATERIAL_ENVELOPE_ID,
    BODY_PRESENTATION_TYPE_GUID, BODY_PRESENTATION_TYPE_VERSION, BODY_SCENE_NODE_TYPE_GUID,
    BODY_SCENE_NODE_TYPE_VERSION, BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION,
    BROWSER_NODE_BASE_TYPE_GUID, BROWSER_NODE_TYPE_GUID, BROWSER_NODE_TYPE_VERSION, GUID_LEN,
    MODERN_APPEARANCE_LIBRARY_IDS, PHYSICAL_MATERIAL_LIBRARY_ID,
};
use crate::records::{DESIGN_MODULE_BODY, DESIGN_MODULE_FUSION};

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
    pub visual_preset: Option<crate::records::Located<String>>,
}

/// One body record, its exact owner header, and its browser-node join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BodyPresentation {
    pub byte_offset: u64,
    pub entity_suffix: u64,
    pub owner: BodyPresentationOwner,
    pub browser_node: Option<BrowserNodeRecord>,
    pub material: Option<PresentationMaterial>,
}

/// Entity identity form stored by one body-presentation owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BodyPresentationOwner {
    /// The owner stores a component-qualified entity ID after its entity
    /// suffix.
    Named {
        entity_id: String,
        entity_id_offset: u64,
    },
    /// The owner stores only its u64 entity suffix in the indexed head.
    Bare,
}

/// Decode every browser node whose dynamic class resolves to the registered
/// browser-node type. The record body is ten zero bytes, an LP-UTF16 GUID, the
/// hidden flag, `01 01`, and the owning Design entity suffix.
pub(crate) fn browser_node_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BrowserNodeRecord>, CodecError> {
    let mut out = Vec::new();
    for frame in typed_primary_frames(bytes, meta, BROWSER_NODE_TYPE_GUID, "browser-node")? {
        if frame.design_type.version != BROWSER_NODE_TYPE_VERSION {
            continue;
        }
        if frame.design_type.module != DESIGN_MODULE_FUSION
            || !frame
                .design_type
                .base_type_guid
                .as_deref()
                .is_some_and(|base| base.eq_ignore_ascii_case(BROWSER_NODE_BASE_TYPE_GUID))
        {
            return Err(CodecError::malformed(format_args!(
                "F3D Design browser-node entity {} has incompatible registration metadata",
                frame.entity_id
            )));
        }
        let record = &bytes[frame.start..frame.end];
        let record_index = View::u32_le_at(record, 7).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D Design browser-node entity {} has a truncated record index",
                frame.entity_id
            ))
        })?;
        if u64::from(record_index) != frame.entity_id || record.get(11..21) != Some(&[0; 10]) {
            return Err(CodecError::malformed(format_args!(
                "F3D Design browser-node entity {} has an invalid header",
                frame.entity_id
            )));
        }
        let Some((guid, after_guid)) =
            lp_utf16_bounded(record, 21, GUID_LEN..=GUID_LEN).filter(|(guid, after)| {
                is_guid_prefix(guid)
                    && after
                        .checked_add(11)
                        .is_some_and(|record_end| record_end <= record.len())
                    && record.get(*after + 1..*after + 3) == Some(&[0x01, 0x01])
            })
        else {
            continue;
        };
        let hidden @ (0 | 1) = record.get(after_guid).copied().ok_or_else(|| {
            CodecError::Malformed("F3D Design browser-node flag is truncated".into())
        })?
        else {
            return Err(CodecError::malformed(format_args!(
                "F3D Design browser-node entity {} has an invalid hidden flag",
                frame.entity_id
            )));
        };
        let entity_suffix = View::u64_le_at(record, after_guid + 3).ok_or_else(|| {
            CodecError::Malformed("F3D Design browser-node suffix is truncated".into())
        })?;
        out.push(BrowserNodeRecord {
            record_index,
            guid,
            entity_suffix,
            hidden_offset: (frame.start + after_guid) as u64,
            hidden: hidden == 1,
        });
    }
    Ok(out)
}

/// Decode every typed body presentation in one Design stream.
pub(crate) fn body_presentations(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyPresentation>, CodecError> {
    let nodes = browser_node_records(bytes, meta)?;
    let entity_types = entity_types(meta)?;

    let mut out = Vec::new();
    for frame in typed_primary_frames(
        bytes,
        meta,
        BODY_PRESENTATION_TYPE_GUID,
        "body-presentation",
    )? {
        if frame.design_type.version != BODY_PRESENTATION_TYPE_VERSION {
            continue;
        }
        if frame.design_type.module != DESIGN_MODULE_BODY
            || !frame
                .design_type
                .base_type_guid
                .as_deref()
                .is_some_and(|base| base.eq_ignore_ascii_case(BODY_PRESENTATION_BASE_TYPE_GUID))
        {
            return Err(CodecError::malformed(format_args!(
                "F3D Design body-presentation entity {} has incompatible registration metadata",
                frame.entity_id
            )));
        }
        let framed_bytes = &bytes[..frame.end];
        let named_header = parse_settled_entity_header(framed_bytes, frame.start)
            .or_else(|| parse_genesis_entity_header(framed_bytes, frame.start));
        let (entity_suffix, owner, material) = if let Some((
            entity_suffix,
            entity_id,
            _,
            header_end,
        )) = named_header
        {
            if entity_suffix != frame.entity_id {
                return Err(CodecError::malformed(format_args!(
                    "F3D Design body-presentation entity {} disagrees with its named header entity {entity_suffix}",
                    frame.entity_id
                )));
            }
            let entity_id_offset = header_end
                .checked_sub(entity_id.encode_utf16().count() * 2)
                .expect("entity header end follows its UTF-16 payload");
            (
                entity_suffix,
                BodyPresentationOwner::Named {
                    entity_id,
                    entity_id_offset: entity_id_offset as u64,
                },
                presentation_material(
                    framed_bytes,
                    header_end,
                    frame.end,
                    entity_suffix,
                    &entity_types,
                ),
            )
        } else {
            let entity_suffix =
                View::u64_le_at(framed_bytes, frame.start + 7).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "F3D Design bare body-presentation entity {} has a truncated head",
                        frame.entity_id
                    ))
                })?;
            if entity_suffix == 0 || entity_suffix != frame.entity_id {
                return Err(CodecError::malformed(format_args!(
                    "F3D Design bare body-presentation entity {} has head entity {entity_suffix}",
                    frame.entity_id
                )));
            }
            let Some(material) = bare_presentation_material(
                framed_bytes,
                frame.start + 15,
                frame.end,
                entity_suffix,
            ) else {
                continue;
            };
            (entity_suffix, BodyPresentationOwner::Bare, Some(material))
        };
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
            byte_offset: frame.start as u64,
            entity_suffix,
            owner,
            browser_node,
            material,
        });
    }
    Ok(out)
}

fn entity_types(
    meta: &crate::metastream::MetaStream,
) -> Result<HashMap<u64, (&str, u32)>, CodecError> {
    let mut out = HashMap::new();
    for design_type in &meta.types {
        for &entity_id in &design_type.entity_ids {
            if out
                .insert(
                    entity_id,
                    (design_type.type_guid.as_str(), design_type.version),
                )
                .is_some()
            {
                return Err(CodecError::malformed(format_args!(
                    "F3D Design entity {entity_id} has multiple registered types"
                )));
            }
        }
    }
    Ok(out)
}

fn presentation_material(
    bytes: &[u8],
    start: usize,
    end: usize,
    entity_suffix: u64,
    entity_types: &HashMap<u64, (&str, u32)>,
) -> Option<PresentationMaterial> {
    let bytes = bytes.get(..end)?;
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
        if visual_token(&visual_guid).is_none() {
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
            visual_guid,
            visual_guid_offset: (visual_at + 4) as u64,
            visual_preset: visual_preset.map(|(at, value)| crate::records::Located { value, offset: (at + 4) as u64 }),
        });
    }
    match candidates.as_slice() {
        [material] => Some(material.clone()),
        _ => None,
    }
}

/// Parse the material envelope of a body-presentation owner whose indexed
/// head stores no component-qualified entity ID.
fn bare_presentation_material(
    bytes: &[u8],
    start: usize,
    end: usize,
    entity_suffix: u64,
) -> Option<PresentationMaterial> {
    let bytes = bytes.get(..end)?;
    let marker = lp_utf16_bytes(BODY_PRESENTATION_MATERIAL_ENVELOPE_ID)
        .into_iter()
        .chain(lp_utf16_bytes(PHYSICAL_MATERIAL_LIBRARY_ID))
        .collect::<Vec<_>>();
    let modern_marker = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[0]);
    let modern_trailer = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[1]);
    let mut candidates = Vec::new();
    for marker_at in find_all(bytes, start, end, &marker) {
        let Some(token_at) = skip_zeros(bytes, marker_at + marker.len(), end) else {
            continue;
        };
        let Some((physical_token, after_token)) = lp_utf16_bounded(bytes, token_at, 1..=256) else {
            continue;
        };
        if !is_physical_material_token(&physical_token) {
            continue;
        }

        let mut physical_reference_at = after_token;
        if local_reference(bytes, &mut physical_reference_at).is_none() {
            continue;
        }
        let Some(node_guid_at) = skip_zeros(bytes, physical_reference_at, end) else {
            continue;
        };
        let Some((node_guid, after_node_guid)) =
            lp_utf16_bounded(bytes, node_guid_at, GUID_LEN..=GUID_LEN)
        else {
            continue;
        };
        if !is_guid_prefix(&node_guid) {
            continue;
        }
        let mut node_reference_at = after_node_guid;
        let Some(node_entity) = local_reference(bytes, &mut node_reference_at) else {
            continue;
        };
        if entity_suffix.checked_add(1) != Some(node_entity) {
            continue;
        }

        let mut name_ends = vec![node_reference_at];
        if let Some((_, after_name)) = skip_zeros(bytes, node_reference_at, end)
            .and_then(|name_at| lp_utf16_bounded(bytes, name_at, 1..=256))
        {
            name_ends.push(after_name);
        }
        let mut visual_offsets = name_ends
            .into_iter()
            .filter_map(|name_end| record_tail_visual_offset(bytes, name_end, end))
            .collect::<Vec<_>>();
        visual_offsets.sort_unstable();
        visual_offsets.dedup();
        let [visual_at] = visual_offsets.as_slice() else {
            continue;
        };
        let Some((visual_guid, after_visual)) = lp_utf16_bounded(bytes, *visual_at, 1..=256) else {
            continue;
        };
        if visual_token(&visual_guid).is_none() {
            continue;
        }
        let Some(marker_at) = skip_zeros(bytes, after_visual, end) else {
            continue;
        };
        if bytes.get(marker_at..marker_at + modern_marker.len()) != Some(modern_marker.as_slice()) {
            continue;
        }
        let Some(trailer_at) = skip_zeros(bytes, marker_at + modern_marker.len(), end) else {
            continue;
        };
        if bytes.get(trailer_at..trailer_at + modern_trailer.len())
            != Some(modern_trailer.as_slice())
        {
            continue;
        }
        candidates.push(PresentationMaterial {
            node_guid,
            physical_token,
            physical_token_offset: (token_at + 4) as u64,
            visual_guid,
            visual_guid_offset: (*visual_at + 4) as u64,
            visual_preset: None,
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
    (at <= end && (at == end || bytes.get(at) != Some(&0))).then_some(at)
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

    fn design_type(
        type_guid: &str,
        base_type_guid: Option<&str>,
        version: u32,
        module: &str,
        entity_ids: Vec<u64>,
    ) -> crate::records::SegmentType {
        crate::records::SegmentType {
            id: String::new(),
            byte_offset: 0,
            type_guid: type_guid.into(),
            type_guid_offset: 0,
            base_type_guid: base_type_guid.map(str::to_owned),
            base_type_guid_offset: base_type_guid.map(|_| 0),
            version,
            version_offset: 0,
            module: module.into(),
            entity_ids,
            entity_id_offsets: Vec::new(),
        }
    }

    fn primary_record(entity_id: u64, bulk_offset: usize) -> crate::metastream::RecordIndexEntry {
        crate::metastream::RecordIndexEntry {
            entity_id,
            bulk_offset: bulk_offset as u64,
        }
    }

    #[test]
    fn typed_presentation_joins_its_exact_browser_node() {
        let body_tag = 256u32;
        let node_tag = 257u32;
        let entity = (1u64 << 40) + 42;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
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
        let node_start = bytes.len();
        push_ascii(&mut bytes, &node_tag.to_string());
        bytes.extend_from_slice(&43u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        push_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![43],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0), primary_record(43, node_start)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("typed primary frames");
        assert_eq!(presentations.len(), 1);
        let presentation = &presentations[0];
        assert_eq!(presentation.entity_suffix, entity);
        assert_eq!(
            presentation.owner,
            BodyPresentationOwner::Named {
                entity_id: format!("0_{entity}"),
                entity_id_offset: 25,
            }
        );
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

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("typed body owner");
        assert_eq!(presentations.len(), 1);
        assert!(presentations[0].browser_node.is_none());
        assert_eq!(
            presentations[0]
                .material
                .as_ref()
                .and_then(|material| material.visual_preset.as_ref().map(|field| field.value.as_str())),
            Some("Prism-001")
        );
    }

    #[test]
    fn presentation_does_not_consume_the_next_primary_frame() {
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let mut bytes = Vec::new();
        push_ascii(&mut bytes, "256");
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        push_utf16(&mut bytes, "0_42");

        let next_start = bytes.len();
        push_ascii(&mut bytes, "257");
        bytes.extend_from_slice(&99u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
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

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    "00000000-0000-0000-0000-000000000000",
                    None,
                    0,
                    "",
                    vec![99],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0), primary_record(99, next_start)],
            secondary_records: Vec::new(),
        };

        let presentations = body_presentations(&bytes, &meta).expect("exact body frame");
        let [presentation] = presentations.as_slice() else {
            panic!("one body presentation expected")
        };
        assert!(presentation.material.is_none());
    }

    #[test]
    fn browser_node_records_skip_other_typed_member_variants() {
        let record_index = 43u32;
        let mut bytes = Vec::new();
        push_ascii(&mut bytes, "256");
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        push_utf16(&mut bytes, "11111111-2222-8333-A444-555555555555");
        bytes.extend_from_slice(&[1, 0, 1]);
        bytes.extend_from_slice(&42u64.to_le_bytes());
        let meta = crate::metastream::MetaStream {
            types: vec![design_type(
                BROWSER_NODE_TYPE_GUID,
                Some(BROWSER_NODE_BASE_TYPE_GUID),
                BROWSER_NODE_TYPE_VERSION,
                DESIGN_MODULE_FUSION,
                vec![u64::from(record_index)],
            )],
            records: vec![primary_record(u64::from(record_index), 0)],
            secondary_records: Vec::new(),
        };

        assert!(browser_node_records(&bytes, &meta)
            .expect("typed alternate member")
            .is_empty());
    }

    #[test]
    fn bare_presentation_uses_its_typed_primary_owner_not_class_299() {
        let entity = 2_000u64;
        let node_record = 2_100u32;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut types = (0..44)
            .map(|_| {
                design_type(
                    "00000000-0000-0000-0000-000000000000",
                    None,
                    0,
                    "",
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>();
        let body_tag = 300u32;
        let node_tag = 301u32;
        types.extend([
            design_type(
                BODY_PRESENTATION_TYPE_GUID,
                Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                BODY_PRESENTATION_TYPE_VERSION,
                DESIGN_MODULE_BODY,
                vec![entity],
            ),
            design_type(
                BROWSER_NODE_TYPE_GUID,
                Some(BROWSER_NODE_BASE_TYPE_GUID),
                BROWSER_NODE_TYPE_VERSION,
                DESIGN_MODULE_FUSION,
                vec![u64::from(node_record)],
            ),
        ]);

        let mut bytes = Vec::new();
        push_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        push_ascii(&mut bytes, "299");
        bytes.extend_from_slice(&999_999u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        push_utf16(&mut bytes, BODY_PRESENTATION_MATERIAL_ENVELOPE_ID);
        push_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        bytes.extend_from_slice(&[0; 4]);
        push_utf16(&mut bytes, "PrismMaterial-018");
        push_reference(&mut bytes, 1_900);
        bytes.push(0);
        push_utf16(&mut bytes, node_guid);
        push_reference(&mut bytes, entity + 1);
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend_from_slice(&[0; 10]);
        push_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            push_utf16(&mut bytes, marker);
        }

        let node_start = bytes.len();
        assert!(bare_presentation_material(&bytes, 15, node_start, entity).is_some());
        let trailer_len = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[1]).len();
        assert!(
            bare_presentation_material(&bytes, 15, node_start - trailer_len, entity,).is_none()
        );
        push_ascii(&mut bytes, &node_tag.to_string());
        bytes.extend_from_slice(&node_record.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        push_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let meta = crate::metastream::MetaStream {
            types,
            records: vec![
                primary_record(entity, 0),
                primary_record(u64::from(node_record), node_start),
            ],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("exact primary owner frame");
        let [presentation] = presentations.as_slice() else {
            panic!("one bare body presentation expected")
        };
        assert_eq!(presentation.entity_suffix, entity);
        assert_eq!(presentation.owner, BodyPresentationOwner::Bare);
        assert_eq!(
            presentation
                .browser_node
                .as_ref()
                .map(|node| node.guid.as_str()),
            Some(node_guid)
        );
        let material = presentation.material.as_ref().expect("material envelope");
        assert_eq!(material.physical_token, "PrismMaterial-018");
        assert_eq!(material.visual_guid, visual_guid);
    }
}
