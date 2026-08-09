// SPDX-License-Identifier: Apache-2.0
//! Parse Design segment metadata and the ordered feature timeline.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::le::{u32_at, u64_at};
use cadmpeg_core::CodecError;

use crate::bytes::{lp_ascii_filtered, take_reference, Reference};
use crate::container::{role, ContainerScan};
use crate::ids::{self, native_stream};
use crate::records::{DesignFeatureTimeline, SegmentType, DESIGN_MODULE_FUSION};

/// Stable Design type identity of the record that owns the ordered feature
/// scope list.
pub(crate) const FEATURE_TIMELINE_TYPE_GUID: &str = "2F4C1849-1A5A-4F6C-A086-8DD445CBF94B";
pub(crate) const FEATURE_TIMELINE_TYPE_VERSION: u32 = 3;

/// Decode the type table of every Design `MetaStream` entry.
pub fn decode_types(scan: &ContainerScan) -> Result<Vec<SegmentType>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::METASTREAM && entry.name.contains("Design"))
    {
        let meta = crate::metastream::parse(scan.entry_bytes(&entry.name)?, &entry.name)?;
        out.extend(meta.types.into_iter().map(|mut design_type| {
            design_type.id = ids::native_design_type_id(&entry.name, design_type.byte_offset);
            design_type
        }));
    }
    Ok(out)
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

/// Type GUID and record version keyed by the segment-local dynamic class tag.
pub(crate) fn stream_types_by_class_tag<'a>(
    types: &'a [SegmentType],
    bulk_entry_name: &str,
) -> HashMap<u32, (&'a str, u32)> {
    let Some(prefix) = bulk_entry_name.strip_suffix("BulkStream.dat") else {
        return HashMap::new();
    };
    let meta_scope = ids::native_scope(&format!("{prefix}MetaStream.dat"));
    types
        .iter()
        .filter(|design_type| native_stream(&design_type.id) == Some(meta_scope.as_str()))
        .enumerate()
        .filter_map(|(ordinal, design_type)| {
            Some((
                u32::try_from(ordinal).ok()?.checked_add(256)?,
                (design_type.type_guid.as_str(), design_type.version),
            ))
        })
        .collect()
}

fn local_reference(reference: &Reference) -> Option<u64> {
    (reference.segment.is_none() && reference.link_name.is_none()).then_some(reference.target?)
}

fn parse_feature_timeline_record(
    bytes: &[u8],
    stream: &str,
    start: usize,
    end: usize,
    expected_class_tag: &str,
    expected_entity_id: u64,
    source_ordinal: u32,
) -> Option<DesignFeatureTimeline> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 3..=3, u8::is_ascii_digit)?;
    if class_tag != expected_class_tag || u64_at(bytes, after_tag)? != expected_entity_id {
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
    let context_record_index = local_reference(&take_reference(bytes, &mut at)?)?;
    if context_record_index == 0 {
        return None;
    }
    let item_count_offset = at;
    let count = usize::try_from(u32_at(bytes, at)?).ok()?;
    at = at.checked_add(4)?;
    if count > end.checked_sub(at)? / 11 {
        return None;
    }
    let mut item_record_indices = Vec::with_capacity(count);
    let mut item_record_index_offsets = Vec::with_capacity(count);
    let mut unique = HashSet::with_capacity(count);
    for _ in 0..count {
        let target_offset = at.checked_add(1)?;
        let target = local_reference(&take_reference(bytes, &mut at)?)?;
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
        .filter(|entry| entry.role == role::METASTREAM && entry.name.contains("Design"))
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
        if timeline_types.iter().any(|(_, design_type)| {
            design_type.version != FEATURE_TIMELINE_TYPE_VERSION
                || design_type.module != DESIGN_MODULE_FUSION
        }) {
            return Err(CodecError::NotImplemented(
                "unsupported Design feature-timeline record version".into(),
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
                    start,
                    end,
                    &expected_class_tag,
                    *entity_id,
                    entity_source_ordinal,
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
