// SPDX-License-Identifier: Apache-2.0
//! Parse Design segment metadata and the ordered feature timeline.

use cadmpeg_core::container::ContainerRole;

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_core::decode::View;

use crate::bytes::{
    Reference, is_guid_relaxed, lp_ascii_filtered, lp_utf16_bounded, take_reference,
};
use crate::container::ContainerScan;
use crate::ids::{self, native_stream};
use crate::records::{
    DESIGN_MODULE_FUSION, DesignComponentNamingSpace, DesignFeatureTimeline, SegmentType,
};

const COMPONENT_MODULE: &str = "Component";
const COMPONENT_NAMING_SPACE_BASE_TYPE_GUID: &str = "21F379C8-CAFD-4985-B461-767673A4C502";
const COMPONENT_UUID_RESERVED_LENGTHS: [usize; 2] = [2, 3];

/// Stable Design type identity of the record that owns the ordered feature
/// scope list.
pub(crate) const FEATURE_TIMELINE_TYPE_GUID: &str = "2F4C1849-1A5A-4F6C-A086-8DD445CBF94B";
pub(crate) const FEATURE_TIMELINE_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
pub(crate) const FEATURE_TIMELINE_TYPE_VERSIONS: [u32; 2] = [2, 3];

/// Whether a type-table row has the exact registration metadata of a supported
/// feature-timeline frame.
pub(crate) fn is_supported_feature_timeline_type(design_type: &SegmentType) -> bool {
    FEATURE_TIMELINE_TYPE_VERSIONS.contains(&design_type.version)
        && design_type.module == DESIGN_MODULE_FUSION
        && design_type
            .base_type_guid
            .as_deref()
            .is_some_and(|base| base.eq_ignore_ascii_case(FEATURE_TIMELINE_BASE_TYPE_GUID))
}

/// Decode the type table of every Design `MetaStream` entry.
pub fn decode_types(scan: &ContainerScan) -> Result<Vec<SegmentType>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Metastream))
    {
        let meta = scan.parsed_metastream(&entry.name)?;
        out.extend(meta.types.iter().cloned().map(|mut design_type| {
            design_type.id = ids::native_design_type_id(&entry.name, design_type.byte_offset);
            design_type
        }));
    }
    Ok(out)
}

fn insert_component_naming_space(
    by_component: &mut HashMap<u64, DesignComponentNamingSpace>,
    bulk_name: &str,
    marker: usize,
    component_record_index: u64,
    context_uuid: String,
    context_uuid_offset: usize,
) -> Result<(), CodecError> {
    let binding = DesignComponentNamingSpace {
        id: ids::native_design_component_naming_space_id(bulk_name, marker),
        byte_offset: marker as u64,
        component_record_index,
        context_uuid,
        context_uuid_offset: context_uuid_offset as u64,
    };
    if let Some(existing) = by_component.insert(component_record_index, binding.clone()) {
        if existing.context_uuid != binding.context_uuid {
            return Err(CodecError::malformed(format_args!(
                "Design component {component_record_index} has conflicting context UUID bindings"
            )));
        }
        by_component.insert(component_record_index, existing);
    }
    Ok(())
}

