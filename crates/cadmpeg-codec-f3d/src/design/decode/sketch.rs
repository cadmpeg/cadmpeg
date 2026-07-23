// SPDX-License-Identifier: Apache-2.0
//! Parse sketch placements, objects, headers, relations, and geometry.

use crate::bytes::{is_guid_relaxed, lp_ascii_filtered, lp_utf16_bounded};
use crate::container::{role, ContainerScan};
use crate::design::decode::body::object_kind;
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use crate::records::{
    DesignEntityHeader, DesignObject, DesignObjectKind, DesignParameterScope, DesignRecordHeader,
    DesignSketchPlacement, LostEdgeReference, PersistentReference, PersistentReferenceKind,
    SketchConstraintKind, SketchCurveGeometry, SketchCurveIdentity, SketchPoint, SketchRelation,
    SketchRelationOperand, SketchSurface, SketchText,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::le::{f64_at, f64s_at, u32_at, u64_at as read_u64, utf16le_at};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::HashMap;

/// Decode the unique local-to-model placement frame referenced by every
/// parameter-owning sketch scope, and every member-run head placement. A
/// localized Sketch scope follows its entity container within the same
/// stream interval even though its generic reference table does not repeat
/// the entity suffix.
pub fn decode_sketch_placements(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignSketchPlacement>, CodecError> {
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Sketch))
    {
        let (Some(entity_id), Some(entity_suffix)) =
            (scope.entity_id.as_deref(), scope.entity_suffix)
        else {
            continue;
        };
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && scope.id.starts_with(&ids::native_scope_prefix(&entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let start = usize::try_from(scope.byte_offset).ok();
        let end = usize::try_from(scope.paired_byte_offset).ok();
        let Some(frame) = start
            .zip(end)
            .and_then(|(start, end)| bytes.get(start..end))
        else {
            continue;
        };
        let mut referenced_indices = Vec::new();
        for window in frame.windows(11) {
            if window[0] == 1 && window[5..11] == [0; 6] {
                let record_index = u32::from_le_bytes([window[1], window[2], window[3], window[4]]);
                if !referenced_indices.contains(&record_index) {
                    referenced_indices.push(record_index);
                }
            }
        }
        let mut candidates = Vec::new();
        for record_index in referenced_indices {
            candidates.extend(parse_sketch_placement_candidates(
                bytes,
                scope.record_index,
                entity_id,
                entity_suffix,
                record_index,
            ));
        }
        if candidates.len() == 1 {
            let Some(mut placement) = candidates.pop() else {
                continue;
            };
            placement.id =
                ids::native_design_sketch_placement_id(&entry.name, placement.byte_offset);
            out.push(placement);
        }
    }
    // A sketch entity header pairs with a same-index member-run record whose
    // leading marked reference names a head record carrying the row-major
    // 4×4 placement. A localized Sketch scope belongs to the preceding sketch
    // entity interval: it follows that entity and precedes the next sketch
    // entity in the same stream. Some member-run sketches have no scope.
    let placed = out
        .iter()
        .filter_map(|placement| {
            Some((
                native_stream(&placement.id)?.to_owned(),
                placement.entity_suffix,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    for entity in entities
        .iter()
        .filter(|entity| entity.object_kind == Some(DesignObjectKind::Sketch))
    {
        let Some(stream) = native_stream(&entity.id) else {
            continue;
        };
        if placed.contains(&(stream.to_owned(), entity.entity_suffix)) {
            continue;
        }
        let Some(entry_name) = stream.strip_prefix(ids::SCHEME_PREFIX) else {
            continue;
        };
        let bytes = scan.entry_bytes(entry_name)?;
        let Some(mut placement) = parse_member_run_head_placement(bytes, entity) else {
            continue;
        };
        let next_entity_offset = entities
            .iter()
            .filter(|candidate| {
                candidate.object_kind == Some(DesignObjectKind::Sketch)
                    && native_stream(&candidate.id) == Some(stream)
                    && candidate.byte_offset > entity.byte_offset
            })
            .map(|candidate| candidate.byte_offset)
            .min();
        let matching_scopes = scopes
            .iter()
            .filter(|scope| {
                design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Sketch)
                    && native_stream(&scope.id) == Some(stream)
                    && scope.byte_offset > entity.byte_offset
                    && next_entity_offset.is_none_or(|end| scope.byte_offset < end)
            })
            .collect::<Vec<_>>();
        if let [scope] = matching_scopes.as_slice() {
            placement.scope_record_index = Some(scope.record_index);
        }
        placement.id = ids::native_design_sketch_placement_id(entry_name, placement.byte_offset);
        out.push(placement);
    }
    out.sort_by_key(|placement| placement.id.clone());
    Ok(out)
}

/// Byte length of a member-run head carrying an explicit 4×4 transform.
pub(crate) const MEMBER_RUN_HEAD_FRAME: usize = 162;

/// Parse a member-run head placement: the paired same-index record after the
/// sketch's entity header opens with a marked
/// reference naming a head record. A 34-byte head denotes the identity
/// placement. A 162-byte head stores eleven zero bytes and the row-major 4×4
/// local-to-model transform at offset 22.
pub(crate) fn parse_member_run_head_placement(
    bytes: &[u8],
    entity: &DesignEntityHeader,
) -> Option<DesignSketchPlacement> {
    let start = usize::try_from(entity.byte_offset).ok()?;
    // Locate the paired same-index record after the entity header.
    let mut position = start + 1;
    let paired_at = loop {
        let at = next_indexed_record_offset(bytes, position)?;
        if u32_at(bytes, at + 7).map(u64::from) == Some(entity.entity_suffix) && at > start {
            break at;
        }
        position = at + 1;
    };
    let (paired_class_tag, paired_after_tag) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    if paired_class_tag.len() != 3 || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // The paired record's prologue: the u32 index, zero bytes to offset 19,
    // then a marked u64 reference naming the head record.
    if paired_after_tag != paired_at + 7
        || bytes.get(paired_at + 11..paired_at + 19) != Some(&[0u8; 8][..])
        || bytes.get(paired_at + 19) != Some(&1)
    {
        return None;
    }
    let head_index = u32_at(bytes, paired_at + 20)?;
    if bytes.get(paired_at + 24..paired_at + 28) != Some(&[0u8; 4][..]) {
        return None;
    }
    // Locate the head record and decode its transform.
    let mut position = 0usize;
    let head_at = loop {
        let at = next_indexed_record_offset(bytes, position)?;
        if u32_at(bytes, at + 7) == Some(head_index) {
            break at;
        }
        position = at + 1;
    };
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, head_at, 0..=2000, u8::is_ascii_graphic)?;
    if after_tag != head_at + 7
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let head_end = next_indexed_record_offset(bytes, head_at + 11).unwrap_or(bytes.len());
    let frame_length = head_end.checked_sub(head_at)?;
    let (transform, transform_offset) = match frame_length {
        34 if bytes.get(head_at + 11..head_at + 21) == Some(&[0u8; 10][..])
            && bytes.get(head_at + 21..head_at + 24) == Some(&[1, 0, 1][..])
            && bytes.get(head_at + 28..head_at + 34) == Some(&[0u8; 6][..]) =>
        {
            (identity_matrix(), None)
        }
        MEMBER_RUN_HEAD_FRAME if bytes.get(head_at + 11..head_at + 22) == Some(&[0u8; 11][..]) => {
            let values = f64s_at(bytes, head_at + 22, 16)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.iter().copied().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            if !valid_sketch_transform(&transform)
                || bytes.get(head_at + 150..head_at + 152) != Some(&[0, 1][..])
            {
                return None;
            }
            (transform, Some((head_at + 22) as u64))
        }
        _ => return None,
    };
    Some(DesignSketchPlacement {
        id: String::new(),
        scope_record_index: None,
        entity_id: entity.entity_id.clone(),
        entity_suffix: entity.entity_suffix,
        byte_offset: head_at as u64,
        class_tag,
        record_index: head_index,
        frame_length: frame_length as u64,
        transform,
        transform_offset,
        paired_class_tag,
        paired_byte_offset: paired_at as u64,
        member_run_head: true,
    })
}

pub(crate) fn parse_sketch_placement_candidates(
    bytes: &[u8],
    scope_record_index: u32,
    entity_id: &str,
    entity_suffix: u64,
    record_index: u32,
) -> Vec<DesignSketchPlacement> {
    let mut headers = Vec::new();
    let mut position = 0usize;
    while let Some(at) = next_indexed_record_offset(bytes, position) {
        if u32_at(bytes, at + 7) == Some(record_index) {
            headers.push(at);
        }
        position = at + 1;
    }
    let mut out = Vec::new();
    for pair in headers.windows(2) {
        let start = pair[0];
        let paired_at = pair[1];
        let frame_length = paired_at.saturating_sub(start);
        if frame_length != 201 && frame_length != 329 && frame_length != 213 && frame_length != 341
        {
            continue;
        }
        let Some((class_tag, after_tag)) =
            lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)
        else {
            continue;
        };
        let Some((paired_class_tag, paired_after_tag)) =
            lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)
        else {
            continue;
        };
        if after_tag != start + 7
            || paired_after_tag != paired_at + 7
            || class_tag.len() != 3
            || paired_class_tag.len() != 3
            || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
            || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
            || u32_at(bytes, paired_after_tag) != Some(record_index)
        {
            continue;
        }
        let (transform, transform_offset) = match frame_length {
            201 => (identity_matrix(), None),
            329 => {
                let Some(values) = f64s_at(bytes, start + 55, 16) else {
                    continue;
                };
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.iter().copied().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                if !valid_sketch_transform(&transform) {
                    continue;
                }
                (transform, Some((start + 55) as u64))
            }
            // The `EntityGenesis`-flavor frame: `0x01` at offset 55, nine
            // zero bytes, and a form byte at offset 65. Form `0x01` is the
            // identity transform; form `0x00` is followed by the row-major
            // 4×4 f64 matrix at offset 66. The WorkPlane sibling of this
            // record class carries a marked record reference at offset 57
            // and fails the zero-run check.
            213 | 341 => {
                if bytes.get(start + 55) != Some(&1)
                    || bytes.get(start + 56..start + 65) != Some(&[0u8; 9][..])
                {
                    continue;
                }
                match (frame_length, bytes.get(start + 65)) {
                    (213, Some(&1)) => (identity_matrix(), None),
                    (341, Some(&0)) => {
                        let Some(values) = f64s_at(bytes, start + 66, 16) else {
                            continue;
                        };
                        let mut transform = [[0.0; 4]; 4];
                        for (ordinal, value) in values.iter().copied().enumerate() {
                            transform[ordinal / 4][ordinal % 4] = value;
                        }
                        if !valid_sketch_transform(&transform) {
                            continue;
                        }
                        (transform, Some((start + 66) as u64))
                    }
                    _ => continue,
                }
            }
            _ => continue,
        };
        out.push(DesignSketchPlacement {
            id: String::new(),
            scope_record_index: Some(scope_record_index),
            entity_id: entity_id.to_owned(),
            entity_suffix,
            byte_offset: start as u64,
            class_tag,
            record_index,
            frame_length: frame_length as u64,
            transform,
            transform_offset,
            paired_class_tag,
            paired_byte_offset: paired_at as u64,
            member_run_head: false,
        });
    }
    out
}

pub(crate) fn identity_matrix() -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub(crate) fn valid_sketch_transform(transform: &[[f64; 4]; 4]) -> bool {
    const EPSILON: f64 = 1.0e-10;
    if !transform.iter().flatten().all(|value| value.is_finite())
        || transform[3] != [0.0, 0.0, 0.0, 1.0]
    {
        return false;
    }
    let columns = [
        [transform[0][0], transform[1][0], transform[2][0]],
        [transform[0][1], transform[1][1], transform[2][1]],
        [transform[0][2], transform[1][2], transform[2][2]],
    ];
    for (ordinal, column) in columns.iter().enumerate() {
        let norm = column.iter().map(|value| value * value).sum::<f64>();
        if (norm - 1.0).abs() > EPSILON {
            return false;
        }
        for other in &columns[..ordinal] {
            let dot = column
                .iter()
                .zip(other)
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if dot.abs() > EPSILON {
                return false;
            }
        }
    }
    true
}

/// Decode the persistent u64 point and curve identity references
/// (`pt_tag`, `crv_primary_id`, `crv_secondary_id`, each typed
/// `IntrinsicMetaTypeuint64`) from every design `BulkStream` entry in `scan`,
/// sorted by stream offset.
pub fn decode_persistent_references(
    scan: &ContainerScan,
) -> Result<Vec<PersistentReference>, CodecError> {
    let mut out = Vec::new();
    for (entry_ordinal, entry) in scan
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        for &(name, kind) in &[
            (b"pt_tag".as_slice(), PersistentReferenceKind::Point),
            (
                b"crv_primary_id".as_slice(),
                PersistentReferenceKind::CurvePrimary,
            ),
            (
                b"crv_secondary_id".as_slice(),
                PersistentReferenceKind::CurveSecondary,
            ),
        ] {
            let mut cursor = 0;
            while let Some(relative) = bytes[cursor..].windows(name.len()).position(|w| w == name) {
                let offset = cursor + relative;
                cursor = offset + name.len();
                let compact_type_offset = offset + name.len();
                let type_offset = if u32_at(bytes, compact_type_offset) == Some(23) {
                    compact_type_offset
                } else if u32_at(bytes, compact_type_offset) == Some(2)
                    && u32_at(bytes, compact_type_offset + 4) == Some(14)
                    && bytes
                        .get(compact_type_offset + 8..compact_type_offset + 22)
                        .is_some()
                    && u32_at(bytes, compact_type_offset + 22) == Some(23)
                {
                    compact_type_offset + 22
                } else {
                    continue;
                };
                let Some(length_bytes) = bytes.get(type_offset..type_offset + 4) else {
                    continue;
                };
                if u32::from_le_bytes(length_bytes.try_into().expect(
                    "invariant: length_bytes is a 4-byte slice from bytes.get(range) of length 4",
                )) != 23
                {
                    continue;
                }
                let type_name = b"IntrinsicMetaTypeuint64";
                if bytes.get(type_offset + 4..type_offset + 4 + type_name.len()) != Some(type_name)
                {
                    continue;
                }
                let value_offset = type_offset + 4 + type_name.len();
                let Some(raw) = bytes.get(value_offset..value_offset + 8) else {
                    continue;
                };
                out.push((
                    entry_ordinal,
                    PersistentReference {
                        id: ids::native_persistent_reference_id(&entry.name, offset),
                        byte_offset: offset as u64,
                        value_offset: (value_offset - offset) as u32,
                        kind,
                        value: u64::from_le_bytes(raw.try_into().expect(
                            "invariant: raw is an 8-byte slice from bytes.get(range) of length 8",
                        )),
                    },
                ));
            }
        }
    }
    out.sort_by_key(|(entry_ordinal, reference)| (*entry_ordinal, reference.byte_offset));
    Ok(out.into_iter().map(|(_, reference)| reference).collect())
}

/// Decode every indexed `EDGE_REFERENCE_LOST` record from each design
/// `BulkStream` entry in `scan`.
pub fn decode_lost_edge_references(
    scan: &ContainerScan,
) -> Result<Vec<LostEdgeReference>, CodecError> {
    let mut out = Vec::new();
    let marker = b"EDGE_REFERENCE_LOST";
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut cursor = 0;
        while let Some(relative) = bytes[cursor..]
            .windows(marker.len())
            .position(|window| window == marker)
        {
            let offset = cursor + relative;
            cursor = offset + marker.len();
            let Some(header_offset) = offset.checked_sub(29) else {
                continue;
            };
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, header_offset, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if after_tag != header_offset + 7
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || bytes.get(header_offset + 11..header_offset + 25) != Some(&[0; 14])
                || u32_at(bytes, header_offset + 25) != Some(marker.len() as u32)
            {
                continue;
            }
            let Some(record_index) = u32_at(bytes, after_tag) else {
                continue;
            };
            let next_byte_offset = offset + marker.len();
            let Some((next_class_tag, after_next_tag)) =
                lp_ascii_filtered(bytes, next_byte_offset, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if after_next_tag != next_byte_offset + 7
                || !next_class_tag.bytes().all(|byte| byte.is_ascii_digit())
            {
                continue;
            }
            let Some(next_record_index) = u32_at(bytes, after_next_tag) else {
                continue;
            };
            out.push(LostEdgeReference {
                id: ids::native_lost_edge_reference_id(&entry.name, header_offset),
                record_byte_offset: header_offset as u64,
                class_tag_offset: (header_offset + 4) as u64,
                class_tag,
                record_index,
                record_index_offset: (header_offset + 7) as u64,
                byte_offset: offset as u64,
                next_byte_offset: next_byte_offset as u64,
                next_class_tag,
                next_record_index,
            });
        }
    }
    Ok(out)
}

/// Decode every GUID-owned design object record from each design
/// `MetaStream` entry in `scan` ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)): an ASCII type name, the design
/// entity IDs it owns, its self GUID, an optional parent GUID, and a
/// revision. Unrecognized type names remain exact native object kinds.
pub fn decode_objects(scan: &ContainerScan) -> Result<Vec<DesignObject>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::METASTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut offset = 0usize;
        while offset + 8 <= bytes.len() {
            let Some((name, after_name)) =
                lp_ascii_filtered(bytes, offset, 0..=2000, u8::is_ascii_graphic)
            else {
                offset += 1;
                continue;
            };
            if name.is_empty()
                || is_guid_relaxed(&name)
                || !name.bytes().all(|byte| byte.is_ascii_graphic())
            {
                offset += 1;
                continue;
            }
            let kind = object_kind(&name);
            let Some(count_raw) = bytes.get(after_name..after_name + 4) else {
                break;
            };
            let count = usize::try_from(u32::from_le_bytes(count_raw.try_into().expect(
                "invariant: count_raw is a 4-byte slice from bytes.get(range) of length 4",
            )))
            .unwrap_or(usize::MAX);
            let ids_end = after_name
                .checked_add(4)
                .and_then(|at| count.checked_mul(8).and_then(|size| at.checked_add(size)));
            let Some(ids_end) = ids_end.filter(|end| count <= 200 && *end <= bytes.len()) else {
                offset += 1;
                continue;
            };
            let entity_ids = bytes[after_name + 4..ids_end]
                .chunks_exact(8)
                .map(|raw| {
                    u64::from_le_bytes(
                        raw.try_into()
                            .expect("invariant: chunks_exact(8) yields 8-byte slices"),
                    )
                })
                .collect::<Vec<_>>();
            let entity_id_offsets = (0..entity_ids.len())
                .map(|index| (after_name + 4 + index * 8) as u64)
                .collect();
            let Some((self_guid, after_self)) =
                lp_ascii_filtered(bytes, ids_end, 0..=2000, u8::is_ascii_graphic)
                    .filter(|(guid, _)| is_guid_relaxed(guid))
            else {
                offset += 1;
                continue;
            };
            let mut tail = after_self;
            while bytes.get(tail) == Some(&0) {
                tail += 1;
            }
            let zero_run_length = u32::try_from(tail - after_self).unwrap_or(u32::MAX);
            let (parent_guid, parent_guid_offset, revision_offset) =
                lp_ascii_filtered(bytes, tail, 0..=2000, u8::is_ascii_graphic)
                    .filter(|(guid, _)| is_guid_relaxed(guid))
                    .map_or((None, None, tail), |(guid, end)| {
                        (Some(guid), Some((tail + 4) as u64), end)
                    });
            let Some(revision_raw) = bytes.get(revision_offset..revision_offset + 4) else {
                offset += 1;
                continue;
            };
            let revision = u32::from_le_bytes(revision_raw.try_into().expect(
                "invariant: revision_raw is a 4-byte slice from bytes.get(range) of length 4",
            ));
            if revision > 10_000 {
                offset += 1;
                continue;
            }
            out.push(DesignObject {
                id: ids::native_design_object_id(&entry.name, offset),
                byte_offset: offset as u64,
                kind,
                entity_ids,
                entity_id_offsets,
                self_guid,
                self_guid_offset: (ids_end + 4) as u64,
                zero_run_length,
                parent_guid,
                parent_guid_offset,
                revision,
                revision_offset: revision_offset as u64,
            });
            offset = revision_offset + 4;
        }
    }
    Ok(out)
}

