// SPDX-License-Identifier: Apache-2.0
//! Parse sketch placements, `MetaStream` types, headers, relations, and geometry.

use crate::bytes::{
    is_guid_relaxed, lp_ascii_filtered, lp_utf16_bounded, take_reference, Reference,
};
use crate::container::{role, ContainerScan};
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use crate::records::{
    DesignEntityHeader, DesignParameterScope, DesignRecordHeader, DesignSketchPlacement,
    DesignType, LostEdgeReference, PersistentReference, PersistentReferenceKind,
    SketchConstraintKind, SketchCurveGeometry, SketchCurveIdentity, SketchPoint, SketchRelation,
    SketchRelationOperand, SketchSurface, SketchText, DESIGN_MODULE_SKETCH,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::le::{f64_at, f64s_at, take_f32, u32_at, u64_at as read_u64, utf16le_at};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::Color;
use std::collections::HashMap;

/// Byte offsets of every indexed-record header in one `BulkStream`, grouped by
/// the record index carried at header offset seven.
pub(crate) struct IndexedRecordOffsets {
    by_record_index: HashMap<u32, Vec<usize>>,
}

impl IndexedRecordOffsets {
    /// Index every exact indexed-record header in `bytes` in one forward pass.
    pub(crate) fn build(bytes: &[u8]) -> Self {
        let mut by_record_index = HashMap::<u32, Vec<usize>>::new();
        for at in indexed_record_offsets(bytes) {
            if let Some(record_index) = indexed_record_index(bytes, at) {
                by_record_index.entry(record_index).or_default().push(at);
            }
        }
        Self { by_record_index }
    }

    /// Ascending header offsets carrying `record_index`.
    pub(crate) fn offsets(&self, record_index: u32) -> &[usize] {
        self.by_record_index
            .get(&record_index)
            .map_or(&[], Vec::as_slice)
    }

    /// Record indexes and their ascending header offsets.
    pub(crate) fn records(&self) -> impl Iterator<Item = (u32, &[usize])> {
        self.by_record_index
            .iter()
            .map(|(record_index, offsets)| (*record_index, offsets.as_slice()))
    }

    /// The first header at or after `position` that carries `record_index`.
    pub(crate) fn first_at_or_after(&self, position: usize, record_index: u32) -> Option<usize> {
        let offsets = self.offsets(record_index);
        offsets
            .get(offsets.partition_point(|offset| *offset < position))
            .copied()
    }

    /// Consecutive header offsets carrying `record_index`, each pair delimiting
    /// one frame of that record.
    pub(crate) fn frames(&self, record_index: u32) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.offsets(record_index)
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
    }
}

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
    let mut record_offsets = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        record_offsets.insert(
            ids::native_scope(&entry.name),
            IndexedRecordOffsets::build(scan.entry_bytes(&entry.name)?),
        );
    }
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
        let Some(records) = record_offsets.get(&ids::native_scope(&entry.name)) else {
            continue;
        };
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
                records,
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
    for entity in entities.iter().filter(|entity| entity.in_sketch_module()) {
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
        let Some(records) = record_offsets.get(stream) else {
            continue;
        };
        let Some(mut placement) = parse_member_run_head_placement(bytes, entity, records) else {
            continue;
        };
        let next_entity_offset = entities
            .iter()
            .filter(|candidate| {
                candidate.in_sketch_module()
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
    records: &IndexedRecordOffsets,
) -> Option<DesignSketchPlacement> {
    let start = usize::try_from(entity.byte_offset).ok()?;
    // Locate the paired same-index record after the entity header.
    let entity_index = u32::try_from(entity.entity_suffix).ok()?;
    let paired_at = records.first_at_or_after(start.checked_add(1)?, entity_index)?;
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
    let head_at = records.offsets(head_index).first().copied()?;
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
    records: &IndexedRecordOffsets,
) -> Vec<DesignSketchPlacement> {
    let mut out = Vec::new();
    for pair in records.offsets(record_index).windows(2) {
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

/// Skip a `u32`-counted run of fixed-width elements at `at`, returning the
/// offset past it.
fn skip_counted_run(bytes: &[u8], at: usize, stride: usize) -> Option<usize> {
    let count = usize::try_from(u32_at(bytes, at)?).ok()?;
    let end = count
        .checked_mul(stride)
        .and_then(|size| at.checked_add(4)?.checked_add(size))?;
    (end <= bytes.len()).then_some(end)
}

/// Parse one Design `MetaStream` segment ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)) into its type table.
///
/// The stream is a segment header, a type table, a named-entity list, two
/// record indexes, and an optional trailing property block, in that order. Each
/// type-table entry is an LP-ASCII type GUID, an LP-ASCII base type GUID that is
/// empty for a root type, a u32 type version, an LP-ASCII add-in name, and a
/// u32-counted run of u64 design-entity ids. The whole stream must close on its
/// own end, which pins the header shape; a stream that does not is rejected
/// whole rather than parsed in part. Returned entries carry no `id`.
pub(crate) fn parse_design_type_table(bytes: &[u8]) -> Option<Vec<DesignType>> {
    // Header: short segment type name, segment id, asset GUID, serializer
    // magic and its magic-gated integer group, full segment type name, add-in
    // name, and the segment type code.
    let (_, at) = lp_ascii_filtered(bytes, 0, 1..=256, u8::is_ascii_graphic)?;
    let at = at.checked_add(4)?;
    let (_, at) = lp_utf16_bounded(bytes, at, 0..=256)?;
    let magic = u32_at(bytes, at)?;
    let at = at.checked_add(if magic == 1234 { 16 } else { 8 })?;
    let (_, at) = lp_ascii_filtered(bytes, at, 1..=256, u8::is_ascii_graphic)?;
    let (_, at) = lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic)?;
    let mut at = at.checked_add(8)?;
    let count = u32_at(bytes, at)?;
    at = at.checked_add(4)?;
    let mut out = Vec::new();
    for _ in 0..count {
        let entry_at = at;
        let type_guid_offset = at.checked_add(4)?;
        let (type_guid, next) = lp_ascii_filtered(bytes, at, 1..=256, u8::is_ascii_graphic)
            .filter(|(guid, _)| is_guid_relaxed(guid))?;
        at = next;
        let base_type_guid_offset = at.checked_add(4)?;
        let (base_type_guid, next) = lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic)
            .filter(|(guid, _)| guid.is_empty() || is_guid_relaxed(guid))?;
        at = next;
        let version_offset = at;
        let version = u32_at(bytes, at)?;
        at = at.checked_add(4)?;
        let (module, next) = lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic)?;
        at = next;
        let id_count = usize::try_from(u32_at(bytes, at)?).ok()?;
        let ids_at = at.checked_add(4)?;
        let ids_end = id_count
            .checked_mul(8)
            .and_then(|size| ids_at.checked_add(size))?;
        let raw_ids = bytes.get(ids_at..ids_end)?;
        at = ids_end;
        out.push(DesignType {
            id: String::new(),
            byte_offset: entry_at as u64,
            type_guid,
            type_guid_offset: type_guid_offset as u64,
            base_type_guid_offset: (!base_type_guid.is_empty())
                .then_some(base_type_guid_offset as u64),
            base_type_guid: (!base_type_guid.is_empty()).then_some(base_type_guid),
            version,
            version_offset: version_offset as u64,
            module,
            entity_ids: raw_ids
                .chunks_exact(8)
                .map(|raw| {
                    u64::from_le_bytes(
                        raw.try_into()
                            .expect("invariant: chunks_exact(8) yields 8-byte slices"),
                    )
                })
                .collect(),
            entity_id_offsets: (0..id_count)
                .map(|index| (ids_at + index * 8) as u64)
                .collect(),
        });
    }
    // The named-entity list, the record index, and the secondary index. A
    // legacy segment may end at any of the trailing next-entity counter, the
    // flag, or the property block.
    at = skip_counted_run(bytes, at, 8)?;
    at = skip_counted_run(bytes, at, 16)?;
    at = skip_counted_run(bytes, at, 16)?;
    if bytes.len() - at >= 8 {
        at += 8;
    }
    if bytes.len() - at >= 4 {
        at += 4;
    }
    if bytes.len() - at >= 4 {
        let properties = u32_at(bytes, at)?;
        at = at.checked_add(4)?;
        for _ in 0..properties {
            let (_, next) = lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic)?;
            at = next.checked_add(4)?;
        }
    }
    (at == bytes.len()).then_some(out)
}

/// Decode the type table of every design `MetaStream` entry in `scan`
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)). A stream that does not close on its own end contributes
/// nothing.
pub fn decode_types(scan: &ContainerScan) -> Result<Vec<DesignType>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::METASTREAM && entry.name.contains("Design"))
    {
        let Some(types) = parse_design_type_table(scan.entry_bytes(&entry.name)?) else {
            continue;
        };
        out.extend(types.into_iter().map(|mut design_type| {
            design_type.id = ids::native_design_type_id(&entry.name, design_type.byte_offset);
            design_type
        }));
    }
    Ok(out)
}

