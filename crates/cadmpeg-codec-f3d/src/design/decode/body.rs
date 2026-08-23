// SPDX-License-Identifier: Apache-2.0
//! Parse body members, bounds, bindings, and visibility.

use crate::bytes::{lp_ascii_filtered, lp_utf16_bounded, take_reference};
use crate::container::{role, ContainerScan};
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::RECIPES;
use crate::ids::{self, native_stream};
use crate::layout::indexed_design_record_header as indexed_header;
use crate::records::{
    ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector, DesignBodyBinding,
    DesignBodyBounds, DesignBodyMember, DesignEntityHeader, DESIGN_MODULE_BODY,
};
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::bytes::find_from;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;
use std::collections::{HashMap, HashSet};

/// Decode the `BodiesRoot` member list following the doubled `BodiesRoot`
/// marker in each design `BulkStream` entry in `scan`: each member's entity
/// suffix and flags. The decode is rejected (no members returned for that
/// stream) unless the declared count is fully consumed and immediately
/// followed by a zero byte.
pub fn decode_body_members(scan: &ContainerScan) -> Result<Vec<DesignBodyMember>, CodecError> {
    let mut out = Vec::new();
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&10u32.to_le_bytes());
    prefix.extend_from_slice(b"BodiesRoot");
    prefix.extend_from_slice(&0u16.to_le_bytes());
    prefix.extend_from_slice(&10u32.to_le_bytes());
    prefix.extend_from_slice(b"BodiesRoot");
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
        else {
            continue;
        };
        let count_offset = start + prefix.len();
        let mut view = View::over_retained(bytes);
        if view.seek(count_offset).is_none() {
            continue;
        }
        let Some(count) = view
            .u32_le()
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
        else {
            continue;
        };
        if count > 100_000 {
            continue;
        }
        let mut decoded = Vec::with_capacity(count);
        for _ in 0..count {
            let cursor = view.position();
            if view.u8() != Some(1) {
                decoded.clear();
                break;
            }
            let Some(entity_suffix) = view.u64_le() else {
                decoded.clear();
                break;
            };
            let Some(flags) = view.u16_le() else {
                decoded.clear();
                break;
            };
            decoded.push(DesignBodyMember {
                id: ids::native_design_body_member_id(&entry.name, cursor),
                byte_offset: cursor as u64,
                entity_suffix,
                flags,
            });
        }
        if decoded.len() == count && bytes.get(view.position()) == Some(&0) {
            out.extend(decoded);
        }
    }
    Ok(out)
}