/// Decode each component entity's UUID-bound local naming space.
pub fn decode_component_naming_spaces(
    scan: &ContainerScan,
) -> Result<Vec<DesignComponentNamingSpace>, CodecError> {
    let mut out = Vec::new();
    for meta_entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Metastream))
    {
        let meta = scan.parsed_metastream(&meta_entry.name)?;
        let component_entities = meta
            .types
            .iter()
            .filter(|design_type| {
                design_type.module == COMPONENT_MODULE
                    && design_type.base_type_guid.as_deref().is_some_and(|base| {
                        base.eq_ignore_ascii_case(COMPONENT_NAMING_SPACE_BASE_TYPE_GUID)
                    })
            })
            .flat_map(|design_type| design_type.entity_ids.iter().copied())
            .collect::<HashSet<_>>();
        if component_entities.is_empty() {
            continue;
        }
        let prefix = meta_entry
            .name
            .strip_suffix("MetaStream.dat")
            .expect("filtered MetaStream entry has the expected basename");
        let bulk_name = format!("{prefix}BulkStream.dat");
        let bytes = scan.entry_bytes(&bulk_name)?;
        let mut by_component = HashMap::<u64, DesignComponentNamingSpace>::new();
        for reserved_len in COMPONENT_UUID_RESERVED_LENGTHS {
            let prefix_len = 1 + 8 + reserved_len;
            for uuid_offset in prefix_len..bytes.len().saturating_sub(4) {
                let marker = uuid_offset - prefix_len;
                if bytes[marker] != 1
                    || (marker > 0 && bytes[marker - 1] == 1)
                    || !bytes[marker + 9..uuid_offset].iter().all(|byte| *byte == 0)
                {
                    continue;
                }
                let Some(component_record_index) = View::u64_le_at(bytes, marker + 1) else {
                    continue;
                };
                if !component_entities.contains(&component_record_index) {
                    continue;
                }
                let Some((context_uuid, _)) = lp_utf16_bounded(bytes, uuid_offset, 36..=36) else {
                    continue;
                };
                if !is_guid_relaxed(&context_uuid) {
                    continue;
                }
                insert_component_naming_space(
                    &mut by_component,
                    &bulk_name,
                    marker,
                    component_record_index,
                    context_uuid,
                    uuid_offset,
                )?;
            }
        }
        for marker in 0..bytes.len() {
            let mut uuid_offset = marker;
            let Some(reference) = take_reference(bytes, &mut uuid_offset) else {
                continue;
            };
            let (Some(component_record_index), Some(inline_type_guid)) =
                (reference.target, reference.inline_type_guid.as_deref())
            else {
                continue;
            };
            if reference.segment.is_some()
                || reference.link_name.is_some()
                || !meta.types.iter().any(|design_type| {
                    design_type.module == COMPONENT_MODULE
                        && design_type.base_type_guid.as_deref().is_some_and(|base| {
                            base.eq_ignore_ascii_case(COMPONENT_NAMING_SPACE_BASE_TYPE_GUID)
                        })
                        && design_type.type_guid.eq_ignore_ascii_case(inline_type_guid)
                        && design_type.entity_ids.contains(&component_record_index)
                })
            {
                continue;
            }
            let Some((context_uuid, _)) = lp_utf16_bounded(bytes, uuid_offset, 36..=36) else {
                continue;
            };
            if !is_guid_relaxed(&context_uuid) {
                continue;
            }
            insert_component_naming_space(
                &mut by_component,
                &bulk_name,
                marker,
                component_record_index,
                context_uuid,
                uuid_offset,
            )?;
        }
        if let Some(missing) = component_entities
            .iter()
            .filter(|entity| !by_component.contains_key(entity))
            .min()
        {
            return Err(CodecError::malformed(format_args!(
                "Design component {missing} has no context UUID binding"
            )));
        }
        out.extend(by_component.into_values());
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Parse the `MetaStream` paired with one Design `BulkStream`.
pub(crate) fn metadata_for_bulk_stream(
    scan: &ContainerScan,
    bulk_entry_name: &str,
) -> Result<Option<crate::metastream::MetaStream>, CodecError> {
    let prefix = bulk_entry_name
        .strip_suffix("BulkStream.dat")
        .ok_or_else(|| CodecError::Malformed("Design stream has no BulkStream suffix".into()))?;
    let meta_name = format!("{prefix}MetaStream.dat");
    if !scan.entries.iter().any(|entry| entry.name == meta_name) {
        return Ok(None);
    }
    scan.parsed_metastream(&meta_name)
        .map(|meta| Some((*meta).clone()))
}

/// One live Design record selected by the primary index and resolved through
/// its segment-local class tag.
#[derive(Clone, Copy)]
pub(crate) struct DesignPrimaryFrame<'a> {
    pub(crate) entity_id: u64,
    pub(crate) class_tag: u32,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) design_type: &'a SegmentType,
}

fn dynamic_type(
    meta: &crate::metastream::MetaStream,
    class_tag: u32,
) -> Option<(usize, &SegmentType)> {
    let ordinal = usize::try_from(class_tag.checked_sub(256)?).ok()?;
    Some((ordinal, meta.types.get(ordinal)?))
}

fn record_header_class_tag(
    bytes: &[u8],
    at: usize,
    end: usize,
    expected_entity_id: u64,
) -> Option<u32> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, at, 3..=3, u8::is_ascii_digit)?;
    let indexed_matches = after_tag
        .checked_add(4)
        .filter(|entity_end| *entity_end <= end)
        .and_then(|_| View::u32_le_at(bytes, after_tag))
        .is_some_and(|entity_id| u64::from(entity_id) == expected_entity_id);
    let named_matches = after_tag
        .checked_add(8)
        .filter(|entity_end| *entity_end <= end)
        .and_then(|_| View::u64_le_at(bytes, after_tag))
        == Some(expected_entity_id);
    if !indexed_matches && !named_matches {
        return None;
    }
    class_tag.parse().ok()
}