/// Parse the fixed entity-header layout at `start`: a u64 entity suffix, five
/// zero bytes, an optional slot, and the UTF-16LE entity id whose numeric
/// suffix equals the header's entity suffix.
pub(crate) fn parse_settled_entity_header(
    bytes: &[u8],
    start: usize,
) -> Option<(u64, String, bool, usize)> {
    let entity_suffix = u64::from_le_bytes(bytes.get(start + 7..start + 15)?.try_into().ok()?);
    if entity_suffix == 0
        || entity_suffix >= 1 << 32
        || bytes.get(start + 15..start + 20) != Some(&[0u8; 5])
    {
        return None;
    }
    let (optional_slot_present, string_offset) = match bytes.get(start + 20)? {
        0 => (false, start + 21),
        1 if bytes.get(start + 21..start + 25) == Some(&[0u8; 4]) => (true, start + 25),
        _ => return None,
    };
    let (entity_id, end) = lp_utf16_bounded(bytes, string_offset, 1..=256)?;
    let (_, suffix) = entity_id.rsplit_once('_')?;
    (suffix.parse::<u64>().ok() == Some(entity_suffix)).then_some((
        entity_suffix,
        entity_id,
        optional_slot_present,
        end,
    ))
}

/// Parse the `EntityGenesis` entity-header layout at `start`: the u32 record
/// index doubles as the entity suffix and is followed by a zero run, a
/// `0x01`-marked u32 1, the `EntityGenesis` and `IntrinsicMetaTypeuint64`
/// key strings, the u64 origin bitfield, and the UTF-16LE entity id whose
/// numeric suffix equals the record index.
pub(crate) fn parse_genesis_entity_header(
    bytes: &[u8],
    start: usize,
) -> Option<(u64, String, bool, usize)> {
    let entity_suffix = u64::from(u32_at(bytes, start + 7)?);
    if entity_suffix == 0 {
        return None;
    }
    let mut cursor = start + 11;
    while bytes.get(cursor) == Some(&0) && cursor < start + 35 {
        cursor += 1;
    }
    if cursor == start + 11 || bytes.get(cursor) != Some(&1) || u32_at(bytes, cursor + 1) != Some(1)
    {
        return None;
    }
    let (key, after_key) = lp_ascii_filtered(bytes, cursor + 5, 0..=2000, u8::is_ascii_graphic)?;
    if key != "EntityGenesis" {
        return None;
    }
    let (meta_type, after_type) =
        lp_ascii_filtered(bytes, after_key, 0..=2000, u8::is_ascii_graphic)?;
    if meta_type != "IntrinsicMetaTypeuint64" {
        return None;
    }
    let (entity_id, end) = lp_utf16_bounded(bytes, after_type + 8, 1..=256)?;
    let (_, suffix) = entity_id.rsplit_once('_')?;
    (suffix.parse::<u64>().ok() == Some(entity_suffix)).then_some((
        entity_suffix,
        entity_id,
        false,
        end,
    ))
}