/// Type GUID and record version of the types registered by the design
/// `MetaStream` beside `bulk_entry_name`, keyed by the design entity ids those
/// types own. A record carries neither of its own: its class tag selects a
/// type-table entry, that entry's version fixes the member sequence the record
/// was written under, and that entry's GUID is the type's only identity that
/// holds across segments, since a class tag is `256` plus a segment-local index.
pub fn stream_types_by_entity<'a>(
    types: &'a [DesignType],
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

/// Recognize either legacy sketch-container tail. A counted container owns
/// its complete member run. A localized container omits that run and is
/// accepted only when its paired record names an exact placement-head frame.
pub(crate) fn parse_legacy_sketch_container_members(
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
    records: &IndexedRecordOffsets,
) -> Option<(Vec<u32>, Vec<u64>)> {
    if let Some(members) = parse_legacy_sketch_member_run(bytes, primary_at, entity_suffix) {
        return Some(members);
    }
    let entity = DesignEntityHeader {
        id: String::new(),
        byte_offset: primary_at as u64,
        entity_suffix: u64::from(entity_suffix),
        entity_id: format!("Sketch_{entity_suffix}"),
        class_tag: String::new(),
        optional_slot_present: false,
        module: Some(DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: None,
        record_reference_offset: None,
        declared_reference_count: None,
        reference_indices: Vec::new(),
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };
    parse_member_run_head_placement(bytes, &entity, records)?;
    Some((Vec::new(), Vec::new()))
}

/// Decode every self-validating per-entity design `BulkStream` header (spec
/// [§8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)): a three-digit class tag, an entity suffix, a UTF-16LE entity ID
/// whose numeric suffix must match the header's entity suffix, and, for
/// sketch-typed entities, the trailing reference-list header. Headers occur in
/// the fixed layout or in the `EntityGenesis` layout.
pub fn decode_entity_headers(scan: &ContainerScan) -> Result<Vec<DesignEntityHeader>, CodecError> {
    let mut out = Vec::new();
    // A design entity id is unique inside its own segment and not across the
    // archive, so the module map is per stream: one archive's Design segments
    // reuse ids for entities of different modules, and a flat map would let
    // whichever segment is read first name the module for all of them.
    let mut entity_modules = HashMap::<String, HashMap<u64, String>>::new();
    let types = decode_types(scan)?;
    let mut legacy_sketch_candidates = HashMap::<String, std::collections::HashSet<u32>>::new();
    for design_type in types {
        if let Some(stream) = native_stream(&design_type.id) {
            let stream_modules = entity_modules.entry(stream.to_owned()).or_default();
            for &entity_id in &design_type.entity_ids {
                stream_modules
                    .entry(entity_id)
                    .or_insert_with(|| design_type.module.clone());
            }
        }
        if design_type.module == DESIGN_MODULE_SKETCH {
            let Some(stream) = native_stream(&design_type.id) else {
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
                    design_type
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
        // Modules come from the type table of this stream's own `MetaStream`.
        let stream_modules = entry
            .name
            .strip_suffix("BulkStream.dat")
            .map(|prefix| ids::native_scope(&format!("{prefix}MetaStream.dat")))
            .and_then(|meta_scope| entity_modules.get(&meta_scope));
        let indexed_offsets = indexed_record_offsets(bytes).collect::<Vec<_>>();
        for &start in &indexed_offsets {
            let Some(class_tag) = bytes.get(start + 4..start + 7) else {
                continue;
            };
            let settled = parse_settled_entity_header(bytes, start);
            let genesis_form = settled.is_none();
            let Some((entity_suffix, entity_id, optional_slot_present, end)) =
                settled.or_else(|| parse_genesis_entity_header(bytes, start))
            else {
                continue;
            };
            let module = stream_modules
                .and_then(|modules| modules.get(&entity_suffix))
                .cloned();
            let in_sketch_module = module.as_deref() == Some(DESIGN_MODULE_SKETCH);
            let (
                record_reference,
                record_reference_offset,
                declared_reference_count,
                reference_indices,
                reference_offsets,
                record_end,
            ) = if in_sketch_module {
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
            let (member_indices, member_offsets) = if genesis_form && in_sketch_module {
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
                module,
                record_reference,
                record_reference_offset,
                declared_reference_count,
                reference_indices,
                reference_offsets,
                member_indices,
                member_offsets,
            });
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
        if candidates.is_empty() {
            continue;
        }
        let records = IndexedRecordOffsets::build(bytes);
        let scope = ids::native_scope(&entry.name);
        let mut existing = out
            .iter()
            .filter(|entity| native_stream(&entity.id) == Some(scope.as_str()))
            .filter_map(|entity| u32::try_from(entity.entity_suffix).ok())
            .collect::<std::collections::HashSet<_>>();
        for &start in &indexed_offsets {
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
                parse_legacy_sketch_container_members(bytes, start, entity_suffix, &records)
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
                module: Some(DESIGN_MODULE_SKETCH.to_owned()),
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
        for position in indexed_record_offsets(bytes) {
            let record_index = indexed_record_index(bytes, position)
                .expect("validated indexed-record header carries a four-byte record index");
            let scope = ids::native_scope(&entry.name);
            if wanted.contains(&(scope, record_index)) && emitted.insert(record_index) {
                out.push(DesignRecordHeader {
                    id: ids::native_design_record_header_id(&entry.name, position),
                    record_index,
                    class_tag: std::str::from_utf8(&bytes[position + 4..position + 7])
                        .expect("validated indexed-record class tag is ASCII")
                        .to_owned(),
                    byte_offset: position as u64,
                });
            }
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
    // A record carries no class identity of its own: its class tag selects an
    // entry in its segment's own type table, and only that entry's GUID names
    // the class across segments.
    let types = decode_types(scan)?;
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let stream_types = stream_types_by_entity(&types, &entry.name);
        let scope = ids::native_scope(&entry.name);
        let owners = entities
            .iter()
            .filter(|entity| {
                native_stream(&entity.id) == Some(scope.as_str()) && entity.in_sketch_module()
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
            let class = stream_types
                .get(&u64::from(record.record_index))
                .and_then(|(type_guid, version)| SketchRelationClass::of(type_guid, *version));
            let parsed = match class {
                Some(class) => parse_classed_sketch_relation(payload, class),
                None => parse_sketch_relation(payload, &owners),
            };
            let Some(parsed) = parsed else {
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
                member_relation_ordinals: parsed.member_relation_ordinals,
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

/// Decode the pattern definition a relation's class members carry, reading them
/// beside the auxiliary references the parse recorded. Circular patterns store
/// the angle- and count-parameter references, the evaluated f64 total angle six
/// zero bytes after the count-parameter reference, and the evaluated u32
/// instance count directly after it. Rectangular patterns store, per direction,
/// the evaluated u32 count, the count-parameter reference, a three-component
/// f64 unit direction six zero bytes after that reference, the evaluated f64
/// seed-to-final-instance span, and the distance-parameter reference. Text-frame
/// relations repeat the sketch-text member as an auxiliary reference.
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
    if parsed.state == 0x2000_0000 {
        // Each direction clause writes its evaluated count directly before its
        // count-parameter reference, so the count is five bytes before that
        // reference's target. The clauses follow the class members that precede
        // them, which is one leading reference or none; where the class was not
        // named that is read off the length of the auxiliary run.
        let clause_ordinal = parsed
            .rectangular_clause_ordinal
            .unwrap_or(usize::from(parsed.auxiliary_references.len() == 5));
        if parsed.auxiliary_references.len() < clause_ordinal + 4 {
            return None;
        }
        let mut directions = Vec::with_capacity(2);
        let clauses = [
            (
                parsed.auxiliary_reference_offsets[clause_ordinal].checked_sub(5)?,
                clause_ordinal,
                clause_ordinal + 1,
            ),
            (
                parsed.auxiliary_reference_offsets[clause_ordinal + 2].checked_sub(5)?,
                clause_ordinal + 2,
                clause_ordinal + 3,
            ),
        ];
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

/// Read a class property block: a presence byte, and when it is `01`, a u32
/// count and that many `(key, type name, value)` triples.
///
/// Which keys a record carries varies by record, so a caller addresses a
/// property by name. Reading the block by fixed offset misframes every record
/// whose key set differs from the one the offsets were taken from.
pub(crate) fn read_property_block(
    payload: &[u8],
    cursor: &mut usize,
) -> Option<Vec<(String, u64)>> {
    let mut properties = Vec::new();
    match payload.get(*cursor)? {
        0 => *cursor += 1,
        1 => {
            *cursor += 1;
            let count = usize::try_from(u32_at(payload, *cursor)?).ok()?;
            if count > MAX_RELATION_RUN {
                return None;
            }
            *cursor += 4;
            for _ in 0..count {
                let (key, after_key) =
                    lp_ascii_filtered(payload, *cursor, 0..=256, u8::is_ascii_graphic)?;
                let (type_name, after_type) =
                    lp_ascii_filtered(payload, after_key, 0..=256, u8::is_ascii_graphic)?;
                if type_name != "IntrinsicMetaTypeuint64" {
                    return None;
                }
                properties.push((key, read_u64(payload, after_type)?));
                *cursor = after_type.checked_add(8)?;
            }
        }
        _ => return None,
    }
    Some(properties)
}

/// Decode sketch-text records carrying persistent identities, font metrics,
/// UTF-16 content, and an owning-sketch reference.
pub fn decode_sketch_texts(scan: &ContainerScan) -> Result<Vec<SketchText>, CodecError> {
    let mut out = Vec::new();
    // A record carries no version of its own: its entity id selects an entry in
    // its segment's own type table, and that entry's version fixes the member
    // sequence the record was written under.
    let types = decode_types(scan)?;
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let stream_types = stream_types_by_entity(&types, &entry.name);
        let bytes = scan.entry_bytes(&entry.name)?;
        // A stream can retain a superseded copy of a record beside the copy its
        // index names, and both parse. The record index is the entity, so the
        // first copy is kept and the rest dropped.
        let mut emitted = std::collections::HashSet::new();
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
            let Some((_, class_version)) = stream_types.get(&u64::from(record_index)).copied()
            else {
                at += 1;
                continue;
            };
            let record_end = next_indexed_record_offset(bytes, at + 7).unwrap_or(bytes.len());
            let Some(payload) = bytes.get(at..record_end) else {
                break;
            };
            if let Some(text) = decode_sketch_text_record(
                payload,
                &entry.name,
                class_tag,
                class_version,
                record_index,
                at,
            ) {
                if emitted.insert(record_index) {
                    out.push(text);
                }
                at = record_end;
            } else {
                at += 1;
            }
        }
    }
    Ok(out)
}

/// Whether a sketch-text record carries one of the two parameter-reference
/// members. A record either writes the member, whose own presence byte then
/// says whether it targets a parameter, or omits the member entirely; nothing
/// ahead of the slot distinguishes the two.
#[derive(Clone, Copy)]
enum TextReferenceSlot {
    Omitted,
    Written,
}

const TEXT_REFERENCE_SLOTS: [TextReferenceSlot; 2] =
    [TextReferenceSlot::Omitted, TextReferenceSlot::Written];

/// Bytes between a sketch-text record's last class member and its
/// owning-sketch reference, in either identity form.
const SKETCH_TEXT_TRAILING_RUN: usize = 30;

/// How far a placement-transform element may sit from the constant a planar
/// rigid placement writes there. The transform is composed in floating point,
/// so its constants arrive rounded; the bound is far tighter than a run of
/// misframed bytes would meet.
const TEXT_PLACEMENT_TOLERANCE: f64 = 1e-9;

/// Bytes from the property block to the four f32 RGBA colour components in the
/// `txt_tag` form: the `0` byte that opens the run and twelve bytes that store
/// no width factor. The components close the run at the u32 font-family count.
const TXT_TAG_HEAD_RUN: usize = 13;

/// Bytes between the text-anchor coordinates and the u32 text count in the
/// `txt_tag` form, from [`TXT_TAG_ANCHOR_MEMBER_VERSION`] onward.
const TXT_TAG_ANCHOR_RUN: usize = 11;

/// The `txt_tag` class version that adds the eleventh byte of the run between
/// the text anchor and the text count. Below it the run is ten bytes.
const TXT_TAG_ANCHOR_MEMBER_VERSION: u32 = 4;

/// The `txt_tag` class version from which a record writes its persistent
/// identity as a property-block key. A property block carrying neither identity
/// key belongs to a `txt_tag` record below this version, which stores no
/// persistent identity at all.
const TXT_TAG_IDENTITY_KEY_VERSION: u32 = 4;

/// Bytes between the `txt_tag` form's counted reference run and the thirty-byte
/// run that closes every sketch-text record.
const TXT_TAG_MEMBER_RUN: usize = 15;

/// Which identity key a sketch-text record carries. The key selects the layout
/// the record uses from the property block onward.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SketchTextIdentity {
    /// `textex_tag`: a `1` byte, the width factor, a zero byte between the font
    /// family and the height, and the anchor inside a placement transform.
    TextexTag,
    /// `txt_tag`: a `0` byte, no width factor, the height directly after the
    /// font family, and the anchor stored on its own.
    TxtTag,
}

/// Read one parameter-reference slot in the given form, advancing `cursor` by
/// what that form occupies. An omitted member reads as a null reference.
fn read_text_reference(
    payload: &[u8],
    cursor: &mut usize,
    slot: TextReferenceSlot,
) -> Option<Reference> {
    match slot {
        TextReferenceSlot::Omitted => Some(Reference::default()),
        TextReferenceSlot::Written => take_reference(payload, cursor),
    }
}

/// Read the four f32 RGBA colour components both sketch-text classes store,
/// advancing `cursor` past them. Every component is a fraction of full
/// intensity, so a run leaving `[0, 1]` says the record is misframed rather
/// than that the text carries an out-of-range colour.
fn read_sketch_text_color(payload: &[u8], cursor: &mut usize) -> Option<Color> {
    let mut components = [0f32; 4];
    for component in &mut components {
        let value = take_f32(payload, cursor)?;
        (0.0..=1.0).contains(&value).then_some(())?;
        *component = value;
    }
    let [r, g, b, a] = components;
    Some(Color { r, g, b, a })
}

/// Read the row-major 4×4 f64 placement transform a frame-text record stores,
/// advancing `cursor` past it, and reduce it to the anchor point in millimetres
/// and the rotation about that point in radians.
///
/// The transform is a planar rigid placement: its third row and column are the
/// identity's, its bottom row is `(0, 0, 0, 1)`, its translation lives in the
/// last column, and its 2×2 basis is a rotation of unit determinant carrying no
/// scale or shear. A run failing any of that is not a placement, so the record
/// is misframed.
fn read_text_placement(payload: &[u8], cursor: &mut usize) -> Option<(Point2, f64)> {
    let elements = f64s_at(payload, *cursor, 16)?;
    *cursor = cursor.checked_add(128)?;
    let at = |row: usize, column: usize| elements[row * 4 + column];
    let constant = |value: f64, expected: f64| (value - expected).abs() <= TEXT_PLACEMENT_TOLERANCE;
    let planar = [
        (0, 2, 0.0),
        (1, 2, 0.0),
        (2, 0, 0.0),
        (2, 1, 0.0),
        (2, 2, 1.0),
        (2, 3, 0.0),
        (3, 0, 0.0),
        (3, 1, 0.0),
        (3, 2, 0.0),
        (3, 3, 1.0),
    ]
    .into_iter()
    .all(|(row, column, expected)| constant(at(row, column), expected));
    let determinant = at(0, 0) * at(1, 1) - at(0, 1) * at(1, 0);
    (planar && constant(determinant, 1.0)).then_some(())?;
    let anchor = Point2::new(at(0, 3) * 10.0, at(1, 3) * 10.0);
    (anchor.u.is_finite() && anchor.v.is_finite()).then_some(())?;
    Some((anchor, at(1, 0).atan2(at(0, 0))))
}

/// The record index a reference names, absent when the reference is null.
fn reference_index(reference: &Reference) -> Option<u32> {
    reference
        .target
        .and_then(|target| u32::try_from(target).ok())
}

/// Class-level fields of a sketch-text record, ending at the text height.
struct SketchTextHead {
    identity: SketchTextIdentity,
    entity_genesis: Option<u64>,
    persistent_id: Option<u64>,
    base_id: Option<u64>,
    font_family: String,
    height: f64,
    width_factor: Option<f64>,
    color: Color,
    cursor: usize,
}

/// Fields following the height, whose framing depends on the two slot forms.
struct SketchTextTail {
    first_reference: Option<u32>,
    second_reference: Option<u32>,
    text: String,
    font_weight: i32,
    anchor: Option<Point2>,
    rotation: Option<f64>,
    owner_reference: u32,
}

/// Read the class-defined leading block: the presence byte, and when it is
/// `01`, a u32 count and that many `(reference, u32)` pairs. The `txt_tag` form
/// writes such a block; the `textex_tag` form writes the `00` byte alone.
fn read_sketch_text_leading_block(payload: &[u8], cursor: &mut usize) -> Option<()> {
    match payload.get(*cursor)? {
        0 => *cursor += 1,
        1 => {
            *cursor += 1;
            let count = usize::try_from(u32_at(payload, *cursor)?).ok()?;
            if count > MAX_RELATION_RUN {
                return None;
            }
            *cursor = cursor.checked_add(4)?;
            for _ in 0..count {
                take_reference(payload, cursor)?;
                u32_at(payload, *cursor)?;
                *cursor = cursor.checked_add(4)?;
            }
        }
        _ => return None,
    }
    Some(())
}

/// Read the record prefix, property block, and the metrics up to the height.
fn decode_sketch_text_head(payload: &[u8], class_version: u32) -> Option<SketchTextHead> {
    // Record prefix: the LP-ASCII class tag, the u64 entity ID, and the
    // LP-ASCII record name.
    let (_, after_tag) = lp_ascii_filtered(payload, 0, 3..=3, u8::is_ascii_digit)?;
    let (_, mut cursor) = lp_ascii_filtered(
        payload,
        after_tag.checked_add(8)?,
        0..=256,
        u8::is_ascii_graphic,
    )?;
    read_sketch_text_leading_block(payload, &mut cursor)?;
    // The block carries the text identities under keys that vary by record, so
    // each is addressed by name. The identity key is `textex_tag` or `txt_tag`,
    // and which of the two the record carries selects the layout that follows.
    // A `txt_tag` record below TXT_TAG_IDENTITY_KEY_VERSION writes neither key
    // and carries no persistent identity, so its class version selects the
    // layout in place of a key.
    let properties = read_property_block(payload, &mut cursor)?;
    let property = |key: &str| {
        properties
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| *value)
    };
    let (identity, persistent_id) = match (property("textex_tag"), property("txt_tag")) {
        (Some(value), _) => (SketchTextIdentity::TextexTag, Some(value)),
        (None, Some(value)) => (SketchTextIdentity::TxtTag, Some(value)),
        (None, None) if class_version < TXT_TAG_IDENTITY_KEY_VERSION => {
            (SketchTextIdentity::TxtTag, None)
        }
        (None, None) => return None,
    };
    let (width_factor, color) = match identity {
        SketchTextIdentity::TextexTag => {
            (payload.get(cursor)? == &1).then_some(())?;
            cursor += 1;
            let width_factor = f64_at(payload, cursor)?;
            cursor = cursor.checked_add(8)?;
            let color = read_sketch_text_color(payload, &mut cursor)?;
            (width_factor.is_finite() && width_factor >= 0.0).then_some(())?;
            (Some(width_factor), color)
        }
        SketchTextIdentity::TxtTag => {
            (payload.get(cursor)? == &0).then_some(())?;
            cursor = cursor.checked_add(TXT_TAG_HEAD_RUN)?;
            let color = read_sketch_text_color(payload, &mut cursor)?;
            (None, color)
        }
    };
    let font_count = usize::try_from(u32_at(payload, cursor)?).ok()?;
    if font_count == 0 || font_count > 1_024 {
        return None;
    }
    let (font_family, after_font) = utf16le_at(payload, cursor + 4, font_count)?;
    cursor = after_font;
    if identity == SketchTextIdentity::TextexTag {
        (payload.get(cursor)? == &0).then_some(())?;
        cursor += 1;
    }
    let height = f64_at(payload, cursor)? * 10.0;
    cursor = cursor.checked_add(8)?;
    (height.is_finite() && height > 0.0).then_some(())?;
    Some(SketchTextHead {
        identity,
        entity_genesis: property("EntityGenesis"),
        persistent_id,
        base_id: property("txt_tag_base"),
        font_family,
        height,
        width_factor,
        color,
        cursor,
    })
}

/// Read the alignment fields, text content, and class tail under one pair of
/// slot forms, requiring the walk to end exactly on the owning-sketch
/// reference.
fn decode_sketch_text_tail(
    payload: &[u8],
    mut cursor: usize,
    first_slot: TextReferenceSlot,
    second_slot: TextReferenceSlot,
) -> Option<SketchTextTail> {
    let first_reference = read_text_reference(payload, &mut cursor, first_slot)?;
    // Horizontal alignment enum and three flag bytes.
    u32_at(payload, cursor)?;
    cursor = cursor.checked_add(7)?;
    let text_count = usize::try_from(u32_at(payload, cursor)?).ok()?;
    if text_count == 0 || text_count > 1_048_576 {
        return None;
    }
    let (text, after_text) = utf16le_at(payload, cursor + 4, text_count)?;
    cursor = after_text;
    let second_reference = read_text_reference(payload, &mut cursor, second_slot)?;
    // Vertical alignment enum, one flag byte, and the font weight.
    u32_at(payload, cursor)?;
    let font_weight = u32_at(payload, cursor.checked_add(5)?)? as i32;
    matches!(font_weight, 400 | 500 | 750).then_some(())?;
    cursor = cursor.checked_add(9)?;
    // The class tail opens with the text-type enum, which gates the placement
    // transform: frame text stores a 4x4 transform, path text stores none. One
    // flag byte follows the enum and repeats it, so a slot form that has
    // desynchronized fails here instead of framing a transform out of whatever
    // bytes the walk landed on.
    let text_type = u32_at(payload, cursor)?;
    (u32::from(*payload.get(cursor.checked_add(4)?)?) == text_type).then_some(())?;
    cursor = cursor.checked_add(5)?;
    let placement = match text_type {
        0 => Some(read_text_placement(payload, &mut cursor)?),
        1 => None,
        _ => return None,
    };
    cursor = cursor.checked_add(SKETCH_TEXT_TRAILING_RUN)?;
    let owner = take_reference(payload, &mut cursor)?;
    (cursor == payload.len()).then_some(())?;
    Some(SketchTextTail {
        first_reference: reference_index(&first_reference),
        second_reference: reference_index(&second_reference),
        text,
        font_weight,
        anchor: placement.map(|(anchor, _)| anchor),
        rotation: placement.map(|(_, rotation)| rotation),
        owner_reference: reference_index(&owner)?,
    })
}

/// Read the `txt_tag` form's members from the height to the end of the record.
/// Two bytes separate the height from the anchor coordinates, which this form
/// stores directly rather than in a placement transform. The form writes no
/// parameter-reference slot: an eleven-byte run carries the alignment fields,
/// ten bytes below [`TXT_TAG_ANCHOR_MEMBER_VERSION`], and the text string is
/// followed by a counted reference run, fifteen bytes, and the trailing run and
/// owning-sketch reference that close both forms.
fn decode_txt_tag_sketch_text_tail(
    payload: &[u8],
    mut cursor: usize,
    class_version: u32,
) -> Option<SketchTextTail> {
    cursor = cursor.checked_add(2)?;
    let anchor = Point2::new(
        f64_at(payload, cursor)? * 10.0,
        f64_at(payload, cursor.checked_add(8)?)? * 10.0,
    );
    (anchor.u.is_finite() && anchor.v.is_finite()).then_some(())?;
    let anchor_run = if class_version < TXT_TAG_ANCHOR_MEMBER_VERSION {
        TXT_TAG_ANCHOR_RUN - 1
    } else {
        TXT_TAG_ANCHOR_RUN
    };
    cursor = cursor.checked_add(16 + anchor_run)?;
    let text_count = usize::try_from(u32_at(payload, cursor)?).ok()?;
    if text_count == 0 || text_count > 1_048_576 {
        return None;
    }
    let (text, after_text) = utf16le_at(payload, cursor + 4, text_count)?;
    cursor = after_text;
    let references = usize::try_from(u32_at(payload, cursor)?).ok()?;
    if references > MAX_RELATION_RUN {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    for _ in 0..references {
        take_reference(payload, &mut cursor)?;
    }
    let font_weight = u32_at(payload, cursor.checked_add(3)?)? as i32;
    matches!(font_weight, 400 | 500 | 750).then_some(())?;
    cursor = cursor.checked_add(TXT_TAG_MEMBER_RUN + SKETCH_TEXT_TRAILING_RUN)?;
    let owner = take_reference(payload, &mut cursor)?;
    (cursor == payload.len()).then_some(())?;
    Some(SketchTextTail {
        first_reference: None,
        second_reference: None,
        text,
        font_weight,
        anchor: Some(anchor),
        // This form writes no placement transform, so it stores no rotation.
        rotation: None,
        owner_reference: reference_index(&owner)?,
    })
}

pub(crate) fn decode_sketch_text_record(
    payload: &[u8],
    stream: &str,
    class_tag: String,
    class_version: u32,
    record_index: u32,
    byte_offset: usize,
) -> Option<SketchText> {
    let head = decode_sketch_text_head(payload, class_version)?;
    let tail = match head.identity {
        SketchTextIdentity::TextexTag => {
            let mut closed = None;
            for first_slot in TEXT_REFERENCE_SLOTS {
                for second_slot in TEXT_REFERENCE_SLOTS {
                    let Some(tail) =
                        decode_sketch_text_tail(payload, head.cursor, first_slot, second_slot)
                    else {
                        continue;
                    };
                    // Two slot forms both ending on the owning-sketch reference
                    // leave the parameter references undetermined.
                    if closed.replace(tail).is_some() {
                        return None;
                    }
                }
            }
            closed?
        }
        SketchTextIdentity::TxtTag => {
            decode_txt_tag_sketch_text_tail(payload, head.cursor, class_version)?
        }
    };
    Some(SketchText {
        id: ids::native_sketch_text_id(stream, byte_offset),
        record_index,
        owner_reference: tail.owner_reference,
        class_tag,
        class_version,
        byte_offset: byte_offset as u64,
        entity_genesis: head.entity_genesis,
        persistent_id: head.persistent_id,
        base_id: head.base_id,
        text: tail.text,
        font_family: head.font_family,
        font_weight: tail.font_weight,
        height: head.height,
        width_factor: head.width_factor,
        color: head.color,
        anchor: tail.anchor,
        rotation: tail.rotation,
        first_reference: tail.first_reference,
        second_reference: tail.second_reference,
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
        .filter(|entity| entity.in_sketch_module())
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
    for entity in entities.iter().filter(|entity| entity.in_sketch_module()) {
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
    pub(crate) member_relation_ordinals: Vec<u32>,
    pub(crate) auxiliary_references: Vec<u32>,
    pub(crate) auxiliary_reference_offsets: Vec<usize>,
    pub(crate) owner_reference: u32,
    pub(crate) owner_reference_offset: usize,
    pub(crate) state: u64,
    pub(crate) state_offset: usize,
    pub(crate) entity_genesis: Option<u64>,
    pub(crate) text_glyph_transforms: Option<Vec<[[f64; 4]; 4]>>,
    /// Position within `auxiliary_references` of the first direction clause's
    /// count-parameter reference on a rectangular pattern whose four clause
    /// references are all present. `None` where the class was not named or a
    /// clause reference is absent, which leaves the ordinals to be guessed from
    /// the length of the auxiliary run.
    pub(crate) rectangular_clause_ordinal: Option<usize>,
    pub(crate) return_members: Vec<u32>,
    pub(crate) return_member_offsets: Vec<usize>,
    pub(crate) parsed_end: usize,
}

/// Type GUID of the shared sketch-relation class, which adds no member of its
/// own between the property block and `ParentNode`.
const RELATION_TYPE_GUID: &str = "60403D47-0C49-49B0-BDE8-1679608164A2";
/// Type GUID of the offset class, whose relations carry mask `0x2000000000`.
const OFFSET_RELATION_TYPE_GUID: &str = "D3BD153B-EB8A-405E-9D29-69EE0C3D227C";
/// Type GUID of the spline-group class, whose relations carry mask `0x80000000`.
const SPLINE_RELATION_TYPE_GUID: &str = "73762C3B-82DC-4632-93B0-B8FE1CC5282F";
/// Type GUID of the tangency class, whose relations carry mask `0x100`.
const TANGENT_RELATION_TYPE_GUID: &str = "24DB790E-3DCD-4336-AFA3-6F119EF2239B";
/// Type GUID of the circular-pattern class, mask `0x10000000`.
const CIRCULAR_PATTERN_RELATION_TYPE_GUID: &str = "8269E861-0BB7-47E0-9911-5AE3EC475058";
/// Type GUID of the rectangular-pattern class, mask `0x20000000`.
const RECTANGULAR_PATTERN_RELATION_TYPE_GUID: &str = "40800FB9-C2BE-494E-A047-7D76E82B9F6C";
/// Type GUID of the text-frame class, mask `0x10000000000`.
const TEXT_FRAME_RELATION_TYPE_GUID: &str = "8B369926-123F-4F9D-878E-6D4C076128D3";
/// Type GUID of the text-path class, mask `0x20000000000`.
const TEXT_PATH_RELATION_TYPE_GUID: &str = "9D30FCDC-EA07-4141-93E2-918B1A59E962";

/// A sketch-relation record class, named by the type GUID its segment's type
/// table carries for the record's entity. The class fixes the members written
/// between the property block and the base class's `ParentNode`. A class tag
/// cannot name it: a tag is `256` plus an index into the segment's own type
/// table, so one tag names different relation classes in different segments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SketchRelationClass {
    /// A class whose most-derived level adds no member: the shared relation
    /// class, the offset class, and the spline-group class.
    Plain,
    /// Three `u8` flags, each `0` or `1`.
    Tangent,
    /// The angle-parameter reference, the count-parameter reference, the
    /// evaluated f64 total angle in radians, the evaluated u32 instance count,
    /// the pattern tables, and one `u8`.
    CircularPattern,
    /// Three `u8` flags, a u32-counted run of references, the pattern tables,
    /// and two direction clauses.
    RectangularPattern,
    /// Two references.
    TextFrame,
    /// The text-entity reference, a u32 character count, and that many glyph
    /// blocks. `leading_flag` is the `u8` the class writes before the
    /// reference, which it does from class version 1.
    TextPath {
        /// Whether the class version writes the leading `u8`.
        leading_flag: bool,
    },
}

impl SketchRelationClass {
    /// The relation class `type_guid` names at class version `version`, or
    /// `None` where the GUID is not a sketch-relation class.
    pub(crate) fn of(type_guid: &str, version: u32) -> Option<Self> {
        let matches = |known: &str| type_guid.eq_ignore_ascii_case(known);
        if matches(RELATION_TYPE_GUID)
            || matches(OFFSET_RELATION_TYPE_GUID)
            || matches(SPLINE_RELATION_TYPE_GUID)
        {
            return Some(Self::Plain);
        }
        if matches(TANGENT_RELATION_TYPE_GUID) {
            return Some(Self::Tangent);
        }
        if matches(CIRCULAR_PATTERN_RELATION_TYPE_GUID) {
            return Some(Self::CircularPattern);
        }
        if matches(RECTANGULAR_PATTERN_RELATION_TYPE_GUID) {
            return Some(Self::RectangularPattern);
        }
        if matches(TEXT_FRAME_RELATION_TYPE_GUID) {
            return Some(Self::TextFrame);
        }
        if matches(TEXT_PATH_RELATION_TYPE_GUID) {
            return Some(Self::TextPath {
                leading_flag: version >= 1,
            });
        }
        None
    }
}

/// Largest plausible counted run inside a sketch relation; a larger count is a
/// misparse rather than a record that owns that many members.
const MAX_RELATION_RUN: usize = 4096;

/// Whether a sketch-relation record carries the paired member run. That leading
/// byte also selects the constraint-mask width: a u64 with the run, a u32 at
/// class version 0, which has neither.
pub(crate) fn relation_has_paired_member_run(record: &[u8]) -> Option<bool> {
    let (_, start) = lp_ascii_filtered(record, 15, 0..=256, u8::is_ascii_graphic)?;
    match record.get(start)? {
        1 => Some(true),
        0 => Some(false),
        _ => None,
    }
}

/// Take one reference member at `cursor`, returning its 32-bit target and the
/// byte offset of the target within the reference. Relation members address
/// records in the relation's own segment, so a reference whose target does not
/// fit a `u32` is a misparse.
fn take_relation_reference(payload: &[u8], cursor: &mut usize) -> Option<(u32, usize)> {
    let at = *cursor;
    let reference = take_reference(payload, cursor)?;
    Some((u32::try_from(reference.target?).ok()?, at + 1))
}

/// Take one reference member that the class may leave absent, recording it in
/// the auxiliary run when it is present. An absent reference is one zero byte
/// and names nothing, so it contributes no entry.
fn take_auxiliary_relation_reference(
    payload: &[u8],
    cursor: &mut usize,
    auxiliary_references: &mut Vec<u32>,
    auxiliary_reference_offsets: &mut Vec<usize>,
) -> Option<bool> {
    let at = *cursor;
    let reference = take_reference(payload, cursor)?;
    let Some(target) = reference.target else {
        return Some(false);
    };
    auxiliary_references.push(u32::try_from(target).ok()?);
    auxiliary_reference_offsets.push(at + 1);
    Some(true)
}

/// Skip the two tables both pattern classes write after their own leading
/// members: a u32-counted map whose entry is a u64 key, a u32 count, and that
/// many u64 values; then a u32-counted run of u32.
fn skip_pattern_tables(payload: &[u8], cursor: &mut usize) -> Option<()> {
    let entries = usize::try_from(u32_at(payload, *cursor)?).ok()?;
    if entries > MAX_RELATION_RUN {
        return None;
    }
    *cursor += 4;
    for _ in 0..entries {
        let values = usize::try_from(u32_at(payload, *cursor + 8)?).ok()?;
        if values > MAX_RELATION_RUN {
            return None;
        }
        *cursor += 12 + values * 8;
    }
    let ordinals = usize::try_from(u32_at(payload, *cursor)?).ok()?;
    if ordinals > MAX_RELATION_RUN {
        return None;
    }
    *cursor += 4 + ordinals * 4;
    (*cursor <= payload.len()).then_some(())
}

/// What a sketch-relation subclass leaves behind after its own members.
struct RelationClassMembers {
    rectangular_clause_ordinal: Option<usize>,
    text_glyph_transforms: Option<Vec<[[f64; 4]; 4]>>,
}

/// Consume the members `class` writes between the property block and the base
/// class's `ParentNode`, advancing `cursor` past them and recording every
/// present reference among them in the auxiliary run
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
fn parse_relation_class_members(
    payload: &[u8],
    cursor: &mut usize,
    class: SketchRelationClass,
    auxiliary_references: &mut Vec<u32>,
    auxiliary_reference_offsets: &mut Vec<usize>,
) -> Option<RelationClassMembers> {
    let mut members = RelationClassMembers {
        rectangular_clause_ordinal: None,
        text_glyph_transforms: None,
    };
    macro_rules! take {
        () => {
            take_auxiliary_relation_reference(
                payload,
                cursor,
                auxiliary_references,
                auxiliary_reference_offsets,
            )
        };
    }
    match class {
        SketchRelationClass::Plain => {}
        SketchRelationClass::Tangent => {
            for _ in 0..3 {
                // A flag outside `{0, 1}` is a misparse, not a third state.
                if !matches!(payload.get(*cursor)?, 0 | 1) {
                    return None;
                }
                *cursor += 1;
            }
        }
        SketchRelationClass::TextFrame => {
            take!()?;
            take!()?;
        }
        SketchRelationClass::CircularPattern => {
            take!()?;
            take!()?;
            // The evaluated total angle and the evaluated instance count.
            *cursor += 12;
            skip_pattern_tables(payload, cursor)?;
            if payload.get(*cursor)? != &0 {
                return None;
            }
            *cursor += 1;
        }
        SketchRelationClass::RectangularPattern => {
            // The three flags are not checked the way the tangency class's are:
            // the counted runs and the unit directions that follow them already
            // reject a misframed record, and the flags do not.
            *cursor += 3;
            let seeds = usize::try_from(u32_at(payload, *cursor)?).ok()?;
            if seeds > MAX_RELATION_RUN {
                return None;
            }
            *cursor += 4;
            for _ in 0..seeds {
                take!()?;
            }
            skip_pattern_tables(payload, cursor)?;
            let clause_ordinal = auxiliary_reference_offsets.len();
            let mut complete = true;
            for _ in 0..2 {
                // The evaluated instance count precedes the count-parameter
                // reference; the unit direction and the seed-to-final-instance
                // span follow it, and the distance parameter closes the clause.
                *cursor += 4;
                complete &= take!()?;
                *cursor += 32;
                complete &= take!()?;
            }
            members.rectangular_clause_ordinal = complete.then_some(clause_ordinal);
        }
        SketchRelationClass::TextPath { leading_flag } => {
            if leading_flag {
                if payload.get(*cursor)? != &1 {
                    return None;
                }
                *cursor += 1;
            }
            let (text_reference, transforms, end) = parse_text_glyph_run(payload, *cursor)?;
            auxiliary_references.push(text_reference);
            auxiliary_reference_offsets.push(*cursor + 1);
            members.text_glyph_transforms = Some(transforms);
            *cursor = end;
        }
    }
    Some(members)
}

/// Parse one sketch-relation record body ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
///
/// The payload is `u8 1`, a u32 count and that many `(reference, u32 relation
/// ordinal)` pairs, the property-block presence byte and its block, the
/// `ParentNode` reference naming the owning sketch, a u64 constraint mask, a
/// u32 count and that many bare references, and one zero byte. At class version
/// 0 the leading byte is zero, the pair list is absent, and the mask is a u32.
/// Both reference runs hold the same members; only the second is in semantic
/// order. Pattern and text classes carry extra class members between the
/// property block and `ParentNode`, which are retained as the auxiliary run.
///
/// The class of the record is not known here, so the class members are walked
/// reference to reference and `ParentNode` is taken to be the first reference
/// naming an owning sketch. That walk desynchronizes on a class member whose
/// bytes fit a reference, so it is the fallback for a record whose segment's
/// type table does not register its entity; a record whose class is named is
/// parsed by [`parse_classed_sketch_relation`].
pub(crate) fn parse_sketch_relation(
    payload: &[u8],
    owners: &std::collections::HashSet<u32>,
) -> Option<ParsedSketchRelation> {
    parse_relation(payload, RelationFraming::Walked(owners))
}

/// Parse one sketch-relation record body whose class is known
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
///
/// The grammar is the one [`parse_sketch_relation`] documents, with `class`
/// fixing the members between the property block and `ParentNode` exactly, so
/// no scan is involved, `ParentNode` is read where the class puts it, and the
/// owning sketches need not be known to find it.
pub(crate) fn parse_classed_sketch_relation(
    payload: &[u8],
    class: SketchRelationClass,
) -> Option<ParsedSketchRelation> {
    parse_relation(payload, RelationFraming::Classed(class))
}

/// How the members between the property block and `ParentNode` are framed.
#[derive(Clone, Copy)]
enum RelationFraming<'a> {
    /// The record's class is not known, so the run is walked reference to
    /// reference and `ParentNode` is the first reference naming one of these
    /// owning sketches.
    Walked(&'a std::collections::HashSet<u32>),
    /// The record's class is known and fixes the run exactly.
    Classed(SketchRelationClass),
}

fn parse_relation(payload: &[u8], framing: RelationFraming) -> Option<ParsedSketchRelation> {
    // The record header is the LP-ASCII class tag, the u64 entity id, and the
    // LP-ASCII record name; the member payload follows it.
    let (_, start) = lp_ascii_filtered(payload, 15, 0..=256, u8::is_ascii_graphic)?;
    let paired_run = match payload.get(start)? {
        1 => true,
        0 => false,
        _ => return None,
    };
    let mut cursor = start + 1;
    let mut members = Vec::new();
    let mut member_offsets = Vec::new();
    let mut member_relation_ordinals = Vec::new();
    if paired_run {
        let member_count = usize::try_from(u32_at(payload, cursor)?).ok()?;
        if member_count > MAX_RELATION_RUN {
            return None;
        }
        cursor += 4;
        members.reserve(member_count);
        member_offsets.reserve(member_count);
        member_relation_ordinals.reserve(member_count);
        for _ in 0..member_count {
            let (member, offset) = take_relation_reference(payload, &mut cursor)?;
            members.push(member);
            member_offsets.push(offset);
            member_relation_ordinals.push(u32_at(payload, cursor)?);
            cursor += 4;
        }
    }
    // The base class level opens with its property-block presence byte. The
    // block is `u32 count` and that many `(key, type name, value)` triples;
    // `EntityGenesis` is one such key.
    let entity_genesis = read_property_block(payload, &mut cursor)?
        .into_iter()
        .find(|(key, _)| key == "EntityGenesis")
        .map(|(_, value)| value);
    let mut auxiliary_references = Vec::new();
    let mut auxiliary_reference_offsets = Vec::new();
    let mut text_glyph_transforms = None;
    let mut rectangular_clause_ordinal = None;
    let (owner_reference, owner_reference_offset, state_offset) = match framing {
        RelationFraming::Classed(class) => {
            // The class fixes its own members, so `ParentNode` is read where the
            // class puts it rather than searched for.
            let class_members = parse_relation_class_members(
                payload,
                &mut cursor,
                class,
                &mut auxiliary_references,
                &mut auxiliary_reference_offsets,
            )?;
            rectangular_clause_ordinal = class_members.rectangular_clause_ordinal;
            text_glyph_transforms = class_members.text_glyph_transforms;
            let (reference, offset) = take_relation_reference(payload, &mut cursor)?;
            (reference, offset, cursor)
        }
        RelationFraming::Walked(owners) => {
            // A text-path relation follows the `EntityGenesis` block with a `0x01`
            // flag, the marked text-entity reference and its zero padding, a u32
            // character count, and per-character blocks of `u32 16` and sixteen f64
            // values. Parse the run structurally so the f64 payload's bytes are not
            // misread as auxiliary references; the owning sketch reference follows
            // the last block directly.
            if payload.get(cursor) == Some(&1) && payload.get(cursor + 1) == Some(&1) {
                if let Some((text_reference, transforms, after)) =
                    parse_text_glyph_run(payload, cursor + 1)
                {
                    if marked_u32(payload, after)
                        .is_some_and(|(reference, _)| owners.contains(&reference))
                    {
                        auxiliary_references.push(text_reference);
                        auxiliary_reference_offsets.push(cursor + 2);
                        text_glyph_transforms = Some(transforms);
                        cursor = after;
                    }
                }
            }
            // Pattern and text classes place their own members between the property
            // block and `ParentNode`. Without the class their widths are unknown, so
            // the run is walked reference to reference and `ParentNode` is taken to
            // be the first reference that names an owning sketch.
            loop {
                cursor = next_reference_marker(payload, cursor)?;
                let mut probe = cursor;
                let (reference, offset) = take_relation_reference(payload, &mut probe)?;
                if owners.contains(&reference) {
                    break (reference, offset, probe);
                }
                auxiliary_references.push(reference);
                auxiliary_reference_offsets.push(offset);
                cursor = probe;
            }
        }
    };
    // The constraint mask follows `ParentNode` directly. It is a u64 in the
    // paired-run form and a u32 at class version 0.
    let (state, mut cursor) = if paired_run {
        (
            u64::from_le_bytes(
                payload
                    .get(state_offset..state_offset + 8)?
                    .try_into()
                    .ok()?,
            ),
            state_offset + 8,
        )
    } else {
        (u64::from(u32_at(payload, state_offset)?), state_offset + 4)
    };
    let return_count = usize::try_from(u32_at(payload, cursor)?).ok()?;
    if return_count > MAX_RELATION_RUN {
        return None;
    }
    cursor += 4;
    let mut return_members = Vec::with_capacity(return_count);
    let mut return_member_offsets = Vec::with_capacity(return_count);
    for _ in 0..return_count {
        let (member, offset) = take_relation_reference(payload, &mut cursor)?;
        return_members.push(member);
        return_member_offsets.push(offset);
    }
    if payload.get(cursor) != Some(&0) {
        return None;
    }
    let parsed_end = cursor + 1;
    Some(ParsedSketchRelation {
        members,
        member_offsets,
        member_relation_ordinals,
        auxiliary_references,
        auxiliary_reference_offsets,
        owner_reference,
        owner_reference_offset,
        state,
        state_offset,
        entity_genesis,
        text_glyph_transforms,
        rectangular_clause_ordinal,
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

/// Whether an indexed-record header starts at `at`: a u32 length prefix of
/// three, a three-digit ASCII class tag, and a u32 record index. The eleven
/// bytes must be present.
fn indexed_record_header_at(bytes: &[u8], at: usize) -> bool {
    u32_at(bytes, at) == Some(3)
        && bytes
            .get(at + 4..at + 7)
            .is_some_and(|tag| tag.iter().all(u8::is_ascii_digit))
        && bytes.get(at + 7..at + 11).is_some()
}

/// The record index carried by the indexed-record header at `at`. The header
/// spends its first seven bytes on the length-prefixed class tag, so the index
/// always sits at `at + 7`.
pub(crate) fn indexed_record_index(bytes: &[u8], at: usize) -> Option<u32> {
    u32_at(bytes, at.checked_add(7)?)
}

pub(crate) fn next_indexed_record_offset(bytes: &[u8], position: usize) -> Option<usize> {
    indexed_record_offsets(bytes.get(position..)?)
        .next()
        .map(|at| position + at)
}

/// Header offsets of every indexed record whose class tag is three characters.
///
/// A class tag is `256` plus an index into the segment's own type table, so a
/// tag reaches four characters only in a segment registering more than 744
/// types. No segment registers that many, and `indexed_record_index` reads the
/// record index at a fixed `at + 7` on the same assumption; both would have to
/// change together to widen it.
pub(crate) fn indexed_record_offsets(bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
    memchr::memmem::find_iter(bytes, &[3, 0, 0, 0])
        .filter(|at| indexed_record_header_at(bytes, *at))
}

pub(crate) fn next_indexed_record_offset_with_index(
    bytes: &[u8],
    mut position: usize,
    record_index: u32,
) -> Option<usize> {
    loop {
        let offset = next_indexed_record_offset(bytes, position)?;
        if indexed_record_index(bytes, offset) == Some(record_index) {
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

#[cfg(test)]
mod relation_class_tests {
    use super::{
        decode_pattern_definition, parse_classed_sketch_relation, parse_sketch_relation,
        SketchRelationClass,
    };
    use crate::records::{SketchPatternDefinition, SketchPatternDirection};
    use std::collections::HashSet;

    /// One present reference: the presence byte, the u64 target, and the
    /// `cross_document` and same-segment flags.
    fn push_reference(out: &mut Vec<u8>, target: u32) {
        out.push(1);
        out.extend_from_slice(&u64::from(target).to_le_bytes());
        out.extend_from_slice(&[0u8; 2]);
    }

    /// One absent reference.
    fn push_absent_reference(out: &mut Vec<u8>) {
        out.push(0);
    }

    /// A relation record: the header, the paired member run, an empty property
    /// block, `class_members`, `ParentNode`, the u64 mask, the return run, and
    /// the trailing zero byte. The record ends where the parse must end.
    fn relation_record(
        members: &[(u32, u32)],
        class_members: &[u8],
        owner: u32,
        mask: u64,
        returns: &[u32],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(b"298");
        out.extend_from_slice(&7u32.to_le_bytes());
        out.extend_from_slice(&[0u8; 8]);
        out.push(1);
        out.extend_from_slice(
            &u32::try_from(members.len())
                .expect("member count fits a u32")
                .to_le_bytes(),
        );
        for (reference, ordinal) in members {
            push_reference(&mut out, *reference);
            out.extend_from_slice(&ordinal.to_le_bytes());
        }
        out.push(0);
        out.extend_from_slice(class_members);
        push_reference(&mut out, owner);
        out.extend_from_slice(&mask.to_le_bytes());
        out.extend_from_slice(
            &u32::try_from(returns.len())
                .expect("return count fits a u32")
                .to_le_bytes(),
        );
        for reference in returns {
            push_reference(&mut out, *reference);
        }
        out.push(0);
        out
    }

    /// The two pattern tables, both empty.
    fn empty_pattern_tables() -> [u8; 8] {
        [0u8; 8]
    }

    /// One rectangular direction clause.
    fn push_direction_clause(
        out: &mut Vec<u8>,
        count: u32,
        count_parameter: u32,
        direction: [f64; 3],
        distance: f64,
        distance_parameter: u32,
    ) {
        out.extend_from_slice(&count.to_le_bytes());
        push_reference(out, count_parameter);
        for axis in direction {
            out.extend_from_slice(&axis.to_le_bytes());
        }
        out.extend_from_slice(&distance.to_le_bytes());
        push_reference(out, distance_parameter);
    }

    /// A glyph run: the text reference, the character count, and one block of
    /// `u32 16` and a row-major 4x4 transform.
    fn push_glyph_run(out: &mut Vec<u8>, text: u32, translation: f64) {
        push_reference(out, text);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&16u32.to_le_bytes());
        for row in 0..4 {
            for column in 0..4 {
                let value = if row == 0 && column == 3 {
                    translation
                } else {
                    f64::from(u8::from(row == column))
                };
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
    }

    #[test]
    fn relation_classes_are_named_by_type_guid() {
        assert_eq!(
            SketchRelationClass::of("60403D47-0C49-49B0-BDE8-1679608164A2", 3),
            Some(SketchRelationClass::Plain)
        );
        assert_eq!(
            SketchRelationClass::of("d3bd153b-eb8a-405e-9d29-69ee0c3d227c", 0),
            Some(SketchRelationClass::Plain)
        );
        assert_eq!(
            SketchRelationClass::of("73762C3B-82DC-4632-93B0-B8FE1CC5282F", 0),
            Some(SketchRelationClass::Plain)
        );
        assert_eq!(
            SketchRelationClass::of("24DB790E-3DCD-4336-AFA3-6F119EF2239B", 0),
            Some(SketchRelationClass::Tangent)
        );
        assert_eq!(
            SketchRelationClass::of("8269E861-0BB7-47E0-9911-5AE3EC475058", 3),
            Some(SketchRelationClass::CircularPattern)
        );
        assert_eq!(
            SketchRelationClass::of("40800FB9-C2BE-494E-A047-7D76E82B9F6C", 5),
            Some(SketchRelationClass::RectangularPattern)
        );
        assert_eq!(
            SketchRelationClass::of("8B369926-123F-4F9D-878E-6D4C076128D3", 0),
            Some(SketchRelationClass::TextFrame)
        );
        assert_eq!(
            SketchRelationClass::of("9D30FCDC-EA07-4141-93E2-918B1A59E962", 0),
            Some(SketchRelationClass::TextPath {
                leading_flag: false
            })
        );
        assert_eq!(
            SketchRelationClass::of("9D30FCDC-EA07-4141-93E2-918B1A59E962", 1),
            Some(SketchRelationClass::TextPath { leading_flag: true })
        );
        assert_eq!(
            SketchRelationClass::of("69EE2FA7-BCC7-449E-9CA9-976CEFDFED44", 0),
            None
        );
    }

    #[test]
    fn plain_relation_reads_parent_node_without_class_members() {
        let record = relation_record(&[(300, 1), (301, 0)], &[], 201, 0x1, &[300, 301]);
        let parsed = parse_classed_sketch_relation(&record, SketchRelationClass::Plain)
            .expect("the classed parse reads the record");
        assert_eq!(parsed.owner_reference, 201);
        assert_eq!(parsed.state, 0x1);
        assert_eq!(parsed.members, [300, 301]);
        assert_eq!(parsed.return_members, [300, 301]);
        assert!(parsed.auxiliary_references.is_empty());
        assert_eq!(parsed.parsed_end, record.len());
    }

    #[test]
    fn tangent_relation_reads_its_three_flags() {
        // The middle flag is `1`, which the reference-marker walk reads as the
        // presence byte of a reference and steps into the flags.
        let record = relation_record(&[(300, 1), (301, 0)], &[0, 1, 0], 201, 0x100, &[300, 301]);
        let parsed = parse_classed_sketch_relation(&record, SketchRelationClass::Tangent)
            .expect("the classed parse reads the record");
        assert_eq!(parsed.owner_reference, 201);
        assert_eq!(parsed.state, 0x100);
        assert!(parsed.auxiliary_references.is_empty());
        assert_eq!(parsed.parsed_end, record.len());
        assert!(parse_classed_sketch_relation(
            &relation_record(&[(300, 1)], &[0, 2, 0], 201, 0x100, &[300]),
            SketchRelationClass::Tangent
        )
        .is_none());
    }

    #[test]
    fn circular_pattern_relation_reads_its_parameters_and_tables() {
        let mut class_members = Vec::new();
        push_reference(&mut class_members, 336);
        push_reference(&mut class_members, 333);
        class_members.extend_from_slice(&std::f64::consts::TAU.to_le_bytes());
        class_members.extend_from_slice(&3u32.to_le_bytes());
        class_members.extend_from_slice(&empty_pattern_tables());
        class_members.push(0);
        let record = relation_record(
            &[(300, 1), (301, 0)],
            &class_members,
            201,
            0x1000_0000,
            &[300, 301],
        );
        let parsed = parse_classed_sketch_relation(&record, SketchRelationClass::CircularPattern)
            .expect("the classed parse reads the record");
        assert_eq!(parsed.owner_reference, 201);
        assert_eq!(parsed.auxiliary_references, [336, 333]);
        assert_eq!(parsed.parsed_end, record.len());
        assert_eq!(
            decode_pattern_definition(&record, &parsed),
            Some(SketchPatternDefinition::Circular {
                angle_parameter: 336,
                count_parameter: 333,
                evaluated_angle: std::f64::consts::TAU,
                evaluated_count: 3,
            })
        );
    }

    #[test]
    fn circular_pattern_relation_reads_populated_tables_and_absent_parameters() {
        let mut class_members = Vec::new();
        push_absent_reference(&mut class_members);
        push_absent_reference(&mut class_members);
        class_members.extend_from_slice(&std::f64::consts::TAU.to_le_bytes());
        class_members.extend_from_slice(&6u32.to_le_bytes());
        // One map entry keyed `1` holding two values, then a two-entry u32 run.
        class_members.extend_from_slice(&1u32.to_le_bytes());
        class_members.extend_from_slice(&1u64.to_le_bytes());
        class_members.extend_from_slice(&2u32.to_le_bytes());
        class_members.extend_from_slice(&122u64.to_le_bytes());
        class_members.extend_from_slice(&118u64.to_le_bytes());
        class_members.extend_from_slice(&2u32.to_le_bytes());
        class_members.extend_from_slice(&1u32.to_le_bytes());
        class_members.extend_from_slice(&2u32.to_le_bytes());
        class_members.push(0);
        let record = relation_record(&[(300, 1)], &class_members, 201, 0x1000_0000, &[300]);
        let parsed = parse_classed_sketch_relation(&record, SketchRelationClass::CircularPattern)
            .expect("the classed parse reads the record");
        assert_eq!(parsed.owner_reference, 201);
        assert!(parsed.auxiliary_references.is_empty());
        assert_eq!(parsed.parsed_end, record.len());
        assert_eq!(decode_pattern_definition(&record, &parsed), None);
    }

    #[test]
    fn rectangular_pattern_relation_reads_a_seed_reference_before_its_clauses() {
        let mut class_members = vec![1, 0, 0];
        class_members.extend_from_slice(&1u32.to_le_bytes());
        push_reference(&mut class_members, 900);
        class_members.extend_from_slice(&empty_pattern_tables());
        push_direction_clause(&mut class_members, 3, 464, [1.0, 0.0, 0.0], 3.0, 470);
        push_direction_clause(&mut class_members, 1, 467, [0.0, 1.0, 0.0], 0.5, 473);
        let record = relation_record(
            &[(300, 1), (301, 0)],
            &class_members,
            201,
            0x2000_0000,
            &[300, 301],
        );
        let owners = HashSet::from([201]);
        let parsed =
            parse_classed_sketch_relation(&record, SketchRelationClass::RectangularPattern)
                .expect("the classed parse reads the record");
        assert_eq!(parsed.owner_reference, 201);
        assert_eq!(parsed.auxiliary_references, [900, 464, 470, 467, 473]);
        assert_eq!(parsed.rectangular_clause_ordinal, Some(1));
        assert_eq!(parsed.parsed_end, record.len());
        assert_eq!(
            decode_pattern_definition(&record, &parsed),
            Some(SketchPatternDefinition::Rectangular {
                directions: [
                    SketchPatternDirection {
                        evaluated_count: 3,
                        count_parameter: 464,
                        direction: [1.0, 0.0, 0.0],
                        evaluated_distance: 3.0,
                        distance_parameter: 470,
                    },
                    SketchPatternDirection {
                        evaluated_count: 1,
                        count_parameter: 467,
                        direction: [0.0, 1.0, 0.0],
                        evaluated_distance: 0.5,
                        distance_parameter: 473,
                    },
                ],
            })
        );
        // The three leading flags and the seed count read as a reference whose
        // u64 target overflows a record index, so the walk cannot reach the
        // record at all.
        assert!(parse_sketch_relation(&record, &owners).is_none());
    }

    #[test]
    fn rectangular_pattern_relation_reads_clauses_without_a_seed_reference() {
        let mut class_members = vec![0, 0, 0];
        class_members.extend_from_slice(&0u32.to_le_bytes());
        class_members.extend_from_slice(&empty_pattern_tables());
        push_direction_clause(&mut class_members, 4, 464, [1.0, 0.0, 0.0], 2.0, 470);
        push_direction_clause(&mut class_members, 2, 467, [0.0, 1.0, 0.0], 1.5, 473);
        let record = relation_record(&[(300, 1)], &class_members, 201, 0x2000_0000, &[300]);
        let parsed =
            parse_classed_sketch_relation(&record, SketchRelationClass::RectangularPattern)
                .expect("the classed parse reads the record");
        assert_eq!(parsed.auxiliary_references, [464, 470, 467, 473]);
        assert_eq!(parsed.rectangular_clause_ordinal, Some(0));
        assert_eq!(parsed.parsed_end, record.len());
        let Some(SketchPatternDefinition::Rectangular { directions }) =
            decode_pattern_definition(&record, &parsed)
        else {
            panic!("expected a rectangular pattern definition");
        };
        assert_eq!(directions[0].evaluated_count, 4);
        assert_eq!(directions[1].evaluated_count, 2);
    }

    #[test]
    fn text_frame_relation_reads_its_two_references() {
        let mut class_members = Vec::new();
        push_absent_reference(&mut class_members);
        push_reference(&mut class_members, 2394);
        let record = relation_record(
            &[(2394, 0), (2403, 0)],
            &class_members,
            201,
            0x100_0000_0000,
            &[2403],
        );
        let parsed = parse_classed_sketch_relation(&record, SketchRelationClass::TextFrame)
            .expect("the classed parse reads the record");
        assert_eq!(parsed.auxiliary_references, [2394]);
        assert_eq!(parsed.parsed_end, record.len());
        assert_eq!(
            decode_pattern_definition(&record, &parsed),
            Some(SketchPatternDefinition::TextFrame {
                text_reference: 2394
            })
        );

        let mut both = Vec::new();
        push_reference(&mut both, 2404);
        push_reference(&mut both, 2394);
        let record = relation_record(
            &[(2394, 0), (2403, 0)],
            &both,
            201,
            0x100_0000_0000,
            &[2403],
        );
        let parsed = parse_classed_sketch_relation(&record, SketchRelationClass::TextFrame)
            .expect("the classed parse reads the record");
        assert_eq!(parsed.auxiliary_references, [2404, 2394]);
        assert_eq!(parsed.parsed_end, record.len());
    }

    #[test]
    fn text_path_relation_reads_its_glyph_run_at_both_versions() {
        for leading_flag in [false, true] {
            let mut class_members = Vec::new();
            if leading_flag {
                class_members.push(1);
            }
            push_glyph_run(&mut class_members, 2, 5.0);
            let record = relation_record(
                &[(1, 1), (2, 0)],
                &class_members,
                201,
                0x200_0000_0000,
                &[1],
            );
            let parsed = parse_classed_sketch_relation(
                &record,
                SketchRelationClass::TextPath { leading_flag },
            )
            .expect("the classed parse reads the record");
            assert_eq!(parsed.auxiliary_references, [2]);
            assert_eq!(parsed.parsed_end, record.len());
            let Some(SketchPatternDefinition::TextPath {
                text_reference,
                glyph_transforms,
            }) = decode_pattern_definition(&record, &parsed)
            else {
                panic!("expected a text-path pattern definition");
            };
            assert_eq!(text_reference, 2);
            assert_eq!(glyph_transforms[0][0][3], 5.0);
            // The version-0 layout has no leading byte, so reading one steps
            // into the text reference and the run no longer closes.
            assert!(parse_classed_sketch_relation(
                &record,
                SketchRelationClass::TextPath {
                    leading_flag: !leading_flag
                }
            )
            .is_none_or(|other| other.parsed_end != record.len()));
        }
    }
}