/// Resolve every live sibling record from the primary index. The primary
/// header must repeat the indexed entity ID and select a type-table row that
/// registers that entity. A secondary entry supplies the exact end of the
/// primary class-member sequence and must point to a nested header for the same
/// entity.
pub(crate) fn design_primary_frames<'a>(
    bytes: &[u8],
    meta: &'a crate::metastream::MetaStream,
) -> Result<Vec<DesignPrimaryFrame<'a>>, CodecError> {
    let indexed = crate::metastream::primary_record_frames(meta, bytes.len())?;
    let registered_entities = meta
        .types
        .iter()
        .enumerate()
        .flat_map(|(ordinal, design_type)| {
            design_type
                .entity_ids
                .iter()
                .copied()
                .map(move |entity_id| (ordinal, entity_id))
        })
        .collect::<HashSet<_>>();
    let mut frames = Vec::with_capacity(indexed.len());
    for frame in indexed {
        let entity_id = frame.entity_id;
        let Some(class_tag) = record_header_class_tag(bytes, frame.start, frame.end, entity_id)
        else {
            return Err(CodecError::Malformed(
                "F3D primary record index points to an invalid record header".into(),
            ));
        };
        let Some((type_ordinal, design_type)) = dynamic_type(meta, class_tag) else {
            return Err(CodecError::Malformed(
                "F3D primary record class tag is outside its type table".into(),
            ));
        };
        if !registered_entities.contains(&(type_ordinal, entity_id)) {
            return Err(CodecError::Malformed(
                "F3D primary record type does not register its indexed entity ID".into(),
            ));
        }
        if frame.member_end < frame.end {
            let Some(nested_class_tag) =
                record_header_class_tag(bytes, frame.member_end, frame.end, entity_id)
            else {
                return Err(CodecError::Malformed(
                    "F3D secondary record index points to an invalid nested header".into(),
                ));
            };
            if dynamic_type(meta, nested_class_tag).is_none() {
                return Err(CodecError::Malformed(
                    "F3D secondary record header is incompatible with its primary record".into(),
                ));
            }
        }
        frames.push(DesignPrimaryFrame {
            entity_id,
            class_tag,
            start: frame.start,
            end: frame.end,
            design_type,
        });
    }
    Ok(frames)
}

/// One primary `BulkStream` frame selected through a registered Design type.
#[derive(Clone, Copy)]
pub(crate) struct TypedPrimaryFrame<'a> {
    pub(crate) entity_id: u64,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) design_type: &'a SegmentType,
}

/// Resolve every entity registered to `type_guid` through the sibling
/// `MetaStream` primary index and verify its dynamic class tag.
pub(crate) fn typed_primary_frames<'a>(
    bytes: &[u8],
    meta: &'a crate::metastream::MetaStream,
    type_guid: &str,
    record_kind: &str,
) -> Result<Vec<TypedPrimaryFrame<'a>>, CodecError> {
    let mut typed_entities = HashSet::new();
    for design_type in &meta.types {
        if !design_type.type_guid.eq_ignore_ascii_case(type_guid) {
            continue;
        }
        for &entity_id in &design_type.entity_ids {
            if !typed_entities.insert(entity_id) {
                return Err(CodecError::malformed(format_args!(
                    "F3D Design {record_kind} entity {entity_id} is registered more than once"
                )));
            }
        }
    }

    let mut resolved_entities = HashSet::new();
    let mut frames = Vec::new();
    for primary_frame in design_primary_frames(bytes, meta)? {
        if !primary_frame
            .design_type
            .type_guid
            .eq_ignore_ascii_case(type_guid)
        {
            continue;
        }
        resolved_entities.insert(primary_frame.entity_id);
        frames.push(TypedPrimaryFrame {
            entity_id: primary_frame.entity_id,
            start: primary_frame.start,
            end: primary_frame.end,
            design_type: primary_frame.design_type,
        });
    }
    if let Some(entity_id) = typed_entities.difference(&resolved_entities).min() {
        return Err(CodecError::malformed(format_args!(
            "F3D Design {record_kind} entity {entity_id} has no primary record of its registered class"
        )));
    }
    Ok(frames)
}