/// Parse the counted member-record run of the paired same-index container
/// record that follows an `EntityGenesis`-form sketch entity header: the u32
/// member count at paired-record offset 52, the marked reference to the
/// sketch's base-point record, and `count` entries of `0x01 + u32
/// record_index + six zero bytes` naming the sketch's owned records. The
/// base-point reference is returned as the first member.
pub(crate) fn parse_sketch_member_run(
    bytes: &[u8],
    from: usize,
    entity_suffix: u64,
) -> (Vec<u32>, Vec<u64>) {
    let empty = (Vec::new(), Vec::new());
    let Some(paired) = next_indexed_record_offset(bytes, from) else {
        return empty;
    };
    if u32_at(bytes, paired + 7).map(u64::from) != Some(entity_suffix) {
        return empty;
    }
    let Some(count) = u32_at(bytes, paired + 52).and_then(|count| usize::try_from(count).ok())
    else {
        return empty;
    };
    if count == 0
        || bytes.get(paired + 56) != Some(&1)
        || bytes.get(paired + 61..paired + 67) != Some(&[0u8; 6][..])
    {
        return empty;
    }
    let Some(base_point_index) = u32_at(bytes, paired + 57) else {
        return empty;
    };
    let mut member_indices = Vec::with_capacity(count + 1);
    let mut member_offsets = Vec::with_capacity(count + 1);
    member_indices.push(base_point_index);
    member_offsets.push((paired + 57) as u64);
    for ordinal in 0..count {
        let marker = paired + 67 + ordinal * 11;
        if bytes.get(marker) != Some(&1)
            || bytes.get(marker + 5..marker + 11) != Some(&[0u8; 6][..])
        {
            return empty;
        }
        let Some(record_index) = u32_at(bytes, marker + 1) else {
            return empty;
        };
        member_indices.push(record_index);
        member_offsets.push((marker + 1) as u64);
    }
    (member_indices, member_offsets)
}

/// Parse the counted member-record run of a legacy sketch container's paired
/// same-index record. The paired record stores its head-placement reference
/// at offset 19, six zero bytes, a u32 sketch ordinal and seven bytes of
/// state, then the member count at offset 41. Each member is a padded marked
/// reference.
pub(crate) fn parse_legacy_sketch_member_run(
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
) -> Option<(Vec<u32>, Vec<u64>)> {
    let paired_at = next_indexed_record_offset(bytes, primary_at + 11)?;
    if u32_at(bytes, paired_at + 7) != Some(entity_suffix)
        || bytes.get(paired_at + 11..paired_at + 19) != Some(&[0u8; 8][..])
        || bytes.get(paired_at + 19) != Some(&1)
        || bytes.get(paired_at + 24..paired_at + 30) != Some(&[0u8; 6][..])
    {
        return None;
    }
    let (paired_class_tag, after_tag) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    if after_tag != paired_at + 7
        || paired_class_tag.len() != 3
        || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let count = usize::try_from(u32_at(bytes, paired_at + 41)?).ok()?;
    if count == 0 {
        return None;
    }
    let run_end = (paired_at + 45).checked_add(count.checked_mul(11)?)?;
    if run_end > bytes.len() {
        return None;
    }
    let mut member_indices = Vec::with_capacity(count);
    let mut member_offsets = Vec::with_capacity(count);
    for ordinal in 0..count {
        let marker = paired_at + 45 + ordinal * 11;
        if bytes.get(marker) != Some(&1)
            || bytes.get(marker + 5..marker + 11) != Some(&[0u8; 6][..])
        {
            return None;
        }
        member_indices.push(u32_at(bytes, marker + 1)?);
        member_offsets.push((marker + 1) as u64);
    }
    Some((member_indices, member_offsets))
}

/// Decode every self-validating per-entity design `BulkStream` header (spec
/// [§8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)): a three-digit class tag, an entity suffix, a UTF-16LE entity ID
/// whose numeric suffix must match the header's entity suffix, and, for
/// sketch-typed entities, the trailing reference-list header. Headers occur in
/// the fixed layout or in the `EntityGenesis` layout.
pub fn decode_entity_headers(scan: &ContainerScan) -> Result<Vec<DesignEntityHeader>, CodecError> {
    let mut out = Vec::new();
    let mut object_kinds = HashMap::new();
    let objects = decode_objects(scan)?;
    let mut legacy_sketch_candidates = HashMap::<String, std::collections::HashSet<u32>>::new();
    for object in objects {
        for &entity_id in &object.entity_ids {
            object_kinds
                .entry(entity_id)
                .or_insert_with(|| object.kind.clone());
        }
        if object.kind == DesignObjectKind::Sketch {
            let Some(stream) = native_stream(&object.id) else {
                continue;
            };
            let Some(meta_name) = stream.strip_prefix(ids::SCHEME_PREFIX) else {
                continue;
            };
            let Some(prefix) = meta_name.strip_suffix("MetaStream.dat") else {
                continue;
            };
            let bulk_name = format!("{prefix}BulkStream.dat");
            legacy_sketch_candidates
                .entry(bulk_name)
                .or_default()
                .extend(
                    object
                        .entity_ids
                        .into_iter()
                        .filter_map(|identity| u32::try_from(identity).ok()),
                );
        }
    }
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut offset = 0usize;
        while offset + 30 <= bytes.len() {
            let Some(relative) = bytes[offset..]
                .windows(4)
                .position(|window| window == [3, 0, 0, 0])
            else {
                break;
            };
            let start = offset + relative;
            offset = start + 1;
            let Some(class_tag) = bytes.get(start + 4..start + 7) else {
                break;
            };
            if !class_tag.iter().all(u8::is_ascii_digit) {
                continue;
            }
            let settled = parse_settled_entity_header(bytes, start);
            let genesis_form = settled.is_none();
            let Some((entity_suffix, entity_id, optional_slot_present, end)) =
                settled.or_else(|| parse_genesis_entity_header(bytes, start))
            else {
                continue;
            };
            let object_kind = object_kinds.get(&entity_suffix).cloned();
            let (
                record_reference,
                record_reference_offset,
                declared_reference_count,
                reference_indices,
                reference_offsets,
                record_end,
            ) = if object_kind == Some(DesignObjectKind::Sketch) {
                decode_reference_list(bytes, end).map_or_else(
                    || (None, None, None, Vec::new(), Vec::new(), end),
                    |list| {
                        (
                            list.record_reference,
                            Some(list.record_reference_offset as u64),
                            Some(list.declared_count),
                            list.references,
                            list.reference_offsets
                                .into_iter()
                                .map(|offset| offset as u64)
                                .collect(),
                            list.end,
                        )
                    },
                )
            } else {
                (None, None, None, Vec::new(), Vec::new(), end)
            };
            let (member_indices, member_offsets) =
                if genesis_form && object_kind == Some(DesignObjectKind::Sketch) {
                    parse_sketch_member_run(bytes, record_end, entity_suffix)
                } else {
                    (Vec::new(), Vec::new())
                };
            out.push(DesignEntityHeader {
                id: ids::native_design_entity_header_id(&entry.name, start),
                byte_offset: start as u64,
                entity_suffix,
                entity_id,
                class_tag: String::from_utf8_lossy(class_tag).into_owned(),
                optional_slot_present,
                object_kind,
                record_reference,
                record_reference_offset,
                declared_reference_count,
                reference_indices,
                reference_offsets,
                member_indices,
                member_offsets,
            });
            offset = record_end;
        }

        // Legacy Design streams do not carry textual entity headers. Their
        // MSketch metadata names candidate record indices; only actual sketch
        // containers have a consecutive same-index pair with the legacy
        // counted member run. Materialize the same ownership abstraction used
        // by later entity-header forms so downstream binding remains uniform.
        let candidates = legacy_sketch_candidates
            .get(&entry.name)
            .cloned()
            .unwrap_or_default();
        let scope = ids::native_scope(&entry.name);
        let mut existing = out
            .iter()
            .filter(|entity| native_stream(&entity.id) == Some(scope.as_str()))
            .filter_map(|entity| u32::try_from(entity.entity_suffix).ok())
            .collect::<std::collections::HashSet<_>>();
        let mut position = 0usize;
        while let Some(start) = next_indexed_record_offset(bytes, position) {
            position = start + 1;
            let Some(entity_suffix) = u32_at(bytes, start + 7) else {
                continue;
            };
            if !candidates.contains(&entity_suffix) || existing.contains(&entity_suffix) {
                continue;
            }
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if after_tag != start + 7
                || class_tag.len() != 3
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
            {
                continue;
            }
            let Some((member_indices, member_offsets)) =
                parse_legacy_sketch_member_run(bytes, start, entity_suffix)
            else {
                continue;
            };
            existing.insert(entity_suffix);
            out.push(DesignEntityHeader {
                id: ids::native_design_entity_header_id(&entry.name, start),
                byte_offset: start as u64,
                entity_suffix: u64::from(entity_suffix),
                entity_id: format!("Sketch_{entity_suffix}"),
                class_tag,
                optional_slot_present: false,
                object_kind: Some(DesignObjectKind::Sketch),
                record_reference: None,
                record_reference_offset: None,
                declared_reference_count: None,
                reference_indices: Vec::new(),
                reference_offsets: Vec::new(),
                member_indices,
                member_offsets,
            });
        }
    }
    out.sort_by_key(|entity| entity.id.clone());
    Ok(out)
}

/// Decode the indexed dynamic-class record headers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)) that `entities`'
/// reference-list entries point at: a `u32` record index and a three-digit
/// class tag, for each record index named by any [`DesignEntityHeader`] in
/// `entities`.
pub fn decode_record_headers(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let wanted = entities
        .iter()
        .filter_map(|entity| {
            let scope = native_stream(&entity.id)?;
            Some(
                entity
                    .reference_indices
                    .iter()
                    .map(move |record_index| (scope.to_owned(), *record_index)),
            )
        })
        .flatten()
        .collect::<std::collections::HashSet<_>>();
    decode_headers_for_indices(scan, &wanted)
}

/// Decode the indexed dynamic-class record headers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)) named by
/// `indices` directly, bypassing entity reference lists. Used to fetch record
/// headers referenced by records other than [`DesignEntityHeader`] (for
/// example, sketch relation records).
pub fn decode_related_record_headers(
    scan: &ContainerScan,
    indices: &[(String, u32)],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let wanted = indices
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    decode_headers_for_indices(scan, &wanted)
}