/// Decode the three consecutive indexed records that cache each Design body's
/// axis-aligned model-space bounds.
pub fn decode_body_bounds(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignBodyBounds>, CodecError> {
    let mut out = Vec::new();
    for entity in entities
        .iter()
        .filter(|entity| entity.module.as_deref() == Some(DESIGN_MODULE_BODY))
    {
        let Some(stream) = native_stream(&entity.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = usize::try_from(entity.byte_offset).ok() else {
            continue;
        };
        let end = entities
            .iter()
            .filter(|candidate| {
                native_stream(&candidate.id) == Some(stream)
                    && candidate.byte_offset > entity.byte_offset
            })
            .filter_map(|candidate| usize::try_from(candidate.byte_offset).ok())
            .min()
            .unwrap_or(bytes.len());
        let Ok(record_index) = u32::try_from(entity.entity_suffix) else {
            continue;
        };
        let Some(record_indices) = record_index
            .checked_add(1)
            .zip(record_index.checked_add(2))
            .zip(record_index.checked_add(3))
            .map(|((first, second), third)| [first, second, third])
        else {
            continue;
        };
        let mut record_offsets = Vec::with_capacity(3);
        for wanted in record_indices {
            let matches = indexed_headers_in(bytes, start, end)
                .filter(|(_, record_index)| *record_index == wanted)
                .map(|(offset, _)| offset)
                .collect::<Vec<_>>();
            let [offset] = matches.as_slice() else {
                record_offsets.clear();
                break;
            };
            record_offsets.push(*offset);
        }
        let [first, second, third] = record_offsets.as_slice() else {
            continue;
        };
        if !(first < second && second < third) {
            continue;
        }
        let third_end = next_indexed_record_offset(bytes, third.saturating_add(11))
            .filter(|offset| *offset <= end)
            .unwrap_or(end);
        let intervals = [(*first, *second), (*second, *third), (*third, third_end)];
        let mut repeated = body_bound_candidates(bytes, intervals[0].0, intervals[0].1)
            .filter_map(|(marker_offset, values)| {
                let frame = bytes.get(marker_offset..marker_offset + 49)?;
                let mut value_offsets = [marker_offset + 1, 0, 0];
                for (ordinal, (record_start, record_end)) in
                    intervals.iter().copied().enumerate().skip(1)
                {
                    let matches = body_bound_candidates(bytes, record_start, record_end)
                        .filter(|(offset, _)| {
                            bytes.get(*offset..offset.saturating_add(49)) == Some(frame)
                        })
                        .map(|(offset, _)| offset + 1)
                        .collect::<Vec<_>>();
                    let [offset] = matches.as_slice() else {
                        return None;
                    };
                    value_offsets[ordinal] = *offset;
                }
                Some((values, value_offsets))
            })
            .collect::<Vec<_>>();
        repeated.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
        let [(values, value_offsets)] = repeated.as_slice() else {
            continue;
        };
        out.push(DesignBodyBounds {
            id: ids::native_design_body_bounds_id(&entry.name, entity.byte_offset),
            entity_suffix: entity.entity_suffix,
            entity_byte_offset: entity.byte_offset,
            record_indices,
            record_byte_offsets: [*first as u64, *second as u64, *third as u64],
            value_byte_offsets: value_offsets.map(|offset| offset as u64),
            body_binding_ids: Vec::new(),
            maximum: Point3::new(values[0] * 10.0, values[1] * 10.0, values[2] * 10.0),
            minimum: Point3::new(values[3] * 10.0, values[4] * 10.0, values[5] * 10.0),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn indexed_headers_in(
    bytes: &[u8],
    mut position: usize,
    end: usize,
) -> impl Iterator<Item = (usize, u32)> + '_ {
    std::iter::from_fn(move || {
        while position + 11 <= end {
            let at = position;
            position += 1;
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if class_tag.len() == 3 && class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                let Some(record_index) = View::u32_le_at(bytes, after_tag) else {
                    continue;
                };
                return Some((at, record_index));
            }
        }
        None
    })
}

pub(crate) fn body_bound_candidates(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> impl Iterator<Item = (usize, [f64; 6])> + '_ {
    (start..end.saturating_sub(48)).filter_map(move |offset| {
        if bytes.get(offset) != Some(&1) {
            return None;
        }
        let values = [
            View::f64_le_at(bytes, offset + 1)?,
            View::f64_le_at(bytes, offset + 9)?,
            View::f64_le_at(bytes, offset + 17)?,
            View::f64_le_at(bytes, offset + 25)?,
            View::f64_le_at(bytes, offset + 33)?,
            View::f64_le_at(bytes, offset + 41)?,
        ];
        (values.iter().all(|value| value.is_finite())
            && (0..3).all(|axis| values[axis] >= values[axis + 3])
            && (0..3).any(|axis| values[axis] > values[axis + 3]))
        .then_some((offset, values))
    })
}

pub(crate) fn decode_stream(bytes: &[u8], stream: &str, out: &mut Vec<ConstructionRecipe>) {
    let mut counters: HashMap<(ConstructionRecipeKind, Option<String>), u32> = HashMap::new();
    for &(name, kind) in RECIPES {
        let mut cursor = 0;
        while let Some(offset) = find_from(bytes, name, cursor) {
            cursor = offset + 1;
            if kind == ConstructionRecipeKind::Face
                && offset >= 8
                && &bytes[offset - 8..offset] == b"bounded_"
            {
                continue;
            }
            let framed_name = offset
                .checked_sub(4)
                .and_then(|at| View::u32_le_at(bytes, at))
                .and_then(|length| usize::try_from(length).ok())
                == Some(name.len());
            if !framed_name {
                continue;
            }
            let design_id_field = recipe_design_id(bytes, offset, name);
            let design_id = design_id_field.as_ref().map(|field| field.0.clone());
            let design_selector = design_id_field
                .as_ref()
                .and_then(|(design_id, design_id_at)| {
                    let selector_at = design_id_at.checked_add(design_id.len())?;
                    Some(ConstructionRecipeSelector {
                        value: View::u32_le_at(bytes, selector_at)?,
                        byte_offset: u64::try_from(selector_at).ok()?,
                    })
                });
            let key = (kind, design_id.clone());
            let counter = counters.entry(key).or_default();
            let recipe_index = *counter;
            *counter += 1;
            let record_index_offset = offset.checked_sub(16);
            let record_index = record_index_offset
                .and_then(|at| View::i32_le_at(bytes, at))
                .unwrap_or_default();
            out.push(ConstructionRecipe {
                id: ids::native_construction_recipe_id(stream, offset),
                byte_offset: offset as u64,
                record_index_offset: record_index_offset.map(|offset| offset as u64),
                kind,
                design_id,
                design_id_offset: design_id_field.as_ref().map(|field| field.1 as u64),
                design_selector,
                recipe_index,
                record_index,
            });
        }
    }
    out.sort_by_key(|recipe| recipe.record_index);
}

fn recipe_design_id(bytes: &[u8], offset: usize, name: &[u8]) -> Option<(String, usize)> {
    let id_end = offset.checked_sub(20)?;
    for length in 1..=8usize {
        let Some(length_at) = id_end.checked_sub(4 + length) else {
            continue;
        };
        if let Some((id, value_offset)) = ascii_id_at(bytes, length_at) {
            if value_offset.checked_add(id.len()) == Some(id_end) {
                return Some((id, value_offset));
            }
        }
    }
    if offset >= 23 {
        let candidate = bytes.get(offset - 23..offset - 20)?;
        if candidate.iter().all(u8::is_ascii_digit) {
            return Some((String::from_utf8_lossy(candidate).into_owned(), offset - 23));
        }
    }
    ascii_id_at(bytes, offset + name.len() + 8)
}

fn ascii_id_at(bytes: &[u8], length_offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, length_offset)?).ok()?;
    if !(1..=8).contains(&length) {
        return None;
    }
    let value = bytes.get(length_offset + 4..length_offset + 4 + length)?;
    value.iter().all(u8::is_ascii_alphanumeric).then(|| {
        (
            String::from_utf8_lossy(value).into_owned(),
            length_offset + 4,
        )
    })
}

/// One `(asm_body_key, entity_suffix)` pair from a Design `BulkStream` BREP
/// body-map record, with the named B-rep blob the key resolves in and the
/// suffix's byte offset for native patching.
pub(crate) struct BodyBinding {
    /// Basename of the B-rep blob entry the ASM key resolves in.
    pub blob_name: String,
    /// Byte offset of the BREP blob name's UTF-16LE code units.
    pub blob_name_offset: usize,
    /// Number of pairs in the enclosing map.
    pub pair_count: u32,
    /// Zero-based position in the enclosing map.
    pub pair_ordinal: u32,
    /// The referenced ASM body key.
    pub asm_key: u64,
    /// Byte offset of `asm_key` within the stream.
    pub asm_key_offset: usize,
    /// The body's design-entity suffix.
    pub entity_suffix: u64,
    /// Byte offset of `entity_suffix` within the stream.
    pub entity_suffix_offset: usize,
}

/// One record of the sibling Design carrier that binds an `.smb` snapshot.
pub(crate) struct SnapshotBodyMapRecord {
    pub blob_name: String,
    pub bindings: Vec<BodyBinding>,
}

fn entity_has_type(meta: &crate::metastream::MetaStream, entity: u64, type_guid: &str) -> bool {
    meta.types.iter().any(|design_type| {
        design_type.type_guid.eq_ignore_ascii_case(type_guid)
            && design_type.entity_ids.contains(&entity)
    })
}

struct LocalReferenceCandidate {
    target: u64,
    end: usize,
    inline_type_guid: Option<String>,
}

fn local_reference_candidates(bytes: &[u8], at: usize) -> Vec<LocalReferenceCandidate> {
    let mut candidates = Vec::new();
    let mut end = at;
    if let Some(reference) = take_reference(bytes, &mut end) {
        if reference.segment.is_none() && reference.link_name.is_none() {
            if let Some(target) = reference.target {
                candidates.push(LocalReferenceCandidate {
                    target,
                    end,
                    inline_type_guid: reference.inline_type_guid,
                });
            }
        }
    }
    if at.checked_add(2).and_then(|end| bytes.get(at..end)) == Some(&[1, 1]) {
        if let (Some(target_at), Some(end)) = (at.checked_add(2), at.checked_add(10)) {
            if let Some(target) = View::u64_le_at(bytes, target_at) {
                candidates.push(LocalReferenceCandidate {
                    target,
                    end,
                    inline_type_guid: None,
                });
            }
        }
    }
    candidates
}

fn reference_has_type(
    meta: &crate::metastream::MetaStream,
    reference: &LocalReferenceCandidate,
    expected_type_guid: &str,
) -> bool {
    reference
        .inline_type_guid
        .as_deref()
        .is_none_or(|guid| guid.eq_ignore_ascii_case(expected_type_guid))
        && entity_has_type(meta, reference.target, expected_type_guid)
}

/// Parse every exactly framed sibling body-map record that binds an `.smb`
/// snapshot. The carrier uses a bare entity header in every serializer band.
pub(crate) fn snapshot_body_map_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<SnapshotBodyMapRecord>, CodecError> {
    let frames = crate::metastream::primary_record_frames(meta, bytes.len())?;
    let primary_by_entity = frames
        .iter()
        .enumerate()
        .map(|(ordinal, frame)| (frame.entity_id, ordinal))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for (type_ordinal, design_type) in meta.types.iter().enumerate() {
        if !design_type
            .type_guid
            .eq_ignore_ascii_case(crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID)
        {
            continue;
        }
        if !crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_VERSIONS
            .contains(&design_type.version)
        {
            return Err(CodecError::NotImplemented(format!(
                "unsupported F3D Design snapshot body-map carrier version {}",
                design_type.version
            )));
        }
        if design_type.module != DESIGN_MODULE_BODY
            || !design_type.base_type_guid.as_deref().is_some_and(|base| {
                base.eq_ignore_ascii_case(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID)
            })
        {
            return Err(CodecError::Malformed(
                "F3D Design snapshot body-map carrier has incompatible registration metadata"
                    .into(),
            ));
        }
        let class_tag = u32::try_from(type_ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(256))
            .filter(|tag| *tag <= 999)
            .ok_or_else(|| {
                CodecError::Malformed(
                    "F3D Design snapshot body-map class tag is not three digits".into(),
                )
            })?
            .to_string();
        for &entity in &design_type.entity_ids {
            let Some(&frame_ordinal) = primary_by_entity.get(&entity) else {
                return Err(CodecError::Malformed(format!(
                    "F3D Design snapshot body-map entity {entity} has no primary record"
                )));
            };
            let frame = frames[frame_ordinal];
            if View::u32_le_at(bytes, frame.start) != Some(3)
                || bytes.get(frame.start + 4..frame.start + 7) != Some(class_tag.as_bytes())
                || View::u64_le_at(bytes, frame.start + 7) != Some(entity)
                || bytes.get(frame.start + 15..frame.start + 21) != Some(&[0; 6])
            {
                return Err(CodecError::Malformed(format!(
                    "F3D Design snapshot body-map entity {entity} has an invalid entity header"
                )));
            }
            if let Some(record) =
                parse_snapshot_body_map_frame(bytes, meta, frame.start, frame.end, entity)?
            {
                out.push(record);
            }
        }
    }
    Ok(out)
}

fn parse_snapshot_body_map_frame(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    start: usize,
    end: usize,
    entity: u64,
) -> Result<Option<SnapshotBodyMapRecord>, CodecError> {
    let Some(companion_entity) = entity.checked_add(1) else {
        return Ok(None);
    };
    let Some(companion_at) = start.checked_add(21) else {
        return Ok(None);
    };
    for companion in local_reference_candidates(bytes, companion_at) {
        let Some(reserved_end) = companion.end.checked_add(2) else {
            continue;
        };
        if companion.target != companion_entity
            || !reference_has_type(
                meta,
                &companion,
                crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
            )
            || bytes.get(companion.end..reserved_end) != Some(&[0, 0])
        {
            continue;
        }
        let count_at = reserved_end;
        let Some(pair_count) = View::u32_le_at(bytes, count_at) else {
            continue;
        };
        let count = usize::try_from(pair_count).map_err(|_| {
            CodecError::Malformed("F3D snapshot body-map count exceeds usize".into())
        })?;
        let Some(pairs_start) = count_at.checked_add(4) else {
            continue;
        };
        let Some(pairs_end) = count
            .checked_mul(16)
            .and_then(|span| pairs_start.checked_add(span))
        else {
            continue;
        };
        if bytes.get(pairs_start..pairs_end).is_none() {
            continue;
        }
        if (0..count).any(|pair| {
            View::u64_le_at(bytes, pairs_start + pair * 16 + 8).is_none_or(|body_entity| {
                !entity_has_type(
                    meta,
                    body_entity,
                    crate::design::body::SNAPSHOT_BODY_RECORD_TYPE_GUID,
                )
            })
        }) {
            continue;
        }
        for container in local_reference_candidates(bytes, pairs_end) {
            let Some(reserved_end) = container.end.checked_add(3) else {
                continue;
            };
            if !reference_has_type(
                meta,
                &container,
                crate::design::body::SNAPSHOT_BODY_CONTAINER_TYPE_GUID,
            ) || bytes.get(container.end..reserved_end) != Some(&[0, 0, 0])
            {
                continue;
            }
            let name_at = reserved_end;
            let Some(max_chars) = name_at
                .checked_add(4)
                .and_then(|payload| end.checked_sub(payload))
                .map(|remaining| remaining / 2)
            else {
                continue;
            };
            let Some((blob_name, name_end)) = lp_utf16_bounded(bytes, name_at, 0..=max_chars)
            else {
                continue;
            };
            if name_end != end
                || (!blob_name.is_empty()
                    && (!is_brep_blob_basename(&blob_name) || !blob_name.ends_with(".smb")))
                || (blob_name.is_empty() && pair_count != 0)
            {
                continue;
            }
            let mut bindings = Vec::new();
            bindings.try_reserve(count).map_err(|_| {
                CodecError::Malformed("F3D snapshot body-map count exceeds capacity".into())
            })?;
            for pair in 0..count {
                let at = pairs_start + pair * 16;
                bindings.push(BodyBinding {
                    blob_name: blob_name.clone(),
                    blob_name_offset: name_at + 4,
                    pair_count,
                    pair_ordinal: u32::try_from(pair).expect("pair ordinal is below its u32 count"),
                    asm_key: View::u64_le_at(bytes, at).expect("validated pair extent"),
                    asm_key_offset: at,
                    entity_suffix: View::u64_le_at(bytes, at + 8).expect("validated pair extent"),
                    entity_suffix_offset: at + 8,
                });
            }
            return Ok(Some(SnapshotBodyMapRecord {
                blob_name,
                bindings,
            }));
        }
    }
    Ok(None)
}

/// Parse every exactly indexed BREP body-map record in a Design `BulkStream`.
///
/// The type GUID names a family with more than one record frame. The
/// `MetaStream` entity list and primary record index select exact candidate
/// extents. A candidate is a body map only when one supported reserved-zero
/// width makes its count, pair run, tail, and basename consume that extent.
pub(crate) fn body_bindings(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyBinding>, CodecError> {
    let record_frames = crate::metastream::primary_record_frames(meta, bytes.len())?;

    let mut primary_by_entity = HashMap::<u64, Option<usize>>::new();
    for (ordinal, record) in meta.records.iter().enumerate() {
        primary_by_entity
            .entry(record.entity_id)
            .and_modify(|record_ordinal| *record_ordinal = None)
            .or_insert(Some(ordinal));
    }

    let mut out = Vec::new();
    let mut typed_entities = HashSet::new();
    for (type_ordinal, design_type) in meta.types.iter().enumerate() {
        if !design_type
            .type_guid
            .eq_ignore_ascii_case(crate::design::body::BODY_MAP_CARRIER_TYPE_GUID)
        {
            continue;
        }
        if design_type.version != crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION {
            return Err(CodecError::NotImplemented(format!(
                "unsupported F3D Design body-map carrier version {}",
                design_type.version
            )));
        }
        if design_type.module != DESIGN_MODULE_BODY
            || !design_type.base_type_guid.as_deref().is_some_and(|base| {
                base.eq_ignore_ascii_case(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID)
            })
        {
            return Err(CodecError::Malformed(
                "F3D Design body-map carrier type has incompatible registration metadata".into(),
            ));
        }
        let class_tag = u32::try_from(type_ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(256))
            .filter(|class_tag| *class_tag <= 999)
            .ok_or_else(|| {
                CodecError::Malformed(
                    "F3D Design body-map carrier class tag is not three digits".into(),
                )
            })?;
        let class_tag = class_tag.to_string();

        for &entity_id in &design_type.entity_ids {
            if !typed_entities.insert(entity_id) {
                return Err(CodecError::Malformed(format!(
                    "F3D Design body-map carrier entity {entity_id} is registered more than once"
                )));
            }
            let record_ordinal = match primary_by_entity.get(&entity_id) {
                Some(Some(record_ordinal)) => *record_ordinal,
                Some(None) => {
                    return Err(CodecError::Malformed(format!(
                    "F3D Design body-map carrier entity {entity_id} has multiple primary records"
                )))
                }
                None => {
                    return Err(CodecError::Malformed(format!(
                        "F3D Design body-map carrier entity {entity_id} has no primary record"
                    )))
                }
            };
            let frame = record_frames[record_ordinal];
            let start = frame.start;
            let end = frame.end;
            let record_index = u32::try_from(entity_id).map_err(|_| {
                CodecError::Malformed(format!(
                    "F3D Design body-map carrier entity {entity_id} exceeds u32"
                ))
            })?;
            if View::u32_le_at(bytes, start) != Some(3)
                || bytes
                    .get(start + indexed_header::CLASS_TAG..start + indexed_header::RECORD_INDEX)
                    != Some(class_tag.as_bytes())
                || View::u32_le_at(bytes, start + indexed_header::RECORD_INDEX)
                    != Some(record_index)
            {
                return Err(CodecError::Malformed(format!(
                    "F3D Design body-map carrier entity {entity_id} has an invalid indexed header"
                )));
            }

            let mut matched = None;
            for prefix_len in crate::design::body::BODY_MAP_ZERO_PREFIX_LENGTHS {
                let Some(bindings) = parse_body_map_frame(bytes, start, end, prefix_len)? else {
                    continue;
                };
                if matched.replace(bindings).is_some() {
                    return Err(CodecError::Malformed(format!(
                        "F3D Design body-map carrier entity {entity_id} has an ambiguous frame"
                    )));
                }
            }
            if let Some(bindings) = matched {
                out.extend(bindings);
            }
        }
    }
    Ok(out)
}

fn parse_body_map_frame(
    bytes: &[u8],
    start: usize,
    end: usize,
    prefix_len: usize,
) -> Result<Option<Vec<BodyBinding>>, CodecError> {
    let Some(count_at) = start
        .checked_add(indexed_header::LEN)
        .and_then(|payload| payload.checked_add(prefix_len))
    else {
        return Ok(None);
    };
    if !bytes
        .get(start + indexed_header::LEN..count_at)
        .is_some_and(|prefix| prefix.iter().all(|byte| *byte == 0))
    {
        return Ok(None);
    }
    let Some(pair_count) = View::u32_le_at(bytes, count_at) else {
        return Ok(None);
    };
    let count = usize::try_from(pair_count).map_err(|_| {
        CodecError::Malformed(format!(
            "F3D Design body map at byte {start} pair count does not fit this platform"
        ))
    })?;
    let Some(pairs_start) = count_at.checked_add(4) else {
        return Ok(None);
    };
    let Some(pairs_end) = count
        .checked_mul(16)
        .and_then(|span| pairs_start.checked_add(span))
    else {
        return Ok(None);
    };
    let Some(name_at) = pairs_end.checked_add(12) else {
        return Ok(None);
    };
    if View::u64_le_at(bytes, pairs_end).is_none()
        || View::u32_le_at(bytes, pairs_end + 8) != Some(0)
    {
        return Ok(None);
    }
    let Some(max_name_chars) = name_at
        .checked_add(4)
        .and_then(|payload| end.checked_sub(payload))
        .map(|remaining| remaining / 2)
    else {
        return Ok(None);
    };
    let Some((blob_name, _name_end)) =
        lp_utf16_bounded(bytes, name_at, 0..=max_name_chars).filter(|(name, name_end)| {
            *name_end == end
                && ((pair_count == 0 && name.is_empty())
                    || (pair_count > 0 && is_brep_blob_basename(name)))
        })
    else {
        return Ok(None);
    };

    let mut bindings = Vec::new();
    bindings.try_reserve(count).map_err(|_| {
        CodecError::Malformed(format!(
            "F3D Design body map at byte {start} pair count exceeds decoder capacity"
        ))
    })?;
    for pair in 0..count {
        let at = pairs_start + pair * 16;
        let (Some(key), Some(suffix)) =
            (View::u64_le_at(bytes, at), View::u64_le_at(bytes, at + 8))
        else {
            return Err(CodecError::Malformed(format!(
                "F3D Design body map at byte {start} has a truncated pair run"
            )));
        };
        bindings.push(BodyBinding {
            blob_name: blob_name.clone(),
            blob_name_offset: name_at + 4,
            pair_count,
            pair_ordinal: u32::try_from(pair).expect("pair ordinal is below its u32 pair count"),
            asm_key: key,
            asm_key_offset: at,
            entity_suffix: suffix,
            entity_suffix_offset: at + 8,
        });
    }
    Ok(Some(bindings))
}

fn is_brep_blob_basename(value: &str) -> bool {
    let extension = value.rsplit_once('.').map(|(_, extension)| extension);
    value.starts_with("BREP.")
        && matches!(extension, Some("smb" | "smbh"))
        && !value.contains(['/', '\\'])
}

/// Decode every ordered Design BREP body-map pair and resolve each pair in its
/// named blob's body-selector namespace.
pub fn decode_design_body_bindings(
    scan: &ContainerScan,
    active_brep_entry: Option<&str>,
    body_keys: &[BodyNativeKey],
) -> Result<Vec<DesignBodyBinding>, CodecError> {
    let active_basename = active_brep_entry.and_then(|entry| entry.rsplit('/').next());
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        for binding in body_bindings(bytes, &metadata)? {
            let source_bodies = body_keys
                .iter()
                .filter(|key| {
                    key.source_brep.as_deref().map_or_else(
                        || active_basename == Some(binding.blob_name.as_str()),
                        |source| source == binding.blob_name,
                    )
                })
                .collect::<Vec<_>>();
            let body = crate::brep::resolve_body_selector(&source_bodies, binding.asm_key)?;
            out.push(DesignBodyBinding {
                id: ids::native_design_body_binding_id(&entry.name, binding.asm_key_offset),
                stream: entry.name.clone(),
                pair_count: binding.pair_count,
                pair_ordinal: binding.pair_ordinal,
                asm_body_key: binding.asm_key,
                asm_body_key_offset: binding.asm_key_offset as u64,
                entity_suffix: binding.entity_suffix,
                entity_suffix_offset: binding.entity_suffix_offset as u64,
                blob_name: binding.blob_name,
                blob_name_offset: binding.blob_name_offset as u64,
                body,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Bind each body cache to every BREP map pair carrying the same Design entity
/// suffix in the same stream.
pub fn bind_body_bounds(bounds: &mut [DesignBodyBounds], bindings: &[DesignBodyBinding]) {
    for bounds in bounds {
        let Some(stream) = native_stream(&bounds.id) else {
            continue;
        };
        let mut matches = bindings
            .iter()
            .filter(|binding| {
                stream == ids::native_scope(&binding.stream)
                    && binding.entity_suffix == bounds.entity_suffix
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|binding| binding.asm_body_key_offset);
        bounds.body_binding_ids = matches
            .into_iter()
            .map(|binding| binding.id.clone())
            .collect();
    }
}

/// Decode per-body display visibility from the Design `BulkStream`.
///
/// Each BREP body-map record resolves blob-qualified body selectors to Design
/// entity suffixes, and each entity's browser-node record carries a hidden flag
/// directly after the node GUID
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
/// The result maps each blob and body selector to its display visibility;
/// bodies without records are absent.
#[derive(Debug, Clone)]
pub(crate) struct DecodedBodyVisibility {
    pub stream: String,
    pub byte_offset: u64,
    pub asm_body_key_offset: u64,
    pub entity_suffix: u64,
    pub visible: bool,
}

pub(crate) fn decode_all_body_visibility(
    scan: &ContainerScan,
) -> Result<HashMap<(String, u64), DecodedBodyVisibility>, CodecError> {
    let mut out = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let hidden_by_entity = typed_browser_node_hidden_flags(bytes, &metadata)?;
        for binding in body_bindings(bytes, &metadata)? {
            if let Some(node) = hidden_by_entity.get(&binding.entity_suffix) {
                out.insert(
                    (binding.blob_name, binding.asm_key),
                    DecodedBodyVisibility {
                        stream: entry.name.clone(),
                        byte_offset: node.byte_offset,
                        asm_body_key_offset: binding.asm_key_offset as u64,
                        entity_suffix: binding.entity_suffix,
                        visible: !node.hidden,
                    },
                );
            }
        }
    }
    Ok(out)
}

/// Visibility selected from one typed browser-node record.
#[derive(Debug, Clone, Copy)]
struct BrowserNodeVisibility {
    byte_offset: u64,
    hidden: bool,
}

fn typed_browser_node_hidden_flags(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<HashMap<u64, BrowserNodeVisibility>, CodecError> {
    let nodes = crate::design::decode::presentation::browser_node_records(bytes, meta)?;
    let presentations = crate::design::decode::presentation::body_presentations(bytes, meta)?;
    let mut nodes_by_entity = HashMap::<u64, Vec<_>>::new();
    for node in &nodes {
        nodes_by_entity
            .entry(node.entity_suffix)
            .or_default()
            .push(node);
    }

    let mut out = HashMap::new();
    for (entity_suffix, candidates) in nodes_by_entity {
        let mut linked = presentations
            .iter()
            .filter(|presentation| presentation.entity_suffix == entity_suffix)
            .filter_map(|presentation| presentation.browser_node.as_ref())
            .collect::<Vec<_>>();
        linked.sort_by_key(|node| node.record_index);
        linked.dedup_by_key(|node| node.record_index);
        let selected = match linked.as_slice() {
            [node] => Some(*node),
            [] => match candidates.as_slice() {
                [node] => Some(*node),
                _ => None,
            },
            _ => None,
        };
        if let Some(node) = selected {
            out.insert(
                entity_suffix,
                BrowserNodeVisibility {
                    byte_offset: node.hidden_offset,
                    hidden: node.hidden,
                },
            );
        }
    }
    Ok(out)
}

/// Map each browser-node GUID to its Design entity suffix.
///
/// The GUID is the stable join between browser presentation records; the
/// adjacent entity suffix joins the node back to the Design body map.
pub(crate) fn browser_node_entities(bytes: &[u8]) -> HashMap<String, u64> {
    let mut entities = HashMap::new();
    let mut ambiguous = std::collections::HashSet::new();
    for record in browser_node_records(bytes) {
        let key = record.guid.to_ascii_lowercase();
        if entities
            .insert(key.clone(), record.entity_suffix)
            .is_some_and(|previous| previous != record.entity_suffix)
        {
            ambiguous.insert(key);
        }
    }
    entities.retain(|guid, _| !ambiguous.contains(guid));
    entities
}

#[derive(Debug, Clone)]
struct BrowserNodeRecord {
    guid: String,
    entity_suffix: u64,
}

fn browser_node_records(bytes: &[u8]) -> Vec<BrowserNodeRecord> {
    const GUID_CHARS: usize = 36;
    const GUID_BYTES: usize = GUID_CHARS * 2;
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 4 + GUID_BYTES + 3 + 8 <= bytes.len() {
        if View::u32_le_at(bytes, at) != Some(GUID_CHARS as u32)
            || !is_utf16_guid(&bytes[at + 4..at + 4 + GUID_BYTES])
        {
            at += 1;
            continue;
        }
        let flag_at = at + 4 + GUID_BYTES;
        if bytes.get(flag_at + 1..flag_at + 3) == Some(&[0x01, 0x01]) {
            if let (0 | 1, Some(member)) = (bytes[flag_at], View::u64_le_at(bytes, flag_at + 3)) {
                out.push(BrowserNodeRecord {
                    guid: utf16_le_string(&bytes[at + 4..at + 4 + GUID_BYTES]),
                    entity_suffix: member,
                });
            }
        }
        at += 1;
    }
    out
}

fn utf16_le_string(bytes: &[u8]) -> String {
    let mut view = View::over_retained(bytes);
    let mut units = Vec::with_capacity(bytes.len() / 2);
    while let Some(unit) = view.u16_le() {
        units.push(unit);
    }
    String::from_utf16_lossy(&units)
}

fn is_utf16_guid(bytes: &[u8]) -> bool {
    bytes
        .chunks_exact(2)
        .all(|pair| pair[1] == 0 && (pair[0].is_ascii_hexdigit() || pair[0] == b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::lp_utf16_bytes;
    use crate::design::presentation::{
        APPEARANCE_LIBRARY_ID, BODY_PRESENTATION_BASE_TYPE_GUID, BODY_PRESENTATION_TYPE_GUID,
        BODY_PRESENTATION_TYPE_VERSION, BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION,
        BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION, BROWSER_NODE_BASE_TYPE_GUID,
        BROWSER_NODE_TYPE_GUID, BROWSER_NODE_TYPE_VERSION, PHYSICAL_MATERIAL_LIBRARY_ID,
    };
    use crate::records::DESIGN_MODULE_FUSION;

    fn push_indexed_header(out: &mut Vec<u8>, class_tag: &str, record_index: u32) {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(class_tag.as_bytes());
        out.extend_from_slice(&record_index.to_le_bytes());
    }

    fn push_entity_header(out: &mut Vec<u8>, class_tag: &str, entity: u64) {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(class_tag.as_bytes());
        out.extend_from_slice(&entity.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend(lp_utf16_bytes(&format!("0_{entity}")));
    }

    fn push_reference(out: &mut Vec<u8>, target: u64) {
        out.push(1);
        out.extend_from_slice(&target.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
    }

    fn presentation_type(
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

    fn body_map_bytes(prefix_len: usize, declared_count: u32, pairs: &[(u64, u64)]) -> Vec<u8> {
        let mut out = Vec::new();
        push_indexed_header(&mut out, "256", 900);
        out.extend(std::iter::repeat_n(0, prefix_len));
        out.extend_from_slice(&declared_count.to_le_bytes());
        for (key, suffix) in pairs {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&suffix.to_le_bytes());
        }
        out.extend_from_slice(&1793u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend(lp_utf16_bytes(if declared_count == 0 {
            ""
        } else {
            "BREP.synthetic.smbh"
        }));
        out
    }

    fn body_map_metadata() -> crate::metastream::MetaStream {
        crate::metastream::MetaStream {
            types: vec![crate::records::SegmentType {
                id: String::new(),
                byte_offset: 0,
                type_guid: crate::design::body::BODY_MAP_CARRIER_TYPE_GUID.into(),
                type_guid_offset: 0,
                base_type_guid: Some(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID.into()),
                base_type_guid_offset: Some(0),
                version: crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                version_offset: 0,
                module: DESIGN_MODULE_BODY.into(),
                entity_ids: vec![900],
                entity_id_offsets: vec![0],
            }],
            records: vec![crate::metastream::RecordIndexEntry {
                entity_id: 900,
                bulk_offset: 0,
            }],
            secondary_records: Vec::new(),
        }
    }

    fn push_snapshot_reference(out: &mut Vec<u8>, target: u64, target_type: &str, form: u8) {
        match form {
            0 => push_reference(out, target),
            1 => {
                out.push(1);
                out.extend_from_slice(&target.to_le_bytes());
                out.extend_from_slice(&(target_type.len() as u32).to_le_bytes());
                out.extend_from_slice(target_type.as_bytes());
                out.extend_from_slice(&[0, 0]);
            }
            2 => {
                out.extend_from_slice(&[1, 1]);
                out.extend_from_slice(&target.to_le_bytes());
            }
            _ => unreachable!(),
        }
    }

    fn snapshot_body_map_bytes_with(
        form: u8,
        pair_count: u32,
        blob_name: &str,
        companion_type: &str,
    ) -> Vec<u8> {
        let entity = 900u64;
        let mut out = Vec::new();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(b"256");
        out.extend_from_slice(&entity.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        push_snapshot_reference(&mut out, entity + 1, companion_type, form);
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&pair_count.to_le_bytes());
        if pair_count != 0 {
            out.extend_from_slice(&7u64.to_le_bytes());
            out.extend_from_slice(&500u64.to_le_bytes());
        }
        push_snapshot_reference(
            &mut out,
            700,
            crate::design::body::SNAPSHOT_BODY_CONTAINER_TYPE_GUID,
            form,
        );
        out.extend_from_slice(&[0, 0, 0]);
        out.extend(lp_utf16_bytes(blob_name));
        out
    }

    fn snapshot_body_map_bytes(form: u8) -> Vec<u8> {
        snapshot_body_map_bytes_with(
            form,
            1,
            "BREP.snapshot.smb",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        )
    }

    fn snapshot_body_map_metadata() -> crate::metastream::MetaStream {
        crate::metastream::MetaStream {
            types: vec![
                presentation_type(
                    crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID,
                    Some(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID),
                    1,
                    DESIGN_MODULE_BODY,
                    vec![900],
                ),
                presentation_type(
                    crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![901],
                ),
                presentation_type(
                    crate::design::body::SNAPSHOT_BODY_RECORD_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![500],
                ),
                presentation_type(
                    crate::design::body::SNAPSHOT_BODY_CONTAINER_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![700],
                ),
            ],
            records: vec![primary_record(900, 0)],
            secondary_records: Vec::new(),
        }
    }

    #[test]
    fn snapshot_body_map_accepts_every_reference_envelope() {
        for form in 0..=2 {
            let records = snapshot_body_map_records(
                &snapshot_body_map_bytes(form),
                &snapshot_body_map_metadata(),
            )
            .expect("typed snapshot body map");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
            assert_eq!(records[0].bindings.len(), 1);
            assert_eq!(records[0].bindings[0].asm_key, 7);
            assert_eq!(records[0].bindings[0].entity_suffix, 500);
        }
    }

    #[test]
    fn snapshot_body_map_requires_typed_pair_targets() {
        let mut metadata = snapshot_body_map_metadata();
        metadata.types[2].entity_ids.clear();
        assert!(
            snapshot_body_map_records(&snapshot_body_map_bytes(0), &metadata)
                .expect("typed carrier")
                .is_empty()
        );
    }

    #[test]
    fn snapshot_body_map_retains_named_zero_pair_blob() {
        let bytes = snapshot_body_map_bytes_with(
            0,
            0,
            "BREP.snapshot.smb",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        );
        let records = snapshot_body_map_records(&bytes, &snapshot_body_map_metadata())
            .expect("named zero-pair snapshot body map");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
        assert!(records[0].bindings.is_empty());
    }

    #[test]
    fn snapshot_body_map_accepts_empty_zero_pair_record() {
        let bytes = snapshot_body_map_bytes_with(
            0,
            0,
            "",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        );
        let records = snapshot_body_map_records(&bytes, &snapshot_body_map_metadata())
            .expect("empty zero-pair snapshot body map");
        assert_eq!(records.len(), 1);
        assert!(records[0].blob_name.is_empty());
        assert!(records[0].bindings.is_empty());
    }

    #[test]
    fn snapshot_body_map_rejects_contradictory_inline_type() {
        let bytes = snapshot_body_map_bytes_with(
            1,
            1,
            "BREP.snapshot.smb",
            crate::design::body::SNAPSHOT_BODY_CONTAINER_TYPE_GUID,
        );
        assert!(
            snapshot_body_map_records(&bytes, &snapshot_body_map_metadata())
                .expect("typed carrier")
                .is_empty()
        );
    }

    #[test]
    fn body_map_count_is_bounded_by_the_stream_not_sixty_four_pairs() {
        let pairs = (0u64..65)
            .map(|ordinal| (1000 + ordinal, (1u64 << 40) + ordinal))
            .collect::<Vec<_>>();
        let bindings = body_bindings(&body_map_bytes(10, 65, &pairs), &body_map_metadata())
            .expect("65-pair body map");
        assert_eq!(bindings.len(), 65);
        assert!(bindings.iter().all(|binding| binding.pair_count == 65));
        assert_eq!(bindings[0].pair_ordinal, 0);
        assert_eq!(bindings[64].pair_ordinal, 64);
        assert_eq!(bindings[64].asm_key, 1064);
        assert_eq!(bindings[64].entity_suffix, (1u64 << 40) + 64);
    }

    #[test]
    fn both_empty_body_map_prefixes_have_no_pairs_or_brep_basename() {
        for prefix_len in crate::design::body::BODY_MAP_ZERO_PREFIX_LENGTHS {
            let bytes = body_map_bytes(prefix_len, 0, &[]);
            let frame = parse_body_map_frame(&bytes, 0, bytes.len(), prefix_len)
                .expect("empty body-map frame")
                .expect("supported empty body-map variant");
            assert!(frame.is_empty());
            assert!(body_bindings(&bytes, &body_map_metadata())
                .expect("empty typed body map")
                .is_empty());
        }
    }

    #[test]
    fn body_map_header_prevents_a_high_word_count_alias() {
        let bytes = body_map_bytes(10, 2, &[(10, (1u64 << 32) + 77), (20, 30)]);
        let bindings =
            body_bindings(&bytes, &body_map_metadata()).expect("typed two-pair body map");
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].entity_suffix, (1u64 << 32) + 77);
        assert_eq!(bindings[1].asm_key, 20);
    }

    #[test]
    fn truncated_body_map_frame_is_not_decoded() {
        let bytes = body_map_bytes(10, 2, &[(10, 20)]);
        assert!(body_bindings(&bytes, &body_map_metadata())
            .expect("typed carrier record")
            .is_empty());
    }

    #[test]
    fn body_map_parser_does_not_scan_an_unindexed_nested_header() {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, "256", 900);
        bytes.extend_from_slice(&[0xff; 4]);
        bytes.extend(body_map_bytes(10, 1, &[(10, 20)]));

        assert!(body_bindings(&bytes, &body_map_metadata())
            .expect("outer typed carrier record")
            .is_empty());
    }

    fn push_browser_node(
        out: &mut Vec<u8>,
        record_index: u32,
        guid: &str,
        hidden: bool,
        entity: u64,
    ) -> u64 {
        push_indexed_header(out, "257", record_index);
        out.extend_from_slice(&[0; 10]);
        out.extend(lp_utf16_bytes(guid));
        let hidden_offset = out.len() as u64;
        out.push(u8::from(hidden));
        out.extend_from_slice(&[1, 1]);
        out.extend_from_slice(&entity.to_le_bytes());
        hidden_offset
    }

    #[test]
    fn presentation_guid_selects_visibility_when_suffix_repeats() {
        let entity = 42u64;
        let selected_guid = "11111111-2222-8333-A444-555555555555";
        let competing_guid = "AAAAAAAA-BBBB-8CCC-9DDD-EEEEEEEEEEEE";
        let mut bytes = Vec::new();
        push_entity_header(&mut bytes, "256", entity);
        bytes.extend(lp_utf16_bytes(selected_guid));
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend(lp_utf16_bytes("99999999-8888-8777-A666-555555555555"));
        bytes.extend(lp_utf16_bytes(PHYSICAL_MATERIAL_LIBRARY_ID));
        bytes.extend(lp_utf16_bytes("PrismMaterial-001"));
        push_reference(&mut bytes, 7);
        bytes.push(0);
        push_reference(&mut bytes, entity + 1);
        bytes.extend(lp_utf16_bytes("Body"));
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend(lp_utf16_bytes("12345678-1234-8234-A234-123456789ABC"));
        bytes.extend(lp_utf16_bytes(APPEARANCE_LIBRARY_ID));
        let selected_start = bytes.len();
        let selected_offset = push_browser_node(&mut bytes, 100, selected_guid, false, entity);
        let competing_start = bytes.len();
        push_browser_node(&mut bytes, 101, competing_guid, true, entity);

        let meta = crate::metastream::MetaStream {
            types: vec![
                presentation_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                presentation_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100, 101],
                ),
                presentation_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                presentation_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![
                primary_record(entity, 0),
                primary_record(100, selected_start),
                primary_record(101, competing_start),
            ],
            secondary_records: Vec::new(),
        };
        let visibility =
            typed_browser_node_hidden_flags(&bytes, &meta).expect("typed presentation graph");
        let selected = visibility.get(&entity).expect("presentation-selected node");
        assert_eq!(selected.byte_offset, selected_offset);
        assert!(!selected.hidden);

        let mut nodes_only = Vec::new();
        let selected_start = nodes_only.len();
        push_browser_node(&mut nodes_only, 100, selected_guid, false, entity);
        let competing_start = nodes_only.len();
        push_browser_node(&mut nodes_only, 101, competing_guid, true, entity);
        let meta = crate::metastream::MetaStream {
            types: vec![
                presentation_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    Vec::new(),
                ),
                presentation_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100, 101],
                ),
            ],
            records: vec![
                primary_record(100, selected_start),
                primary_record(101, competing_start),
            ],
            secondary_records: Vec::new(),
        };
        let visibility =
            typed_browser_node_hidden_flags(&nodes_only, &meta).expect("typed browser nodes");
        assert!(
            !visibility.contains_key(&entity),
            "two unjoined typed nodes are ambiguous"
        );
    }

    #[test]
    fn body_bound_candidate_has_one_marker_and_six_ordered_f64_values() {
        let values: [f64; 6] = [4.0, 6.0, 1.5, -1.0, 0.0, -0.25];
        let mut bytes = vec![1];
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let candidates = body_bound_candidates(&bytes, 0, bytes.len()).collect::<Vec<_>>();
        assert_eq!(candidates, [(0, values)]);

        bytes[0] = 0;
        assert!(body_bound_candidates(&bytes, 0, bytes.len())
            .next()
            .is_none());
    }

    #[test]
    fn bounded_face_record_identity_is_not_a_second_design_id() {
        let mut bytes = Vec::new();
        for _ in 0..2 {
            let mut prefix = [0u8; 27];
            prefix[11..15].copy_from_slice(&309i32.to_le_bytes());
            prefix[23..27].copy_from_slice(&24u32.to_le_bytes());
            bytes.extend_from_slice(&prefix);
            bytes.extend_from_slice(b"bounded_face_recipe_data");
            bytes.extend_from_slice(&(-1i64).to_le_bytes());
        }
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(&bytes, "Design/BulkStream.dat", &mut recipes);
        assert_eq!(recipes.len(), 2);
        assert!(recipes.iter().all(|recipe| recipe.record_index == 309));
        assert!(recipes.iter().all(|recipe| recipe.design_id.is_none()));
        assert_eq!(recipes[0].recipe_index, 0);
        assert_eq!(recipes[1].recipe_index, 1);

        let mut body = Vec::new();
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(b"2265");
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&[0; 12]);
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(b"body_recipe_data");
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(&body, "Design/BulkStream.dat", &mut recipes);
        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].design_id.as_deref(), Some("2265"));
        assert_eq!(recipes[0].design_id_offset, Some(4));
        assert_eq!(
            recipes[0].design_selector,
            Some(crate::records::ConstructionRecipeSelector {
                value: 3,
                byte_offset: 8,
            })
        );
    }
}