/// Type GUID and record version keyed by the Design entity ids that carry the
/// type in the sibling `BulkStream`.
pub(crate) fn stream_types_by_entity<'a>(
    types: &'a [SegmentType],
    bulk_entry_name: &str,
) -> HashMap<u64, (&'a str, u32)> {
    let Some(prefix) = bulk_entry_name.strip_suffix("BulkStream.dat") else {
        return HashMap::new();
    };
    let meta_scope = ids::native_scope(&format!("{prefix}MetaStream.dat"));
    types
        .iter()
        .filter(|design_type| native_stream(&design_type.id) == Some(meta_scope.as_str()))
        .flat_map(|design_type| {
            design_type.entity_ids.iter().map(|entity_id| {
                (
                    *entity_id,
                    (design_type.type_guid.as_str(), design_type.version),
                )
            })
        })
        .collect()
}

/// Complete type-table row keyed by the segment-local dynamic class tag.
pub(crate) fn stream_types_by_class_tag<'a>(
    types: &'a [SegmentType],
    bulk_entry_name: &str,
) -> HashMap<u32, &'a SegmentType> {
    let Some(prefix) = bulk_entry_name.strip_suffix("BulkStream.dat") else {
        return HashMap::new();
    };
    let meta_scope = ids::native_scope(&format!("{prefix}MetaStream.dat"));
    types
        .iter()
        .filter(|design_type| native_stream(&design_type.id) == Some(meta_scope.as_str()))
        .enumerate()
        .filter_map(|(ordinal, design_type)| {
            Some((u32::try_from(ordinal).ok()?.checked_add(256)?, design_type))
        })
        .collect()
}

fn local_reference(
    reference: &Reference,
    type_guids_by_entity: &HashMap<u64, Vec<&str>>,
) -> Option<u64> {
    if reference.segment.is_some() || reference.link_name.is_some() {
        return None;
    }
    let target = reference.target?;
    if let Some(inline_type_guid) = &reference.inline_type_guid {
        let registered_type_guids = type_guids_by_entity.get(&target)?;
        if !registered_type_guids
            .iter()
            .any(|registered| registered.eq_ignore_ascii_case(inline_type_guid))
        {
            return None;
        }
    }
    Some(target)
}

fn parse_feature_timeline_record(
    bytes: &[u8],
    stream: &str,
    frame: std::ops::Range<usize>,
    expected_class_tag: &str,
    expected_entity_id: u64,
    source_ordinal: u32,
    type_guids_by_entity: &HashMap<u64, Vec<&str>>,
) -> Option<DesignFeatureTimeline> {
    let (start, end) = (frame.start, frame.end);
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 3..=3, u8::is_ascii_digit)?;
    if class_tag != expected_class_tag || View::u64_le_at(bytes, after_tag)? != expected_entity_id {
        return None;
    }
    let (_, payload) = lp_ascii_filtered(
        bytes,
        after_tag.checked_add(8)?,
        0..=2000,
        u8::is_ascii_graphic,
    )?;
    if bytes.get(payload..payload.checked_add(2)?)? != [0, 0] {
        return None;
    }

    let mut at = payload.checked_add(2)?;
    let context_reference_offset = at.checked_add(1)?;
    let context_record_index =
        local_reference(&take_reference(bytes, &mut at)?, type_guids_by_entity)?;
    if context_record_index == 0 {
        return None;
    }
    let item_count_offset = at;
    let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    at = at.checked_add(4)?;
    if count > end.checked_sub(at)? / 11 {
        return None;
    }
    let mut item_record_indices = Vec::with_capacity(count);
    let mut item_record_index_offsets = Vec::with_capacity(count);
    let mut unique = HashSet::with_capacity(count);
    for _ in 0..count {
        let target_offset = at.checked_add(1)?;
        let target = local_reference(&take_reference(bytes, &mut at)?, type_guids_by_entity)?;
        if target == 0 || !unique.insert(target) {
            return None;
        }
        item_record_indices.push(target);
        item_record_index_offsets.push(target_offset as u64);
    }
    if at != end {
        return None;
    }

    Some(DesignFeatureTimeline {
        id: ids::native_design_feature_timeline_id(stream, start),
        byte_offset: start as u64,
        class_tag,
        record_index: expected_entity_id,
        source_ordinal,
        frame_length: end.checked_sub(start)? as u64,
        context_record_index,
        context_record_index_offset: context_reference_offset as u64,
        item_count_offset: item_count_offset as u64,
        item_record_indices,
        item_record_index_offsets,
    })
}