fn decode_headers_for_indices(
    scan: &ContainerScan,
    wanted: &std::collections::HashSet<(String, u32)>,
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let mut emitted = std::collections::HashSet::new();
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while position + 11 <= bytes.len() {
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, position, 0..=2000, u8::is_ascii_graphic)
            else {
                position += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                position += 1;
                continue;
            }
            let Some(raw) = bytes.get(after_tag..after_tag + 4) else {
                break;
            };
            let record_index = u32::from_le_bytes(
                raw.try_into()
                    .expect("invariant: raw is a 4-byte slice from bytes.get(range) of length 4"),
            );
            let scope = ids::native_scope(&entry.name);
            if wanted.contains(&(scope, record_index)) && emitted.insert(record_index) {
                out.push(DesignRecordHeader {
                    id: ids::native_design_record_header_id(&entry.name, position),
                    record_index,
                    class_tag,
                    byte_offset: position as u64,
                });
            }
            // Headers are located in an otherwise heterogeneous stream. Keep
            // the scan byte-aligned so a plausible length-prefixed string in
            // an enclosing payload cannot skip a real nested header.
            position += 1;
        }
    }
    out.sort_by_key(|record| record.id.clone());
    Ok(out)
}

/// Decode the sketch-relation body at each `records` entry's offset: the
/// owning sketch relation's member reference list, owner reference, state,
/// and return-member list. `records` supplies the byte offsets and class tags
/// (typically from [`decode_related_record_headers`]).
pub fn decode_sketch_relations(
    scan: &ContainerScan,
    records: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
) -> Result<Vec<SketchRelation>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let scope = ids::native_scope(&entry.name);
        let owners = entities
            .iter()
            .filter(|entity| {
                native_stream(&entity.id) == Some(scope.as_str())
                    && entity.object_kind == Some(DesignObjectKind::Sketch)
            })
            .filter_map(|entity| u32::try_from(entity.entity_suffix).ok())
            .collect::<std::collections::HashSet<_>>();
        let bytes = scan.entry_bytes(&entry.name)?;
        for record in records
            .iter()
            .filter(|record| native_stream(&record.id) == Some(scope.as_str()))
        {
            let Ok(at) = usize::try_from(record.byte_offset) else {
                continue;
            };
            let record_end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            let Some(payload) = bytes.get(at..record_end) else {
                continue;
            };
            let Some(parsed) = parse_sketch_relation(payload, &owners) else {
                continue;
            };
            if payload
                .get(parsed.parsed_end..)
                .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
            {
                continue;
            }
            let (constraint_kinds, unknown_constraint_bits) = decode_constraint_kinds(parsed.state);
            let pattern = decode_pattern_definition(payload, &parsed);
            out.push(SketchRelation {
                id: ids::native_sketch_relation_id(&entry.name, record.record_index),
                record_index: record.record_index,
                class_tag: record.class_tag.clone(),
                byte_offset: record.byte_offset,
                state_offset: parsed.state_offset as u32,
                owner_reference: parsed.owner_reference,
                owner_entity_id: String::new(),
                owner_reference_offset: parsed.owner_reference_offset as u32,
                auxiliary_references: parsed.auxiliary_references,
                auxiliary_reference_offsets: parsed
                    .auxiliary_reference_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                members: parsed.members,
                resolved_members: Vec::new(),
                member_offsets: parsed
                    .member_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                state: parsed.state,
                constraint_kinds,
                unknown_constraint_bits,
                member_roles: parsed.member_roles,
                entity_genesis: parsed.entity_genesis,
                pattern,
                return_members: parsed.return_members,
                resolved_return_members: Vec::new(),
                return_member_offsets: parsed
                    .return_member_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                raw_bytes: payload.to_vec(),
            });
        }
    }
    Ok(out)
}

/// Decode the class-specific auxiliary payload of a pattern or text-frame
/// relation from its fixed positions inside `payload`. Circular patterns store
/// the angle- and count-parameter references, the evaluated f64 total angle six
/// zero bytes after the count-parameter reference, and the evaluated u32
/// instance count directly after it. Rectangular patterns store, per direction,
/// the evaluated u32 count, the count-parameter reference, a three-component
/// f64 unit direction six zero bytes after that reference, the evaluated f64
/// adjacent-instance spacing, and the distance-parameter reference. Text-frame relations
/// repeat the sketch-text member as the single auxiliary reference.
pub(crate) fn decode_pattern_definition(
    payload: &[u8],
    parsed: &ParsedSketchRelation,
) -> Option<crate::records::SketchPatternDefinition> {
    use crate::records::{SketchPatternDefinition, SketchPatternDirection};
    let f64_at = |at: usize| {
        payload
            .get(at..at + 8)
            .map(|raw| f64::from_le_bytes(raw.try_into().expect("8-byte slice")))
            .filter(|value| value.is_finite())
    };
    let reference_end = |ordinal: usize| Some(parsed.auxiliary_reference_offsets.get(ordinal)? + 4);
    if parsed.state == 0x1000_0000 && parsed.auxiliary_references.len() == 2 {
        let angle_at = reference_end(1)? + 6;
        let evaluated_angle = f64_at(angle_at)?;
        let evaluated_count = u32_at(payload, angle_at + 8)?;
        if !(1..=100_000).contains(&evaluated_count) {
            return None;
        }
        return Some(SketchPatternDefinition::Circular {
            angle_parameter: parsed.auxiliary_references[0],
            count_parameter: parsed.auxiliary_references[1],
            evaluated_angle,
            evaluated_count,
        });
    }
    if parsed.state == 0x2000_0000 && matches!(parsed.auxiliary_references.len(), 4 | 5) {
        let mut directions = Vec::with_capacity(2);
        let clauses = if parsed.auxiliary_references.len() == 5 {
            [
                (reference_end(0)? + 10, 1, 2),
                (reference_end(2)? + 6, 3, 4),
            ]
        } else {
            [
                (parsed.auxiliary_reference_offsets[0].checked_sub(5)?, 0, 1),
                (parsed.auxiliary_reference_offsets[2].checked_sub(5)?, 2, 3),
            ]
        };
        for (count_at, count_ordinal, distance_ordinal) in clauses {
            let evaluated_count = u32_at(payload, count_at)?;
            if !(1..=100_000).contains(&evaluated_count) {
                return None;
            }
            let direction_at = reference_end(count_ordinal)? + 6;
            let direction = [
                f64_at(direction_at)?,
                f64_at(direction_at + 8)?,
                f64_at(direction_at + 16)?,
            ];
            let length = direction.iter().map(|axis| axis * axis).sum::<f64>();
            if (length - 1.0).abs() > 1.0e-6 {
                return None;
            }
            directions.push(SketchPatternDirection {
                evaluated_count,
                count_parameter: parsed.auxiliary_references[count_ordinal],
                direction,
                evaluated_distance: f64_at(direction_at + 24)?,
                distance_parameter: parsed.auxiliary_references[distance_ordinal],
            });
        }
        return Some(SketchPatternDefinition::Rectangular {
            directions: directions.try_into().ok()?,
        });
    }
    if parsed.state == 0x100_0000_0000
        && parsed.auxiliary_references.len() == 1
        && parsed.members.contains(&parsed.auxiliary_references[0])
    {
        return Some(SketchPatternDefinition::TextFrame {
            text_reference: parsed.auxiliary_references[0],
        });
    }
    if parsed.state == 0x200_0000_0000
        && parsed.auxiliary_references.len() == 1
        && parsed.members.contains(&parsed.auxiliary_references[0])
    {
        if let Some(glyph_transforms) = parsed.text_glyph_transforms.clone() {
            return Some(SketchPatternDefinition::TextPath {
                text_reference: parsed.auxiliary_references[0],
                glyph_transforms,
            });
        }
    }
    None
}

pub(crate) const SKETCH_CONSTRAINT_MASK: u64 = 0x0320_b000_3fff;

pub(crate) fn decode_constraint_kinds(state: u64) -> (Vec<SketchConstraintKind>, u64) {
    let definitions = [
        (0x0000_0001, SketchConstraintKind::Coincident),
        (0x0000_0002, SketchConstraintKind::Colinear),
        (0x0000_0004, SketchConstraintKind::Concentric),
        (0x0000_0008, SketchConstraintKind::EqualLength),
        (0x0000_0010, SketchConstraintKind::Parallel),
        (0x0000_0020, SketchConstraintKind::Perpendicular),
        (0x0000_0040, SketchConstraintKind::Horizontal),
        (0x0000_0080, SketchConstraintKind::Vertical),
        (0x0000_0100, SketchConstraintKind::Tangent),
        (0x0000_0200, SketchConstraintKind::Curvature),
        (0x0000_0400, SketchConstraintKind::Symmetry),
        (0x0000_0800, SketchConstraintKind::Equal),
        (0x0000_1000, SketchConstraintKind::Midpoint),
        (0x0000_2000, SketchConstraintKind::Polygon),
        (0x1000_0000, SketchConstraintKind::CircularPattern),
        (0x2000_0000, SketchConstraintKind::RectangularPattern),
        (0x8000_0000, SketchConstraintKind::SplineGroup),
        (0x20_0000_0000, SketchConstraintKind::Offset),
        (0x100_0000_0000, SketchConstraintKind::TextFrame),
        (0x200_0000_0000, SketchConstraintKind::TextPath),
    ];
    let mut kinds = if state == 0 {
        vec![SketchConstraintKind::Coincident]
    } else {
        Vec::new()
    };
    let mut recognized = 0u64;
    for (bit, kind) in definitions {
        if state & bit != 0 {
            kinds.push(kind);
            recognized |= bit;
        }
    }
    debug_assert_eq!(recognized, state & SKETCH_CONSTRAINT_MASK);
    (kinds, state & !SKETCH_CONSTRAINT_MASK)
}

pub(crate) fn trailing_sketch_owner_reference(bytes: &[u8], from: usize) -> Option<u32> {
    let record_end = next_indexed_record_offset(bytes, from).unwrap_or(bytes.len());
    let tail = record_end.checked_sub(11)?;
    if bytes.get(tail) != Some(&1) || bytes.get(tail + 5..tail + 11) != Some(&[0u8; 6][..]) {
        return None;
    }
    u32_at(bytes, tail + 1)
}

/// Decode every sketch-point record ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata), `pt_tag`) from each design
/// `BulkStream` entry in `scan`: the persistent point id, a paired record
/// reference, and the sketch `(u, v)` coordinates, converted centimetre→
/// millimetre. Records whose scaled coordinates are non-finite are skipped.
pub fn decode_sketch_points(scan: &ContainerScan) -> Result<Vec<SketchPoint>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let mut emitted = std::collections::HashSet::new();
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while at + 113 <= bytes.len() {
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)
            else {
                at += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                at += 1;
                continue;
            }
            let Some(record_index) = u32_at(bytes, after_tag) else {
                break;
            };
            let payload = &bytes[at..];
            let Some((persistent_id, paired_reference, x, y, shift, entity_genesis)) =
                decode_sketch_point(payload)
            else {
                at += 1;
                continue;
            };
            let (u, v) = (x * 10.0, y * 10.0);
            let depth = f64_at(payload, 105 + shift).map(|value| value * 10.0);
            if !u.is_finite() || !v.is_finite() || depth.is_none_or(|value| !value.is_finite()) {
                at += 1;
                continue;
            }
            let owner_reference = trailing_sketch_owner_reference(bytes, at + 112 + shift);
            if emitted.insert(record_index) {
                out.push(SketchPoint {
                    id: ids::native_sketch_point_id(&entry.name, at),
                    record_index,
                    owner_reference,
                    class_tag,
                    byte_offset: at as u64,
                    coordinate_offset: (89 + shift) as u32,
                    entity_genesis,
                    persistent_id,
                    paired_reference,
                    coordinates: Point2::new(u, v),
                    raw_bytes: payload[..113 + shift].to_vec(),
                });
            }
            at += 112;
        }
    }
    Ok(out)
}

/// Decode sketch-text records carrying persistent identities, font metrics,
/// UTF-16 content, and an owning-sketch reference.
pub fn decode_sketch_texts(scan: &ContainerScan) -> Result<Vec<SketchText>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while at + 230 <= bytes.len() {
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)
            else {
                at += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                at += 1;
                continue;
            }
            let Some(record_index) = u32_at(bytes, after_tag) else {
                break;
            };
            let record_end = next_indexed_record_offset(bytes, at + 7).unwrap_or(bytes.len());
            let Some(payload) = bytes.get(at..record_end) else {
                break;
            };
            if let Some(text) =
                decode_sketch_text_record(payload, &entry.name, class_tag, record_index, at)
            {
                out.push(text);
                at = record_end;
            } else {
                at += 1;
            }
        }
    }
    Ok(out)
}

pub(crate) fn decode_sketch_text_record(
    payload: &[u8],
    stream: &str,
    class_tag: String,
    record_index: u32,
    byte_offset: usize,
) -> Option<SketchText> {
    if payload.get(20) != Some(&1)
        || u32_at(payload, 21) != Some(3)
        || u32_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || u32_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
        || u32_at(payload, 77) != Some(10)
        || payload.get(81..91) != Some(b"textex_tag")
        || u32_at(payload, 91) != Some(23)
        || payload.get(95..118) != Some(b"IntrinsicMetaTypeuint64")
        || u32_at(payload, 126) != Some(12)
        || payload.get(130..142) != Some(b"txt_tag_base")
        || u32_at(payload, 142) != Some(23)
        || payload.get(146..169) != Some(b"IntrinsicMetaTypeuint64")
        || payload.get(177) != Some(&1)
    {
        return None;
    }
    let entity_genesis = read_u64(payload, 69)?;
    let persistent_id = read_u64(payload, 118)?;
    let base_id = read_u64(payload, 169)?;
    let height = f64_at(payload, 178)? * 10.0;
    let rotation = f64_at(payload, 186)?;
    let baseline_shift = f32::from_le_bytes(payload.get(194..198)?.try_into().ok()?);
    let vertical_scale = f32::from_le_bytes(payload.get(198..202)?.try_into().ok()?);
    let font_count = usize::try_from(u32_at(payload, 202)?).ok()?;
    if font_count == 0 || font_count > 1_024 {
        return None;
    }
    let (font_family, after_font) = utf16le_at(payload, 206, font_count)?;
    if payload.get(after_font) != Some(&0) {
        return None;
    }
    let width_factor = f64_at(payload, after_font + 1)?;
    let first_reference = after_font.checked_add(9)?;
    if payload.get(first_reference) != Some(&1)
        || payload.get(first_reference + 5..first_reference + 11) != Some(&[0; 6])
        || payload.get(first_reference + 11) != Some(&1)
        || payload.get(first_reference + 12..first_reference + 18) != Some(&[0; 6])
    {
        return None;
    }
    let text_count_at = first_reference.checked_add(18)?;
    let text_count = usize::try_from(u32_at(payload, text_count_at)?).ok()?;
    if text_count == 0 || text_count > 1_048_576 {
        return None;
    }
    let (text, after_text) = utf16le_at(payload, text_count_at + 4, text_count)?;
    if payload.get(after_text) != Some(&1)
        || payload.get(after_text + 5..after_text + 11) != Some(&[0; 6])
        || payload.len() < 11
    {
        return None;
    }
    let owner_at = payload.len() - 11;
    if payload.get(owner_at) != Some(&1)
        || payload.get(owner_at + 5..owner_at + 11) != Some(&[0; 6])
        || !height.is_finite()
        || height <= 0.0
        || !rotation.is_finite()
        || !baseline_shift.is_finite()
        || !vertical_scale.is_finite()
        || vertical_scale <= 0.0
        || !width_factor.is_finite()
        || width_factor <= 0.0
    {
        return None;
    }
    Some(SketchText {
        id: ids::native_sketch_text_id(stream, byte_offset),
        record_index,
        owner_reference: u32_at(payload, owner_at + 1)?,
        class_tag,
        byte_offset: byte_offset as u64,
        entity_genesis,
        persistent_id,
        base_id,
        text,
        font_family,
        height,
        width_factor,
        first_reference: u32_at(payload, first_reference + 1)?,
        second_reference: u32_at(payload, after_text + 1)?,
        raw_bytes: payload.to_vec(),
    })
}

fn decode_sketch_point(payload: &[u8]) -> Option<(u64, u32, f64, f64, usize, Option<u64>)> {
    if let Some(point) = decode_sketch_point_variant(payload, 0, 1) {
        return Some((point.0, point.1, point.2, point.3, 0, None));
    }
    if u32_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || u32_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = u64::from_le_bytes(payload.get(69..77)?.try_into().ok()?);
    decode_sketch_point_variant(payload, 52, 2)
        .map(|point| (point.0, point.1, point.2, point.3, 52, Some(entity_genesis)))
}

fn decode_sketch_point_variant(
    payload: &[u8],
    shift: usize,
    property_count: u32,
) -> Option<(u64, u32, f64, f64)> {
    if payload.get(20) != Some(&1)
        || u32_at(payload, 21) != Some(property_count)
        || u32_at(payload, 25 + shift) != Some(6)
        || payload.get(29 + shift..35 + shift) != Some(b"pt_tag")
        || u32_at(payload, 35 + shift) != Some(23)
        || payload.get(39 + shift..62 + shift) != Some(b"IntrinsicMetaTypeuint64")
        || payload.get(70 + shift) != Some(&1)
        || !payload
            .get(75 + shift..89 + shift)?
            .iter()
            .all(|&byte| byte <= 1)
    {
        return None;
    }
    Some((
        u64::from_le_bytes(payload.get(62 + shift..70 + shift)?.try_into().ok()?),
        u32_at(payload, 71 + shift)?,
        f64_at(payload, 89 + shift)?,
        f64_at(payload, 97 + shift)?,
    ))
}

/// Decode every sketch-curve record ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata), `crv_primary_id`/
/// `crv_secondary_id`) from each design `BulkStream` entry in `scan`: the
/// curve's persistent primary and secondary identities plus its NURBS, circular
/// arc, line, or referenced analytic geometry.
pub fn decode_sketch_curve_identities(
    scan: &ContainerScan,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let mut emitted = std::collections::HashSet::new();
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while at + 133 <= bytes.len() {
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)
            else {
                at += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                at += 1;
                continue;
            }
            let Some(record_index) = u32_at(bytes, after_tag) else {
                break;
            };
            let payload = &bytes[at..];
            let Some((primary_id, secondary_id, geometry_shift, entity_genesis)) =
                decode_sketch_curve_identity(payload)
            else {
                at += 1;
                continue;
            };
            if emitted.insert(record_index) {
                let geometry_payload = payload
                    .get(geometry_shift..)
                    .expect("invariant: geometry_shift (0 or 52) is <= payload.len() (checked >= 133 by the at + 133 <= bytes.len() loop guard)");
                let (geometry, geometry_offset, owner_scan_from) =
                    if let Some((geometry, end)) = decode_legacy_sketch_nurbs(geometry_payload) {
                        (Some(geometry), geometry_shift + 133, geometry_shift + end)
                    } else if let Some((geometry, end)) = decode_sketch_nurbs(geometry_payload) {
                        (Some(geometry), geometry_shift + 133, geometry_shift + end)
                    } else if let Some(geometry) = decode_circular_arc(geometry_payload) {
                        (
                            Some(geometry),
                            geometry_shift + 133,
                            geometry_shift + 133 + 12 * 8,
                        )
                    } else if let Some(geometry) = decode_line(geometry_payload) {
                        (
                            Some(geometry),
                            geometry_shift + 133,
                            geometry_shift + 133 + 12 * 8,
                        )
                    } else if let Some(geometry) = decode_compact_planar_line(geometry_payload) {
                        (
                            Some(geometry),
                            geometry_shift + 133,
                            geometry_shift + 133 + 9 * 8,
                        )
                    } else if let Some(geometry) = decode_referenced_analytic(geometry_payload) {
                        let shifted = geometry_payload
                            .get(11..)
                            .expect("referenced analytic decoder validated its 11-byte prefix");
                        let scalar_count = if decode_compact_planar_line(shifted).is_some() {
                            9
                        } else {
                            12
                        };
                        (
                            Some(geometry),
                            geometry_shift + 11 + 133,
                            geometry_shift + 11 + 133 + scalar_count * 8,
                        )
                    } else if let Some((geometry, end)) =
                        decode_text_frame_line(payload, geometry_shift, record_index)
                    {
                        (Some(geometry), end - 12 * 8, end)
                    } else {
                        (None, geometry_shift + 133, geometry_shift + 133)
                    };
                out.push(SketchCurveIdentity {
                    id: ids::native_sketch_curve_identity_id(&entry.name, at),
                    record_index,
                    owner_reference: trailing_sketch_owner_reference(bytes, at + owner_scan_from),
                    class_tag,
                    byte_offset: at as u64,
                    geometry_offset: geometry_offset as u32,
                    entity_genesis,
                    primary_id,
                    secondary_id,
                    geometry,
                });
            }
            at += 133;
        }
    }
    Ok(out)
}

pub(crate) struct ParsedSketchSurface {
    pub(crate) entity_genesis: Option<u64>,
    pub(crate) persistent_id: u64,
    pub(crate) u_degree: u32,
    pub(crate) v_degree: u32,
    pub(crate) u_knots: Vec<f64>,
    pub(crate) v_knots: Vec<f64>,
    pub(crate) control_points: Vec<Vec<Point3>>,
}