/// Decode the exact counted scope list that carries authored feature order.
pub fn decode_feature_timelines(
    scan: &ContainerScan,
) -> Result<Vec<DesignFeatureTimeline>, CodecError> {
    let mut out = Vec::new();
    for meta_entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Metastream))
    {
        let meta = crate::metastream::parse(scan.entry_bytes(&meta_entry.name)?, &meta_entry.name)?;
        let timeline_types = meta
            .types
            .iter()
            .enumerate()
            .filter(|(_, design_type)| {
                design_type
                    .type_guid
                    .eq_ignore_ascii_case(FEATURE_TIMELINE_TYPE_GUID)
            })
            .collect::<Vec<_>>();
        if timeline_types.is_empty() {
            continue;
        }
        if timeline_types
            .iter()
            .any(|(_, design_type)| !FEATURE_TIMELINE_TYPE_VERSIONS.contains(&design_type.version))
        {
            return Err(CodecError::NotImplemented(
                "unsupported Design feature-timeline record version".into(),
            ));
        }
        if timeline_types
            .iter()
            .any(|(_, design_type)| !is_supported_feature_timeline_type(design_type))
        {
            return Err(CodecError::Malformed(
                "Design feature-timeline type has incompatible registration metadata".into(),
            ));
        }
        if meta
            .records
            .windows(2)
            .any(|pair| pair[0].bulk_offset >= pair[1].bulk_offset)
        {
            return Err(CodecError::Malformed(
                "Design MetaStream record offsets are not strictly increasing".into(),
            ));
        }
        let prefix = meta_entry
            .name
            .strip_suffix("MetaStream.dat")
            .expect("filtered MetaStream entry has the expected basename");
        let bulk_name = format!("{prefix}BulkStream.dat");
        let bytes = scan.entry_bytes(&bulk_name)?;
        let mut type_guids_by_entity = HashMap::<u64, Vec<&str>>::new();
        for design_type in &meta.types {
            for entity_id in &design_type.entity_ids {
                type_guids_by_entity
                    .entry(*entity_id)
                    .or_default()
                    .push(&design_type.type_guid);
            }
        }
        let mut source_ordinal = 0_u32;
        for (type_ordinal, design_type) in timeline_types {
            let expected_class_tag = u32::try_from(type_ordinal)
                .ok()
                .and_then(|ordinal| ordinal.checked_add(256))
                .filter(|class_tag| *class_tag <= 999)
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "Design feature-timeline class tag is not three digits".into(),
                    )
                })?
                .to_string();
            for entity_id in &design_type.entity_ids {
                let entity_source_ordinal = source_ordinal;
                source_ordinal = source_ordinal.checked_add(1).ok_or_else(|| {
                    CodecError::Malformed("Design feature-timeline ordinal exceeds u32".into())
                })?;
                let matches = meta
                    .records
                    .iter()
                    .enumerate()
                    .filter(|(_, record)| record.entity_id == *entity_id)
                    .collect::<Vec<_>>();
                let [(record_ordinal, record)] = matches.as_slice() else {
                    return Err(CodecError::Malformed(
                        "Design feature timeline has no unique primary record-index entry".into(),
                    ));
                };
                let start = usize::try_from(record.bulk_offset).map_err(|_| {
                    CodecError::Malformed("Design feature-timeline offset exceeds usize".into())
                })?;
                let end = meta.records.get(record_ordinal + 1).map_or_else(
                    || Ok(bytes.len()),
                    |next| {
                        usize::try_from(next.bulk_offset).map_err(|_| {
                            CodecError::Malformed(
                                "Design feature-timeline end exceeds usize".into(),
                            )
                        })
                    },
                )?;
                if start >= end || end > bytes.len() {
                    return Err(CodecError::Malformed(
                        "Design feature-timeline record extent is outside its BulkStream".into(),
                    ));
                }
                let timeline = parse_feature_timeline_record(
                    bytes,
                    &bulk_name,
                    start..end,
                    &expected_class_tag,
                    *entity_id,
                    entity_source_ordinal,
                    &type_guids_by_entity,
                )
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "Design feature-timeline record does not match its exact frame".into(),
                    )
                })?;
                out.push(timeline);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