pub(crate) fn parse_sketch_surface(payload: &[u8]) -> Option<ParsedSketchSurface> {
    if payload.get(20) != Some(&1)
        || u32_at(payload, 21) != Some(2)
        || u32_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || u32_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
        || u32_at(payload, 77) != Some(11)
        || payload.get(81..92) != Some(b"surface_tag")
        || u32_at(payload, 92) != Some(23)
        || payload.get(96..119) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = read_u64(payload, 69);
    let persistent_id = read_u64(payload, 119)?;
    let point_count = usize::try_from(u32_at(payload, 127)?).ok()?;
    if point_count == 0 || point_count > 100_000 {
        return None;
    }
    let coordinate_count = point_count.checked_mul(3)?;
    let coordinate_bytes = point_count.checked_mul(24)?;
    let coordinates = f64s_at(payload, 131, coordinate_count)?;
    let degrees_at = 131usize.checked_add(coordinate_bytes)?;
    let u_degree = u32_at(payload, degrees_at)?;
    let v_degree = u32_at(payload, degrees_at.checked_add(4)?)?;
    let u_knot_count = usize::try_from(u32_at(payload, degrees_at.checked_add(8)?)?).ok()?;
    let u_knots_at = degrees_at.checked_add(12)?;
    let u_knots = f64s_at(payload, u_knots_at, u_knot_count)?;
    let v_count_at = u_knots_at.checked_add(u_knot_count.checked_mul(8)?)?;
    let v_knot_count = usize::try_from(u32_at(payload, v_count_at)?).ok()?;
    let v_knots_at = v_count_at.checked_add(4)?;
    let v_knots = f64s_at(payload, v_knots_at, v_knot_count)?;
    let grid_at = v_knots_at.checked_add(v_knot_count.checked_mul(8)?)?;
    let u_count = usize::try_from(u32_at(payload, grid_at)?).ok()?;
    let v_count = usize::try_from(u32_at(payload, grid_at.checked_add(4)?)?).ok()?;
    let expected_u_knots = u_count.checked_add(usize::try_from(u_degree).ok()?.checked_add(1)?)?;
    let expected_v_knots = v_count.checked_add(usize::try_from(v_degree).ok()?.checked_add(1)?)?;
    if u_degree == 0
        || v_degree == 0
        || u_count.checked_mul(v_count) != Some(point_count)
        || u_knot_count != expected_u_knots
        || v_knot_count != expected_v_knots
        || coordinates.iter().any(|value| !value.is_finite())
        || u_knots.iter().any(|value| !value.is_finite())
        || v_knots.iter().any(|value| !value.is_finite())
        || u_knots.windows(2).any(|pair| pair[0] > pair[1])
        || v_knots.windows(2).any(|pair| pair[0] > pair[1])
    {
        return None;
    }
    let control_points = coordinates
        .chunks_exact(3)
        .map(|point| Point3::new(point[0] * 10.0, point[1] * 10.0, point[2] * 10.0))
        .collect::<Vec<_>>()
        .chunks(v_count)
        .map(<[Point3]>::to_vec)
        .collect();
    Some(ParsedSketchSurface {
        entity_genesis,
        persistent_id,
        u_degree,
        v_degree,
        u_knots,
        v_knots,
        control_points,
    })
}

/// Decode tensor-product surface entities owned by spatial Design sketches.
pub fn decode_sketch_surfaces(scan: &ContainerScan) -> Result<Vec<SketchSurface>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while let Some(record_at) = next_indexed_record_offset(bytes, at) {
            at = record_at + 1;
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, record_at, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            let Some(record_index) = u32_at(bytes, after_tag) else {
                continue;
            };
            let payload = &bytes[record_at..];
            let Some(surface) = parse_sketch_surface(payload) else {
                continue;
            };
            out.push(SketchSurface {
                id: ids::native_sketch_surface_id(&entry.name, record_at),
                record_index,
                owner_reference: None,
                class_tag,
                byte_offset: record_at as u64,
                entity_genesis: surface.entity_genesis,
                persistent_id: surface.persistent_id,
                u_degree: surface.u_degree,
                v_degree: surface.v_degree,
                u_knots: surface.u_knots,
                v_knots: surface.v_knots,
                control_points: surface.control_points,
            });
        }
    }
    out.sort_by_key(|surface| surface.id.clone());
    Ok(out)
}

/// Bind relation-connected sketch geometry to its unique owning sketch.
pub(crate) fn bind_sketch_graph(
    entities: &[DesignEntityHeader],
    points: &mut [SketchPoint],
    curves: &mut [SketchCurveIdentity],
    surfaces: &mut [SketchSurface],
    relations: &mut [SketchRelation],
) -> Result<(), CodecError> {
    let sketch_owners = entities
        .iter()
        .filter(|entity| entity.object_kind == Some(DesignObjectKind::Sketch))
        .filter_map(|entity| {
            Some((
                (
                    native_stream(&entity.id)?,
                    u32::try_from(entity.entity_suffix).ok()?,
                ),
                entity.entity_id.as_str(),
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    for relation in relations.iter_mut() {
        let scope = native_stream(&relation.id).ok_or_else(|| {
            CodecError::Malformed(format!(
                "Fusion sketch relation {} has no Design stream identity",
                relation.record_index
            ))
        })?;
        relation.owner_entity_id = sketch_owners
            .get(&(scope, relation.owner_reference))
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "Fusion sketch relation {} in {scope} has no owning Design entity {}",
                    relation.record_index, relation.owner_reference,
                ))
            })?
            .to_string();
    }
    let typed_records = points
        .iter()
        .filter_map(|point| Some((native_stream(&point.id)?, point.record_index)))
        .chain(
            curves
                .iter()
                .filter_map(|curve| Some((native_stream(&curve.id)?, curve.record_index))),
        )
        .chain(
            surfaces
                .iter()
                .filter_map(|surface| Some((native_stream(&surface.id)?, surface.record_index))),
        )
        .collect::<std::collections::HashSet<_>>();
    let mut owners = std::collections::HashMap::new();
    let direct_owners = points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            curves
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
        .chain(
            surfaces
                .iter()
                .map(|surface| (&surface.id, surface.record_index, surface.owner_reference)),
        )
        .filter_map(|(id, record_index, owner_reference)| {
            Some((
                native_stream(id)?.to_owned(),
                record_index,
                owner_reference?,
            ))
        })
        .collect::<Vec<_>>();
    for (scope, record_index, owner_reference) in direct_owners {
        if let Some((owner_scope, _)) = sketch_owners
            .keys()
            .find(|(owner_scope, owner)| *owner_scope == scope && *owner == owner_reference)
        {
            owners.insert((*owner_scope, record_index), owner_reference);
        }
    }
    for relation in relations.iter() {
        let scope = native_stream(&relation.id).expect("relation stream checked above");
        for record_index in relation.members.iter().chain(&relation.return_members) {
            if !typed_records.contains(&(scope, *record_index)) {
                continue;
            }
            if owners
                .insert((scope, *record_index), relation.owner_reference)
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                return Err(CodecError::Malformed(format!(
                    "Fusion sketch record {record_index} in {scope} belongs to multiple sketches"
                )));
            }
        }
    }
    // Relation-free geometry carries no owner backlink of its own. The
    // `EntityGenesis`-form sketch container's paired record names every owned
    // record in its counted member run; backfill those owners after the
    // relation-derived pass, holding both sources to one owner per record.
    for entity in entities
        .iter()
        .filter(|entity| entity.object_kind == Some(DesignObjectKind::Sketch))
    {
        let (Some(scope), Ok(suffix)) = (
            native_stream(&entity.id),
            u32::try_from(entity.entity_suffix),
        ) else {
            continue;
        };
        for record_index in &entity.member_indices {
            if !typed_records.contains(&(scope, *record_index)) {
                continue;
            }
            if owners
                .insert((scope, *record_index), suffix)
                .is_some_and(|owner| owner != suffix)
            {
                return Err(CodecError::Malformed(format!(
                    "Fusion sketch record {record_index} in {scope} belongs to multiple sketches"
                )));
            }
        }
    }
    for point in points.iter_mut() {
        point.owner_reference = native_stream(&point.id)
            .and_then(|scope| owners.get(&(scope, point.record_index)))
            .copied();
    }
    for curve in curves.iter_mut() {
        curve.owner_reference = native_stream(&curve.id)
            .and_then(|scope| owners.get(&(scope, curve.record_index)))
            .copied();
    }
    for surface in surfaces.iter_mut() {
        surface.owner_reference = native_stream(&surface.id)
            .and_then(|scope| owners.get(&(scope, surface.record_index)))
            .copied();
    }
    let operands = points
        .iter()
        .filter_map(|point| {
            Some((
                (native_stream(&point.id)?, point.record_index),
                SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id,
                },
            ))
        })
        .chain(curves.iter().filter_map(|curve| {
            Some((
                (native_stream(&curve.id)?, curve.record_index),
                SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id,
                    secondary_id: curve.secondary_id,
                },
            ))
        }))
        .chain(surfaces.iter().filter_map(|surface| {
            Some((
                (native_stream(&surface.id)?, surface.record_index),
                SketchRelationOperand::Surface {
                    record_index: surface.record_index,
                    persistent_id: surface.persistent_id,
                },
            ))
        }))
        .collect::<std::collections::HashMap<_, _>>();
    let resolve = |scope: &str, indices: &[u32]| {
        indices
            .iter()
            .map(|record_index| {
                operands.get(&(scope, *record_index)).cloned().unwrap_or(
                    SketchRelationOperand::Record {
                        record_index: *record_index,
                    },
                )
            })
            .collect()
    };
    for relation in relations {
        let scope = native_stream(&relation.id).expect("relation stream checked above");
        relation.resolved_members = resolve(scope, &relation.members);
        relation.resolved_return_members = resolve(scope, &relation.return_members);
    }
    Ok(())
}

fn decode_sketch_curve_identity(payload: &[u8]) -> Option<(u64, u64, usize, Option<u64>)> {
    if let Some((primary, secondary)) = decode_sketch_curve_identity_variant(payload, 0, 2) {
        return Some((primary, secondary, 0, None));
    }
    if u32_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || u32_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = u64::from_le_bytes(payload.get(69..77)?.try_into().ok()?);
    decode_sketch_curve_identity_variant(payload, 52, 3)
        .map(|(primary, secondary)| (primary, secondary, 52, Some(entity_genesis)))
}

fn decode_sketch_curve_identity_variant(
    payload: &[u8],
    shift: usize,
    property_count: u32,
) -> Option<(u64, u64)> {
    if payload.get(20) != Some(&1)
        || u32_at(payload, 21) != Some(property_count)
        || u32_at(payload, 25 + shift) != Some(14)
        || payload.get(29 + shift..43 + shift) != Some(b"crv_primary_id")
        || u32_at(payload, 43 + shift) != Some(23)
        || payload.get(47 + shift..70 + shift) != Some(b"IntrinsicMetaTypeuint64")
        || u32_at(payload, 78 + shift) != Some(16)
        || payload.get(82 + shift..98 + shift) != Some(b"crv_secondary_id")
        || u32_at(payload, 98 + shift) != Some(23)
        || payload.get(102 + shift..125 + shift) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    Some((
        u64::from_le_bytes(payload.get(70 + shift..78 + shift)?.try_into().ok()?),
        u64::from_le_bytes(payload.get(125 + shift..133 + shift)?.try_into().ok()?),
    ))
}

fn decode_circular_arc(payload: &[u8]) -> Option<SketchCurveGeometry> {
    let values = (0..12)
        .map(|ordinal| f64_at(payload, 133 + ordinal * 8))
        .collect::<Option<Vec<_>>>()?;
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let normal = Vector3::new(values[3], values[4], values[5]);
    let reference_direction = Vector3::new(values[6], values[7], values[8]);
    let dot = normal.x * reference_direction.x
        + normal.y * reference_direction.y
        + normal.z * reference_direction.z;
    if (normal.norm() - 1.0).abs() > 1.0e-9
        || (reference_direction.norm() - 1.0).abs() > 1.0e-9
        || dot.abs() > 1.0e-9
        || values[9] <= 0.0
        || values[10].abs() > std::f64::consts::TAU + 1.0e-9
        || values[11].abs() > std::f64::consts::TAU + 1.0e-9
        || (values[11] - values[10]).abs() < 1.0e-12
    {
        return None;
    }
    Some(SketchCurveGeometry::Arc {
        center: Point3::new(values[0] * 10.0, values[1] * 10.0, values[2] * 10.0),
        normal,
        reference_direction,
        radius: values[9] * 10.0,
        start_angle: values[10],
        end_angle: values[11],
    })
}

pub(crate) fn decode_referenced_analytic(payload: &[u8]) -> Option<SketchCurveGeometry> {
    if payload.get(133) != Some(&1) || payload.get(138..144) != Some(&[0; 6]) {
        return None;
    }
    let shifted = payload.get(11..)?;
    decode_circular_arc(shifted)
        .or_else(|| decode_line(shifted))
        .or_else(|| decode_compact_planar_line(shifted))
}

/// Decode a text-frame boundary line after its two point references and
/// inline analytic-curve record. The first point reference has a trailing
/// null-role byte in addition to its six-byte reference padding. The inline
/// record repeats the enclosing record index and carries eight zero bytes
/// before the line values.
pub(crate) fn decode_text_frame_line(
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
) -> Option<(SketchCurveGeometry, usize)> {
    let mut cursor = geometry_shift.checked_add(133)?;
    for zero_count in [7, 6] {
        let (_, end) = marked_u32(payload, cursor)?;
        if !payload
            .get(end..end + zero_count)?
            .iter()
            .all(|byte| *byte == 0)
        {
            return None;
        }
        cursor = end + zero_count;
    }
    let (class_tag, after_tag) =
        lp_ascii_filtered(payload, cursor, 0..=2000, u8::is_ascii_graphic)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || u32_at(payload, after_tag) != Some(record_index)
        || payload.get(after_tag + 4..after_tag + 12) != Some(&[0; 8])
    {
        return None;
    }
    let values_at = after_tag.checked_add(12)?;
    Some((
        decode_line_values(payload, values_at)?,
        values_at.checked_add(12 * 8)?,
    ))
}

fn decode_sketch_nurbs(payload: &[u8]) -> Option<(SketchCurveGeometry, usize)> {
    let base = 133usize;
    let prefix = payload.get(base..base + 8)?;
    let carrier_reference = (prefix != [0xff; 8]).then(|| {
        u64::from_le_bytes(
            prefix
                .try_into()
                .expect("invariant: prefix is an 8-byte slice from payload.get(range) of length 8"),
        )
    });
    if u32_at(payload, base + 8) != Some(3) || payload.get(base + 88) != Some(&1) {
        return None;
    }
    let subtype_class_tag = std::str::from_utf8(payload.get(base + 12..base + 15)?)
        .ok()?
        .to_string();
    if !subtype_class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let degree = u32_at(payload, base + 90)?;
    let fit_tolerance = f64_at(payload, base + 94)?;
    let knot_count = usize::try_from(u32_at(payload, base + 102)?).ok()?;
    if u32_at(payload, base + 106)? as usize != knot_count
        || u32_at(payload, base + 110)? != 8
        || knot_count > 100_000
    {
        return None;
    }
    let knots = f64s_at(payload, base + 114, knot_count)?;
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(u32_at(payload, weights_at)?).ok()?;
    if u32_at(payload, weights_at + 4)? as usize != weight_count
        || u32_at(payload, weights_at + 8)? != 8
        || weight_count > 100_000
    {
        return None;
    }
    let weights = f64s_at(payload, weights_at + 12, weight_count)?;
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(u32_at(payload, points_at)?).ok()?;
    if (weight_count != 0 && point_count != weight_count)
        || u32_at(payload, points_at + 4)? as usize != point_count
        || u32_at(payload, points_at + 8)? != 8
        || knot_count != point_count.checked_add(degree as usize + 1)?
    {
        return None;
    }
    let coordinates = f64s_at(payload, points_at + 12, point_count.checked_mul(3)?)?;
    if knots.windows(2).any(|pair| pair[0] > pair[1])
        || weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
        || coordinates.iter().any(|value| !value.is_finite())
        || !fit_tolerance.is_finite()
    {
        return None;
    }
    let control_points = coordinates
        .chunks_exact(3)
        .map(|point| Point3::new(point[0] * 10.0, point[1] * 10.0, point[2] * 10.0))
        .collect();
    Some((
        SketchCurveGeometry::Nurbs {
            carrier_reference,
            subtype_class_tag,
            subtype_record_index: u32_at(payload, base + 15)?,
            degree,
            fit_tolerance: fit_tolerance * 10.0,
            scalar_width: 8,
            knots,
            weights,
            control_points,
        },
        points_at + 12 + point_count * 24,
    ))
}

pub(crate) fn decode_legacy_sketch_nurbs(payload: &[u8]) -> Option<(SketchCurveGeometry, usize)> {
    let base = 133usize;
    let prefix = payload.get(base..base + 8)?;
    let carrier_reference = (prefix != [0xff; 8]).then(|| {
        u64::from_le_bytes(
            prefix
                .try_into()
                .expect("invariant: prefix is an eight-byte slice"),
        )
    });
    if u32_at(payload, base + 8) != Some(3)
        || payload.get(base + 19..base + 27) != Some(&[0; 8])
        || payload.get(base + 27) != Some(&1)
        || payload.get(base + 32..base + 42) != Some(&[0; 10])
        || payload.get(base + 50..base + 55) != Some(&[0; 5])
        || payload.get(base + 55) != Some(&1)
        || payload.get(base + 60..base + 66) != Some(&[0; 6])
        || payload.get(base + 66) != Some(&1)
        || payload.get(base + 71..base + 77) != Some(&[0; 6])
        || payload.get(base + 77) != Some(&1)
        || payload.get(base + 80..base + 88) != Some(&[0; 8])
        || payload.get(base + 88).is_none_or(|value| *value > 1)
        || payload.get(base + 89).is_none_or(|value| *value > 1)
        || payload.get(base + 94..base + 102)
            != Some(&[0x95, 0xd6, 0x26, 0xe8, 0x0b, 0x2e, 0x11, 0x3e])
    {
        return None;
    }
    let subtype_class_tag = std::str::from_utf8(payload.get(base + 12..base + 15)?)
        .ok()?
        .to_string();
    if !subtype_class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let degree = u32_at(payload, base + 90)?;
    let fit_tolerance = f64_at(payload, base + 42)?;
    let knot_count = usize::try_from(u32_at(payload, base + 102)?).ok()?;
    let knot_capacity = usize::try_from(u32_at(payload, base + 106)?).ok()?;
    if degree == 0
        || knot_capacity < knot_count
        || u32_at(payload, base + 110)? != 8
        || knot_capacity > 100_000
    {
        return None;
    }
    let knots = f64s_at(payload, base + 114, knot_count)?;
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(u32_at(payload, weights_at)?).ok()?;
    let weight_capacity = usize::try_from(u32_at(payload, weights_at + 4)?).ok()?;
    if weight_capacity < weight_count
        || u32_at(payload, weights_at + 8)? != 8
        || weight_capacity > 100_000
    {
        return None;
    }
    let weights = f64s_at(payload, weights_at + 12, weight_count)?;
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(u32_at(payload, points_at)?).ok()?;
    let point_capacity = usize::try_from(u32_at(payload, points_at + 4)?).ok()?;
    if (weight_count != 0 && point_count != weight_count)
        || point_capacity < point_count
        || point_capacity > 100_000
        || u32_at(payload, points_at + 8)? != 8
        || knot_count != point_count.checked_add(degree as usize + 1)?
    {
        return None;
    }
    let coordinates = f64s_at(payload, points_at + 12, point_count.checked_mul(3)?)?;
    if knots.windows(2).any(|pair| pair[0] > pair[1])
        || weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
        || coordinates.iter().any(|value| !value.is_finite())
        || !fit_tolerance.is_finite()
    {
        return None;
    }
    let control_points = coordinates
        .chunks_exact(3)
        .map(|point| Point3::new(point[0] * 10.0, point[1] * 10.0, point[2] * 10.0))
        .collect();
    Some((
        SketchCurveGeometry::Nurbs {
            carrier_reference,
            subtype_class_tag,
            subtype_record_index: u32_at(payload, base + 15)?,
            degree,
            fit_tolerance: fit_tolerance * 10.0,
            scalar_width: 8,
            knots,
            weights,
            control_points,
        },
        points_at + 12 + point_count * 24,
    ))
}

pub(crate) fn decode_line(payload: &[u8]) -> Option<SketchCurveGeometry> {
    decode_line_values(payload, 133)
}

pub(crate) fn decode_compact_planar_line(payload: &[u8]) -> Option<SketchCurveGeometry> {
    let values_at = 133;
    let values = (0..9)
        .map(|ordinal| f64_at(payload, values_at + ordinal * 8))
        .collect::<Option<Vec<_>>>()?;
    if values.iter().any(|value| !value.is_finite())
        || values[2] != 0.0
        || values[5] != 0.0
        || values[8] != 0.0
    {
        return None;
    }
    let (_, reference_end) = marked_u32(payload, values_at + 9 * 8)?;
    if payload.get(reference_end..reference_end + 6) != Some(&[0; 6]) {
        return None;
    }
    decode_line_components(&values, Vector3::new(0.0, 0.0, 1.0))
}

fn decode_line_values(payload: &[u8], values_at: usize) -> Option<SketchCurveGeometry> {
    let values = (0..12)
        .map(|ordinal| f64_at(payload, values_at + ordinal * 8))
        .collect::<Option<Vec<_>>>()?;
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let stored_normal = Vector3::new(values[9], values[10], values[11]);
    decode_line_components(&values, stored_normal)
}

fn decode_line_components(values: &[f64], stored_normal: Vector3) -> Option<SketchCurveGeometry> {
    let displacement = Vector3::new(values[3], values[4], values[5]);
    let direction = Vector3::new(values[6], values[7], values[8]);
    let length = displacement.norm();
    if length <= 0.0 {
        return None;
    }
    let displacement_direction = Vector3::new(
        displacement.x / length,
        displacement.y / length,
        displacement.z / length,
    );
    if (direction.norm() - 1.0).abs() > 1.0e-9 || (stored_normal.norm() - 1.0).abs() > 1.0e-9 {
        return None;
    }
    // Start plus displacement carries the bounded line and is corroborated by
    // the persistent endpoint records. Imported sketches can retain a stale
    // auxiliary unit direction, so derive the neutral tangent from the exact
    // displacement just as the normal is orthogonalized below.
    let direction = displacement_direction;
    // The stored line normal is an auxiliary orientation vector. Imported
    // legacy sketches can retain a small component along the line direction;
    // remove that component so the typed carrier maintains its orthonormal
    // invariant without changing the line's endpoints or orientation side.
    let dot = direction.x * stored_normal.x
        + direction.y * stored_normal.y
        + direction.z * stored_normal.z;
    let normal = Vector3::new(
        stored_normal.x - dot * direction.x,
        stored_normal.y - dot * direction.y,
        stored_normal.z - dot * direction.z,
    );
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= 1.0e-12 {
        return None;
    }
    let normal = Vector3::new(
        normal.x / normal_length,
        normal.y / normal_length,
        normal.z / normal_length,
    );
    let start = Point3::new(values[0] * 10.0, values[1] * 10.0, values[2] * 10.0);
    Some(SketchCurveGeometry::Line {
        start,
        end: Point3::new(
            start.x + displacement.x * 10.0,
            start.y + displacement.y * 10.0,
            start.z + displacement.z * 10.0,
        ),
        direction,
        normal,
    })
}

pub(crate) struct ParsedSketchRelation {
    pub(crate) members: Vec<u32>,
    pub(crate) member_offsets: Vec<usize>,
    pub(crate) member_roles: Vec<u32>,
    pub(crate) auxiliary_references: Vec<u32>,
    pub(crate) auxiliary_reference_offsets: Vec<usize>,
    pub(crate) owner_reference: u32,
    pub(crate) owner_reference_offset: usize,
    pub(crate) state: u64,
    pub(crate) state_offset: usize,
    pub(crate) entity_genesis: Option<u64>,
    pub(crate) text_glyph_transforms: Option<Vec<[[f64; 4]; 4]>>,
    pub(crate) return_members: Vec<u32>,
    pub(crate) return_member_offsets: Vec<usize>,
    pub(crate) parsed_end: usize,
}

/// Largest plausible member-role code; larger values indicate a misparse.
const MAX_MEMBER_ROLE: u32 = 10_000;

pub(crate) fn parse_sketch_relation(
    payload: &[u8],
    owners: &std::collections::HashSet<u32>,
) -> Option<ParsedSketchRelation> {
    if payload.get(19) != Some(&1) {
        return None;
    }
    let member_count = usize::try_from(u32_at(payload, 20)?).ok()?;
    if member_count > 64 {
        return None;
    }
    let mut cursor = 24;
    let mut members = Vec::with_capacity(member_count);
    let mut member_offsets = Vec::with_capacity(member_count);
    let mut member_roles = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        let (value, end) = marked_u32(payload, cursor)?;
        members.push(value);
        member_offsets.push(cursor + 1);
        // A member entry is the reference, six zero bytes, and a u32 role. A
        // reference marker directly at the entry end is a role-less entry.
        cursor = next_reference_marker(payload, end)?;
        let role = if cursor >= end + 10 && payload.get(end..end + 6) == Some(&[0u8; 6]) {
            u32_at(payload, end + 6).filter(|role| *role <= MAX_MEMBER_ROLE)
        } else {
            None
        };
        member_roles.push(role.unwrap_or(0));
    }
    // An optional `EntityGenesis` metadata block follows the member run: a
    // `0x01`-marked u32 1, the two length-prefixed key strings, and the u64
    // origin bitfield.
    let mut entity_genesis = None;
    if payload.get(cursor) == Some(&1)
        && u32_at(payload, cursor + 1) == Some(1)
        && lp_ascii_filtered(payload, cursor + 5, 0..=2000, u8::is_ascii_graphic)
            .is_some_and(|(key, _)| key == "EntityGenesis")
    {
        let (_, after_key) =
            lp_ascii_filtered(payload, cursor + 5, 0..=2000, u8::is_ascii_graphic)?;
        let (meta_type, after_type) =
            lp_ascii_filtered(payload, after_key, 0..=2000, u8::is_ascii_graphic)?;
        if meta_type == "IntrinsicMetaTypeuint64" {
            entity_genesis = Some(u64::from_le_bytes(
                payload.get(after_type..after_type + 8)?.try_into().ok()?,
            ));
            cursor = next_reference_marker(payload, after_type + 8)?;
        }
    }
    let mut auxiliary_references = Vec::new();
    let mut auxiliary_reference_offsets = Vec::new();
    // A text-path relation follows the `EntityGenesis` block with a `0x01`
    // flag, the marked text-entity reference and its zero padding, a u32
    // character count, and per-character blocks of `u32 16` and sixteen f64
    // values. Parse the run structurally so the f64 payload's bytes are not
    // misread as auxiliary references; the owning sketch reference follows
    // the last block directly.
    let mut text_glyph_transforms = None;
    if payload.get(cursor) == Some(&1) && payload.get(cursor + 1) == Some(&1) {
        if let Some((text_reference, transforms, after)) = parse_text_glyph_run(payload, cursor + 1)
        {
            if marked_u32(payload, after).is_some_and(|(reference, _)| owners.contains(&reference))
            {
                auxiliary_references.push(text_reference);
                auxiliary_reference_offsets.push(cursor + 2);
                text_glyph_transforms = Some(transforms);
                cursor = after;
            }
        }
    }
    let (owner_reference, owner_reference_offset, end) = loop {
        let (reference, end) = marked_u32(payload, cursor)?;
        if owners.contains(&reference) {
            break (reference, cursor + 1, end);
        }
        auxiliary_references.push(reference);
        auxiliary_reference_offsets.push(cursor + 1);
        cursor = next_reference_marker(payload, end)?;
    };
    // In records carrying an `EntityGenesis` block the constraint mask is a
    // u64 six zero bytes after the owner reference. Records without that
    // block store a `0x01`-marked or direct u32 mask after the owner padding.
    let (state, state_offset, end) = if payload.get(end) == Some(&1) {
        let (state, after) = marked_u32(payload, end)?;
        (u64::from(state), end + 1, after)
    } else if entity_genesis.is_some() {
        if payload.get(end..end + 6) != Some(&[0u8; 6]) {
            return None;
        }
        let state = u64::from_le_bytes(payload.get(end + 6..end + 14)?.try_into().ok()?);
        (state, end + 6, end + 14)
    } else {
        let at = next_nonzero(payload, end)?;
        if payload.get(at) == Some(&1) {
            let (state, after) = marked_u32(payload, at)?;
            (u64::from(state), at + 1, after)
        } else {
            (u64::from(u32_at(payload, at)?), at, at + 4)
        }
    };
    cursor = next_nonzero(payload, end)?;
    let return_count = usize::try_from(u32_at(payload, cursor)?).ok()?;
    if return_count > 64 {
        return None;
    }
    cursor += 4;
    let mut return_members = Vec::with_capacity(return_count);
    let mut return_member_offsets = Vec::with_capacity(return_count);
    for ordinal in 0..return_count {
        cursor = next_reference_marker(payload, cursor)?;
        let (value, end) = marked_u32(payload, cursor)?;
        return_members.push(value);
        return_member_offsets.push(cursor + 1);
        cursor = end;
        if ordinal + 1 < return_count {
            cursor = next_reference_marker(payload, cursor)?;
        }
    }
    let parsed_end = cursor;
    Some(ParsedSketchRelation {
        members,
        member_offsets,
        member_roles,
        auxiliary_references,
        auxiliary_reference_offsets,
        owner_reference,
        owner_reference_offset,
        state,
        state_offset,
        entity_genesis,
        text_glyph_transforms,
        return_members,
        return_member_offsets,
        parsed_end,
    })
}

/// Parse a text-path glyph run at `at`: the marked text-entity reference,
/// six zero bytes, a u32 character count, and that many blocks of `u32 16`
/// followed by sixteen finite f64 values forming a row-major 4×4 character
/// placement transform. Returns the text reference, the transforms in
/// character order, and the offset directly after the last block.
type TextGlyphRun = (u32, Vec<[[f64; 4]; 4]>, usize);

fn parse_text_glyph_run(payload: &[u8], at: usize) -> Option<TextGlyphRun> {
    let (text_reference, end) = marked_u32(payload, at)?;
    if payload.get(end..end + 6) != Some(&[0u8; 6]) {
        return None;
    }
    let count = usize::try_from(u32_at(payload, end + 6)?).ok()?;
    if !(1..=4096).contains(&count) {
        return None;
    }
    let mut cursor = end + 10;
    let mut transforms = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(payload, cursor) != Some(16) {
            return None;
        }
        let mut transform = [[0.0; 4]; 4];
        for ordinal in 0..16 {
            let value = f64::from_le_bytes(
                payload
                    .get(cursor + 4 + ordinal * 8..cursor + 12 + ordinal * 8)?
                    .try_into()
                    .ok()?,
            );
            if !value.is_finite() {
                return None;
            }
            transform[ordinal / 4][ordinal % 4] = value;
        }
        transforms.push(transform);
        cursor += 132;
    }
    Some((text_reference, transforms, cursor))
}

pub(crate) fn next_indexed_record_offset(bytes: &[u8], mut position: usize) -> Option<usize> {
    while position + 11 <= bytes.len() {
        let Some((class_tag, after_tag)) =
            lp_ascii_filtered(bytes, position, 0..=2000, u8::is_ascii_graphic)
        else {
            position += 1;
            continue;
        };
        if class_tag.len() == 3
            && class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && bytes.get(after_tag..after_tag + 4).is_some()
        {
            return Some(position);
        }
        position += 1;
    }
    None
}

pub(crate) fn next_indexed_record_offset_with_index(
    bytes: &[u8],
    mut position: usize,
    record_index: u32,
) -> Option<usize> {
    loop {
        let offset = next_indexed_record_offset(bytes, position)?;
        let (_, after_tag) = lp_ascii_filtered(bytes, offset, 0..=2000, u8::is_ascii_graphic)?;
        if u32_at(bytes, after_tag) == Some(record_index) {
            return Some(offset);
        }
        position = offset.checked_add(1)?;
    }
}

fn marked_u32(bytes: &[u8], position: usize) -> Option<(u32, usize)> {
    (bytes.get(position) == Some(&1)).then_some((u32_at(bytes, position + 1)?, position + 5))
}

fn next_reference_marker(bytes: &[u8], mut position: usize) -> Option<usize> {
    while position + 5 <= bytes.len() {
        if bytes.get(position) == Some(&1) {
            let reference = u32_at(bytes, position + 1)?;
            if reference <= 10_000_000 {
                return Some(position);
            }
        }
        position += 1;
    }
    None
}

fn next_nonzero(bytes: &[u8], mut position: usize) -> Option<usize> {
    while bytes.get(position) == Some(&0) {
        position += 1;
    }
    (position + 4 <= bytes.len()).then_some(position)
}

struct SketchReferenceList {
    record_reference: Option<u32>,
    record_reference_offset: usize,
    declared_count: u32,
    references: Vec<u32>,
    reference_offsets: Vec<usize>,
    end: usize,
}

fn decode_reference_list(bytes: &[u8], position: usize) -> Option<SketchReferenceList> {
    // The eight-byte base-record slot is either a u32 record reference with a
    // zero high half or the all-ones sentinel marking a sketch with no base
    // record; the list grammar is identical in both forms.
    let record_reference = if bytes.get(position..position + 8) == Some(&[0xFF; 8]) {
        None
    } else {
        let reference = u32::from_le_bytes(bytes.get(position..position + 4)?.try_into().ok()?);
        if bytes.get(position + 4..position + 8) != Some(&[0; 4]) {
            return None;
        }
        Some(reference)
    };
    if bytes.get(position + 8) != Some(&1) {
        return None;
    }
    let declared_count =
        u32::from_le_bytes(bytes.get(position + 9..position + 13)?.try_into().ok()?);
    let mut cursor = position + 13;
    let mut references = Vec::new();
    let mut reference_offsets = Vec::new();
    while bytes.get(cursor) == Some(&1) && bytes.get(cursor + 5..cursor + 11) == Some(&[0; 6]) {
        references.push(u32::from_le_bytes(
            bytes.get(cursor + 1..cursor + 5)?.try_into().ok()?,
        ));
        reference_offsets.push(cursor + 1);
        cursor += 11;
    }
    (references.len() == declared_count as usize).then_some(SketchReferenceList {
        record_reference,
        record_reference_offset: position,
        declared_count,
        references,
        reference_offsets,
        end: cursor,
    })
}
