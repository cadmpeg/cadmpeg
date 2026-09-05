// SPDX-License-Identifier: Apache-2.0
//! Parse Design sketch placements, headers, relations, and geometry.

use cadmpeg_core::container::ContainerRole;

use crate::bytes::{
    Reference, f64s_at, lp_ascii_filtered, lp_utf16_bounded, take_reference, utf16le_at,
};
use crate::container::ContainerScan;
use crate::design::{DesignFeatureFamily, design_feature_family};
use crate::ids::{self, native_stream};
use crate::layout::sketch_container_visibility_member_prefix as visibility_member;
use crate::records::{
    DESIGN_MODULE_SKETCH, DesignEntityHeader, DesignParameterScope, DesignRecordHeader,
    DesignSketchPlacement, DesignSketchVisibility, LostEdgeReference, PersistentReference,
    PersistentReferenceKind, SketchConstraintKind, SketchCurveGeometry, SketchCurveIdentity,
    SketchPoint, SketchPointClosure, SketchPointCompanion, SketchPointCompanionReferenceEncoding,
    SketchPointRecordForm, SketchRelation, SketchRelationOperand, SketchSurface, SketchText,
};
use cadmpeg_core::CodecError;
use cadmpeg_core::bytes::find_from;
use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::knots_nondecreasing;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::Color;
use std::collections::HashMap;

use super::meta::{
    decode_types, design_primary_frames, metadata_for_bulk_stream, stream_types_by_class_tag,
};

const EPS_SKETCH_DECODE_PATTERN_DEFINITION_E6: f64 = 1.0e-6;
const EPS_SKETCH_DECODE_CIRCULAR_ARC_E9: f64 = 1.0e-9;
const EPS_SKETCH_DECODE_CIRCULAR_ARC_E12: f64 = 1.0e-12;
const EPS_SKETCH_DECODE_LINE_COMPONENTS_E9: f64 = 1.0e-9;
const EPS_SKETCH_DECODE_LINE_COMPONENTS_E12: f64 = 1.0e-12;

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
    let mut visibilities = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        record_offsets.insert(
            ids::native_scope(&entry.name),
            IndexedRecordOffsets::build(bytes),
        );
        let Some(metadata) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        for (entity_suffix, visibility) in decode_sketch_visibilities_in_stream(bytes, &metadata)? {
            if visibilities
                .insert((ids::native_scope(&entry.name), entity_suffix), visibility)
                .is_some()
            {
                return Err(CodecError::malformed(format_args!(
                    "F3D Design stream {} repeats sketch visibility for entity {entity_suffix}",
                    entry.name
                )));
            }
        }
    }
    for scope in scopes
        .iter()
        .filter(|scope| design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Sketch))
    {
        let Some(binding) = scope.sketch_entity() else {
            continue;
        };
        let entity_id = binding.entity_id.as_str();
        let entity_suffix = binding.entity_suffix;
        let entry = scan.entries.iter().find(|entry| {
            scan.is_design_stream(entry, ContainerRole::Bulkstream)
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
                let Some(record_index) = View::u32_le_at(window, 1) else {
                    continue;
                };
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
    for placement in &mut out {
        let Some(stream) = native_stream(&placement.id) else {
            continue;
        };
        placement.visibility = visibilities
            .get(&(stream.to_owned(), placement.entity_suffix))
            .cloned();
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

const CURRENT_SKETCH_CONTAINER_VERSION: u32 = 18;
const SKETCH_CONTAINER_MEMBER_TYPE_GUID: &str = "37AD519C-AFB3-4CE2-9E6D-E3269FC6CDB9";
const SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
const SKETCH_CONTAINER_MEMBER_VERSION: u32 = 4;

/// Decode the direct display flag in every current sketch container's typed
/// Geometry member. Other sketch-container versions do not expose this member
/// layout and therefore leave neutral visibility unknown.
fn decode_sketch_visibilities_in_stream(
    bytes: &[u8],
    metadata: &crate::metastream::MetaStream,
) -> Result<Vec<(u64, DesignSketchVisibility)>, CodecError> {
    let mut out = Vec::new();
    for frame in super::meta::typed_primary_frames(
        bytes,
        metadata,
        SKETCH_CONTAINER_TYPE_GUID,
        "sketch-container",
    )? {
        if frame.design_type.version != CURRENT_SKETCH_CONTAINER_VERSION {
            continue;
        }
        if frame.design_type.module != DESIGN_MODULE_SKETCH
            || !frame
                .design_type
                .base_type_guid
                .as_deref()
                .is_some_and(|base| base.eq_ignore_ascii_case(SKETCH_CONTAINER_MEMBER_TYPE_GUID))
        {
            return Err(CodecError::malformed(format_args!(
                "F3D sketch container {} has incompatible registration metadata",
                frame.entity_id
            )));
        }
        let Some((entity_suffix, _, _, header_end)) =
            parse_settled_entity_header(&bytes[..frame.end], frame.start)
                .or_else(|| parse_genesis_entity_header(&bytes[..frame.end], frame.start))
        else {
            return Err(CodecError::malformed(format_args!(
                "F3D sketch container {} has an invalid entity header",
                frame.entity_id
            )));
        };
        if entity_suffix != frame.entity_id {
            return Err(CodecError::malformed(format_args!(
                "F3D sketch container {} disagrees with its entity header {entity_suffix}",
                frame.entity_id
            )));
        }
        let Some(member_at) = next_indexed_record_offset(&bytes[..frame.end], header_end) else {
            return Err(CodecError::malformed(format_args!(
                "F3D sketch container {entity_suffix} has no typed Geometry member"
            )));
        };
        let Some((class_tag, after_tag)) =
            lp_ascii_filtered(bytes, member_at, 3..=3, u8::is_ascii_digit)
        else {
            return Err(CodecError::malformed(format_args!(
                "F3D sketch container {entity_suffix} has an invalid Geometry-member class tag"
            )));
        };
        let member_type = class_tag
            .parse::<usize>()
            .ok()
            .and_then(|tag| tag.checked_sub(256))
            .and_then(|ordinal| metadata.types.get(ordinal));
        if after_tag != member_at + 7
            || !member_type.is_some_and(|member_type| {
                member_type
                    .type_guid
                    .eq_ignore_ascii_case(SKETCH_CONTAINER_MEMBER_TYPE_GUID)
                    && member_type.version == SKETCH_CONTAINER_MEMBER_VERSION
                    && member_type.module == "Geometry"
                    && member_type.base_type_guid.as_deref().is_some_and(|base| {
                        base.eq_ignore_ascii_case(SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID)
                    })
            })
        {
            return Err(CodecError::malformed(format_args!(
                "F3D sketch container {entity_suffix} has an incompatible Geometry member"
            )));
        }
        let Some(visibility) =
            decode_sketch_visibility_member(&bytes[..frame.end], member_at, entity_suffix)
        else {
            return Err(CodecError::malformed(format_args!(
                "F3D sketch container {entity_suffix} has an invalid visibility member"
            )));
        };
        out.push((entity_suffix, visibility));
    }
    Ok(out)
}

fn decode_sketch_visibility_member(
    bytes: &[u8],
    member_at: usize,
    entity_suffix: u64,
) -> Option<DesignSketchVisibility> {
    let record_index = View::u64_le_at(bytes, member_at + visibility_member::ENTITY_SUFFIX)?;
    if record_index != entity_suffix
        || bytes.get(
            member_at + visibility_member::ZERO_RUN..member_at + visibility_member::OWNER_REFERENCE,
        ) != Some(&[0; 4])
    {
        return None;
    }
    let mut cursor = member_at + visibility_member::OWNER_REFERENCE;
    let owner = take_reference(bytes, &mut cursor)?;
    if cursor != member_at + visibility_member::STREAM_ORDINAL
        || owner.target == Some(0)
        || owner.target.is_none()
        || owner.segment.is_some()
        || owner.link_name.is_some()
        || owner.inline_type_guid.is_some()
    {
        return None;
    }
    let stream_ordinal = View::u32_le_at(bytes, cursor)?;
    if stream_ordinal == 0 || bytes.get(member_at + visibility_member::RESERVED_ZERO) != Some(&0) {
        return None;
    }
    let stream_ordinal_offset = cursor;
    let visible_offset = member_at + visibility_member::VISIBLE;
    let visible = match bytes.get(visible_offset) {
        Some(0) => false,
        Some(1) => true,
        _ => return None,
    };
    if bytes.get(member_at + visibility_member::TAIL_MARKER) != Some(&1) {
        return None;
    }
    Some(DesignSketchVisibility {
        stream_ordinal,
        stream_ordinal_offset: stream_ordinal_offset as u64,
        visible_offset: visible_offset as u64,
        visible,
    })
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
    let head_index = View::u32_le_at(bytes, paired_at + 20)?;
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
        visibility: None,
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
        if !matches!(frame_length, 201 | 213 | 305 | 325 | 329 | 341) {
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
            || View::u32_le_at(bytes, paired_after_tag) != Some(record_index)
        {
            continue;
        }
        let (transform, transform_offset) = match frame_length {
            201 => (identity_matrix(), None),
            305 | 325 => {
                let Some(values) = f64s_at(bytes, start + 48, 16) else {
                    continue;
                };
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.iter().copied().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                if !valid_sketch_transform(&transform) {
                    continue;
                }
                (transform, Some((start + 48) as u64))
            }
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
            visibility: None,
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
        .filter(|(_, entry)| scan.is_design_stream(entry, ContainerRole::Bulkstream))
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
            while let Some(offset) = find_from(bytes, name, cursor) {
                cursor = offset + name.len();
                let compact_type_offset = offset + name.len();
                let type_offset = if View::u32_le_at(bytes, compact_type_offset) == Some(23) {
                    compact_type_offset
                } else if View::u32_le_at(bytes, compact_type_offset) == Some(2)
                    && View::u32_le_at(bytes, compact_type_offset + 4) == Some(14)
                    && bytes
                        .get(compact_type_offset + 8..compact_type_offset + 22)
                        .is_some()
                    && View::u32_le_at(bytes, compact_type_offset + 22) == Some(23)
                {
                    compact_type_offset + 22
                } else {
                    continue;
                };
                if View::u32_le_at(bytes, type_offset) != Some(23) {
                    continue;
                }
                let type_name = b"IntrinsicMetaTypeuint64";
                if bytes.get(type_offset + 4..type_offset + 4 + type_name.len()) != Some(type_name)
                {
                    continue;
                }
                let value_offset = type_offset + 4 + type_name.len();
                let Some(value) = View::u64_le_at(bytes, value_offset) else {
                    continue;
                };
                out.push((
                    entry_ordinal,
                    PersistentReference {
                        id: ids::native_persistent_reference_id(&entry.name, offset),
                        byte_offset: offset as u64,
                        value_offset: (value_offset - offset) as u32,
                        kind,
                        value,
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
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
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
                || View::u32_le_at(bytes, header_offset + 25) != Some(marker.len() as u32)
            {
                continue;
            }
            let Some(record_index) = View::u32_le_at(bytes, after_tag) else {
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
            let Some(next_record_index) = View::u32_le_at(bytes, after_next_tag) else {
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

/// Parse the fixed entity-header layout at `start`: a u64 entity suffix, five
/// zero bytes, an optional slot, and the UTF-16LE entity id whose numeric
/// suffix equals the header's entity suffix.
pub(crate) fn parse_settled_entity_header(
    bytes: &[u8],
    start: usize,
) -> Option<(u64, String, bool, usize)> {
    let entity_suffix = View::u64_le_at(bytes, start + 7)?;
    if entity_suffix == 0 || bytes.get(start + 15..start + 20) != Some(&[0u8; 5]) {
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
    let entity_suffix = u64::from(View::u32_le_at(bytes, start + 7)?);
    if entity_suffix == 0 {
        return None;
    }
    let mut cursor = start + 11;
    while bytes.get(cursor) == Some(&0) && cursor < start + 35 {
        cursor += 1;
    }
    if cursor == start + 11
        || bytes.get(cursor) != Some(&1)
        || View::u32_le_at(bytes, cursor + 1) != Some(1)
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
    if View::u32_le_at(bytes, paired + 7).map(u64::from) != Some(entity_suffix) {
        return empty;
    }
    let Some(count) =
        View::u32_le_at(bytes, paired + 52).and_then(|count| usize::try_from(count).ok())
    else {
        return empty;
    };
    if count == 0
        || bytes.get(paired + 56) != Some(&1)
        || bytes.get(paired + 61..paired + 67) != Some(&[0u8; 6][..])
    {
        return empty;
    }
    let Some(base_point_index) = View::u32_le_at(bytes, paired + 57) else {
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
        let Some(record_index) = View::u32_le_at(bytes, marker + 1) else {
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
    if View::u32_le_at(bytes, paired_at + 7) != Some(entity_suffix)
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
    let count = usize::try_from(View::u32_le_at(bytes, paired_at + 41)?).ok()?;
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
        member_indices.push(View::u32_le_at(bytes, marker + 1)?);
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
/// [§3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)): a three-digit class tag, an entity suffix, a UTF-16LE entity ID
/// whose numeric suffix must match the header's entity suffix, and, for
/// sketch-typed entities, the trailing reference-list header. Headers occur in
/// the fixed layout or in the `EntityGenesis` layout.
pub fn decode_entity_headers(scan: &ContainerScan) -> Result<Vec<DesignEntityHeader>, CodecError> {
    let mut out = Vec::new();
    // Entity ids are unique per Design stream, not archive-wide.
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
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
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
            let Some(entity_suffix) = View::u32_le_at(bytes, start + 7) else {
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
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode the indexed dynamic-class record headers ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)) that `entities`'
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

/// Decode the indexed dynamic-class record headers ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)) named by
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
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
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
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode the sketch-relation body at each `records` entry's offset: the
/// owning sketch relation's member reference list, owner reference, state,
/// and return-member list. `records` supplies the byte offsets and class tags
/// (typically from [`decode_related_record_headers`]).
pub fn decode_sketch_relations(
    scan: &ContainerScan,
    records: &[DesignRecordHeader],
) -> Result<Vec<SketchRelation>, CodecError> {
    let mut out = Vec::new();
    // A record carries no class identity of its own: its class tag selects an
    // entry in its segment's own type table, and only that entry's GUID names
    // the class across segments.
    let types = decode_types(scan)?;
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream_types = stream_types_by_class_tag(&types, &entry.name);
        let scope = ids::native_scope(&entry.name);
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
            let class = record
                .class_tag
                .parse::<u32>()
                .ok()
                .and_then(|class_tag| stream_types.get(&class_tag))
                .and_then(|design_type| {
                    SketchRelationClass::of(&design_type.type_guid, design_type.version)
                });
            let parsed = class.and_then(|class| parse_classed_sketch_relation(payload, class));
            let Some(parsed) = parsed else {
                continue;
            };
            if payload
                .get(parsed.parsed_end..)
                .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
            {
                continue;
            }
            let pattern = decode_pattern_definition(payload, &parsed);
            let kind = crate::records::SketchRelationKind::from_pattern(pattern);
            if !kind.agrees_with_state(parsed.state) {
                continue;
            }
            let Ok(members) = crate::records::zip_relation_members(
                parsed.members,
                parsed
                    .member_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                parsed.member_relation_ordinals,
                Vec::new(),
            ) else {
                continue;
            };
            let Ok(return_members) = crate::records::zip_return_members(
                parsed.return_members,
                parsed
                    .return_member_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                Vec::new(),
            ) else {
                continue;
            };
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
                rectangular_counted_reference_count: parsed.rectangular_reference_count,
                members,
                state: parsed.state,
                entity_genesis: parsed.entity_genesis,
                kind,
                return_members,
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
/// source distance, and the distance-parameter reference. A non-empty counted
/// reference run stores adjacent spacing; an empty run stores the total
/// seed-to-final span. Text-frame relations repeat the sketch-text member as an
/// auxiliary reference.
pub(crate) fn decode_pattern_definition(
    payload: &[u8],
    parsed: &ParsedSketchRelation,
) -> Option<crate::records::SketchPatternDefinition> {
    use crate::records::{SketchPatternDefinition, SketchPatternDirection};
    let f64_at = |at: usize| View::f64_le_at(payload, at).filter(|value| value.is_finite());
    let reference_end = |ordinal: usize| Some(parsed.auxiliary_reference_offsets.get(ordinal)? + 4);
    if parsed.state == 0x1000_0000 && parsed.auxiliary_references.len() == 2 {
        let angle_at = reference_end(1)? + 6;
        let evaluated_angle = f64_at(angle_at)?;
        let evaluated_count = View::u32_le_at(payload, angle_at + 8)?;
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
        // them. The rectangular class parser records their exact position only
        // when all four parameter references are present.
        let clause_ordinal = parsed.rectangular_clause_ordinal?;
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
            let evaluated_count = View::u32_le_at(payload, count_at)?;
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
            if (length - 1.0).abs() > EPS_SKETCH_DECODE_PATTERN_DEFINITION_E6 {
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

pub(crate) fn decode_constraint_kinds(state: u64) -> (Vec<SketchConstraintKind>, u64) {
    crate::records::constraint_kinds_from_state(state)
}

pub(crate) fn trailing_sketch_owner_reference(record: &[u8]) -> Option<u32> {
    let tail = record.len().checked_sub(11)?;
    if record.get(tail) != Some(&1) || record.get(tail + 5..tail + 11) != Some(&[0u8; 6][..]) {
        return None;
    }
    View::u32_le_at(record, tail + 1)
}

pub(crate) fn decode_sketch_points_from_stream(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchPoint>, CodecError> {
    let frames = design_primary_frames(bytes, meta)?;
    let frames_by_entity = frames
        .iter()
        .filter_map(|frame| Some((u32::try_from(frame.entity_id).ok()?, frame)))
        .collect::<HashMap<_, _>>();
    let types_by_entity = frames
        .iter()
        .filter_map(|frame| {
            Some((
                u32::try_from(frame.entity_id).ok()?,
                (
                    frame.design_type.type_guid.as_str(),
                    frame.design_type.version,
                    frame.design_type.module.as_str(),
                ),
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for frame in &frames {
        if !frame
            .design_type
            .type_guid
            .eq_ignore_ascii_case(SKETCH_POINT_TYPE_GUID)
            || frame.design_type.module != CURRENT_SKETCH_POINT_TYPE.2
            || ![0, 8, 10, CURRENT_SKETCH_POINT_TYPE.1].contains(&frame.design_type.version)
        {
            continue;
        }
        let payload = &bytes[frame.start..frame.end];
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-point entity ID exceeds u32".into()))?;
        let decoded =
            decode_sketch_point_record(payload, frame.design_type.version).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D sketch point {record_index} has an invalid version-{} member sequence",
                    frame.design_type.version
                ))
            })?;
        let (u, v, depth) = (
            decoded.coordinates[0] * 10.0,
            decoded.coordinates[1] * 10.0,
            decoded.coordinates[2] * 10.0,
        );
        if !u.is_finite() || !v.is_finite() || !depth.is_finite() {
            return Err(CodecError::malformed(format_args!(
                "F3D sketch point {record_index} has a non-finite coordinate"
            )));
        }
        if !point_target_has_guid(
            &types_by_entity,
            decoded.trailing_reference(),
            SKETCH_CONTAINER_TYPE_GUID,
        ) {
            return Err(CodecError::malformed(format_args!(
                "F3D sketch point {record_index} has an invalid trailing container reference"
            )));
        }
        let companion_encoding = if decoded.record_form.uses_inline_typed_references() {
            SketchPointCompanionReferenceEncoding::InlineTyped
        } else {
            SketchPointCompanionReferenceEncoding::SameSegment
        };
        let companion = frames_by_entity
            .get(&decoded.paired_reference)
            .filter(|companion_frame| {
                let design_type = companion_frame.design_type;
                design_type
                    .type_guid
                    .eq_ignore_ascii_case(SKETCH_POINT_COMPANION_TYPE.0)
                    && design_type.version == SKETCH_POINT_COMPANION_TYPE.1
                    && design_type.module == SKETCH_POINT_COMPANION_TYPE.2
            })
            .and_then(|companion_frame| {
                decode_sketch_point_companion(
                    &bytes[companion_frame.start..companion_frame.end],
                    record_index,
                    companion_encoding,
                    matches!(
                        decoded.record_form,
                        SketchPointRecordForm::Version11 { .. }
                            | SketchPointRecordForm::Version11InlineTyped { .. }
                    ),
                    &types_by_entity,
                )
            })
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "F3D sketch point {record_index} has no valid inverse companion"
                ))
            })?;
        if decoded.record_form.uses_inline_typed_references()
            != (companion.reference_encoding == SketchPointCompanionReferenceEncoding::InlineTyped)
        {
            return Err(CodecError::malformed(format_args!(
                "F3D sketch point {record_index} and its companion use different reference encodings"
            )));
        }
        out.push(SketchPoint {
            id: ids::native_sketch_point_id(stream, frame.start),
            record_index,
            owner_reference: decoded.owner_reference,
            class_tag: frame.class_tag.to_string(),
            byte_offset: frame.start as u64,
            coordinate_offset: decoded.coordinate_offset,
            entity_genesis: decoded.entity_genesis,
            record_form: decoded.record_form,
            paired_reference: decoded.paired_reference,
            coordinates: Point2::new(u, v),
            depth,
            companion: Some(companion),
        });
    }
    Ok(out)
}

/// Decode every sketch-point record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `pt_tag`) from each design
/// `BulkStream` entry in `scan`: its versioned identity, flags, coordinates,
/// closure, trailing reference, and paired reverse curve-incidence record,
/// converted centimetre→millimetre. Class version 0 supplies `(u,v)` and no
/// persistent identity; later forms supply `(u,v,w)` and `pt_tag`. A known
/// point record with a malformed or non-finite member sequence makes the
/// stream malformed.
pub fn decode_sketch_points(scan: &ContainerScan) -> Result<Vec<SketchPoint>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        out.extend(decode_sketch_points_from_stream(bytes, &meta, &entry.name)?);
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
            let count = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
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
                properties.push((key, View::u64_le_at(payload, after_type)?));
                *cursor = after_type.checked_add(8)?;
            }
        }
        _ => return None,
    }
    Some(properties)
}

const SKETCH_TEXT_TYPE_GUIDS: [&str; 2] = [
    "E0618268-3A06-450E-9E94-7CF4C2E66802",
    "F0B1AFA3-3BAF-42D0-B2F3-94B95662F2A9",
];

/// Decode sketch-text records carrying persistent identities, font metrics,
/// UTF-16 content, and an owning-sketch reference.
pub(crate) fn decode_sketch_texts_from_stream(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchText>, CodecError> {
    let mut out = Vec::new();
    for frame in design_primary_frames(bytes, meta)? {
        if !SKETCH_TEXT_TYPE_GUIDS
            .iter()
            .any(|type_guid| frame.design_type.type_guid.eq_ignore_ascii_case(type_guid))
        {
            continue;
        }
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-text entity ID exceeds u32".into()))?;
        let class_tag = frame.class_tag.to_string();
        let payload = &bytes[frame.start..frame.end];
        if let Some(text) = decode_sketch_text_record(
            payload,
            stream,
            class_tag,
            frame.design_type.version,
            record_index,
            frame.start,
        ) {
            out.push(text);
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
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        out.extend(decode_sketch_texts_from_stream(bytes, &meta, &entry.name)?);
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
const TEXT_PLACEMENT_TOLERANCE: f64 = 1.0e-9;

/// Bytes between the end of the stored `txt_tag` rotation and the four f32 RGBA
/// components. The first byte is zero and four further bytes are unclassified.
const TXT_TAG_POST_ROTATION_RUN: usize = 5;

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

/// Three unknown bytes, one i32 font weight, and eight further bytes between
/// the `txt_tag` form's counted reference run and the thirty-byte class tail.
const TXT_TAG_MEMBER_RUN: usize = 15;
const TXT_TAG_FONT_WEIGHT_AT: usize = 3;

/// Which identity key a sketch-text record carries. The key selects the layout
/// the record uses from the property block onward.
#[derive(Clone, Copy, PartialEq)]
enum SketchTextIdentity {
    /// `textex_tag`: a `1` byte, the width factor, a zero byte between the font
    /// family and the height, and the anchor inside a placement transform.
    TextexTag,
    /// `txt_tag`: a `0` byte, no width factor, the height directly after the
    /// font family, and the anchor stored on its own.
    TxtTag { rotation: f64 },
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
    let mut view = View::over_retained(payload);
    view.seek(*cursor)?;
    let mut components = [0f32; 4];
    for component in &mut components {
        let value = view.f32_le()?;
        (0.0..=1.0).contains(&value).then_some(())?;
        *component = value;
    }
    *cursor = view.position();
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
    horizontal_alignment: Option<u32>,
    vertical_alignment: Option<u32>,
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
            let count = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
            if count > MAX_RELATION_RUN {
                return None;
            }
            *cursor = cursor.checked_add(4)?;
            for _ in 0..count {
                take_reference(payload, cursor)?;
                View::u32_le_at(payload, *cursor)?;
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
    let is_txt_tag = match (property("textex_tag"), property("txt_tag")) {
        (Some(_), _) => false,
        (None, Some(_)) => true,
        (None, None) if class_version < TXT_TAG_IDENTITY_KEY_VERSION => true,
        (None, None) => return None,
    };
    let identity = if is_txt_tag {
        let rotation = View::f64_le_at(payload, cursor)?;
        rotation.is_finite().then_some(())?;
        SketchTextIdentity::TxtTag { rotation }
    } else {
        SketchTextIdentity::TextexTag
    };
    let property = |key: &str| {
        properties
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| *value)
    };
    let persistent_id = match identity {
        SketchTextIdentity::TextexTag => property("textex_tag"),
        SketchTextIdentity::TxtTag { .. } => property("txt_tag"),
    };
    let (width_factor, color) = match identity {
        SketchTextIdentity::TextexTag => {
            (payload.get(cursor)? == &1).then_some(())?;
            cursor += 1;
            let width_factor = View::f64_le_at(payload, cursor)?;
            cursor = cursor.checked_add(8)?;
            let color = read_sketch_text_color(payload, &mut cursor)?;
            (width_factor.is_finite() && width_factor >= 0.0).then_some(())?;
            (Some(width_factor), color)
        }
        SketchTextIdentity::TxtTag { .. } => {
            cursor = cursor.checked_add(8)?;
            (payload.get(cursor)? == &0).then_some(())?;
            cursor = cursor.checked_add(TXT_TAG_POST_ROTATION_RUN)?;
            let color = read_sketch_text_color(payload, &mut cursor)?;
            (None, color)
        }
    };
    let font_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
    if font_count == 0 || font_count > 1_024 {
        return None;
    }
    let (font_family, after_font) = utf16le_at(payload, cursor + 4, font_count)?;
    cursor = after_font;
    if matches!(identity, SketchTextIdentity::TextexTag) {
        (payload.get(cursor)? == &0).then_some(())?;
        cursor += 1;
    }
    let height = View::f64_le_at(payload, cursor)? * 10.0;
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

/// Read the indexed Design form of a `textex_tag` record. It has no leading
/// relation block or record-name string: the indexed header is followed by a
/// nine-byte zero entity lane and the ordinary property block. Its one-byte
/// width prefix is zero, unlike the legacy class form's one-byte prefix of
/// one; the f64 width factor and the remaining metrics have the same roles.
fn decode_indexed_sketch_text_head(payload: &[u8]) -> Option<SketchTextHead> {
    let (_, after_tag) = lp_ascii_filtered(payload, 0, 3..=3, u8::is_ascii_digit)?;
    if after_tag != 7
        || View::u32_le_at(payload, after_tag).is_none()
        || payload.get(11..20)?.iter().any(|byte| *byte != 0)
    {
        return None;
    }
    let mut cursor = 20;
    let properties = read_property_block(payload, &mut cursor)?;
    let property = |key: &str| {
        properties
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| *value)
    };
    let persistent_id = property("textex_tag")?;
    (payload.get(cursor)? == &0).then_some(())?;
    cursor += 1;
    let width_factor = View::f64_le_at(payload, cursor)?;
    cursor = cursor.checked_add(8)?;
    (width_factor.is_finite() && width_factor >= 0.0).then_some(())?;
    let color = read_sketch_text_color(payload, &mut cursor)?;
    let font_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
    if font_count == 0 || font_count > 1_024 {
        return None;
    }
    let (font_family, after_font) = utf16le_at(payload, cursor + 4, font_count)?;
    cursor = after_font;
    (payload.get(cursor)? == &0).then_some(())?;
    cursor += 1;
    let height = View::f64_le_at(payload, cursor)? * 10.0;
    cursor = cursor.checked_add(8)?;
    (height.is_finite() && height > 0.0).then_some(())?;
    Some(SketchTextHead {
        identity: SketchTextIdentity::TextexTag,
        entity_genesis: property("EntityGenesis"),
        persistent_id: Some(persistent_id),
        base_id: property("txt_tag_base"),
        font_family,
        height,
        width_factor: Some(width_factor),
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
    let horizontal_alignment = Some(View::u32_le_at(payload, cursor)?);
    cursor = cursor.checked_add(7)?;
    let text_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
    if text_count == 0 || text_count > 1_048_576 {
        return None;
    }
    let (text, after_text) = utf16le_at(payload, cursor + 4, text_count)?;
    cursor = after_text;
    let second_reference = read_text_reference(payload, &mut cursor, second_slot)?;
    // Vertical alignment enum, one flag byte, and the font weight.
    let vertical_alignment = Some(View::u32_le_at(payload, cursor)?);
    let font_weight = View::u32_le_at(payload, cursor.checked_add(5)?)? as i32;
    matches!(font_weight, 400 | 500 | 750).then_some(())?;
    cursor = cursor.checked_add(9)?;
    // The class tail opens with the text-type enum, which gates the placement
    // transform: frame text stores a 4x4 transform, path text stores none. One
    // flag byte follows the enum and repeats it, so a slot form that has
    // desynchronized fails here instead of framing a transform out of whatever
    // bytes the walk landed on.
    let text_type = View::u32_le_at(payload, cursor)?;
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
        horizontal_alignment,
        vertical_alignment,
        owner_reference: reference_index(&owner)?,
    })
}

/// Read the `txt_tag` form's members from the height to the end of the record.
/// Two bytes separate the height from the anchor coordinates, which this form
/// stores directly rather than in a placement transform. The form writes no
/// parameter-reference slot: an eleven-byte unclassified member run, ten bytes
/// below [`TXT_TAG_ANCHOR_MEMBER_VERSION`], follows the anchor. The text string
/// is followed by a counted reference run, fifteen bytes, and the trailing run
/// and owning-sketch reference that close both forms.
fn decode_txt_tag_sketch_text_tail(
    payload: &[u8],
    mut cursor: usize,
    class_version: u32,
    rotation: f64,
) -> Option<SketchTextTail> {
    cursor = cursor.checked_add(2)?;
    let anchor = Point2::new(
        View::f64_le_at(payload, cursor)? * 10.0,
        View::f64_le_at(payload, cursor.checked_add(8)?)? * 10.0,
    );
    (anchor.u.is_finite() && anchor.v.is_finite()).then_some(())?;
    let anchor_run = if class_version < TXT_TAG_ANCHOR_MEMBER_VERSION {
        TXT_TAG_ANCHOR_RUN - 1
    } else {
        TXT_TAG_ANCHOR_RUN
    };
    cursor = cursor.checked_add(16 + anchor_run)?;
    let text_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
    if text_count == 0 || text_count > 1_048_576 {
        return None;
    }
    let (text, after_text) = utf16le_at(payload, cursor + 4, text_count)?;
    cursor = after_text;
    let references = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
    if references > MAX_RELATION_RUN {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    for _ in 0..references {
        take_reference(payload, &mut cursor)?;
    }
    let font_weight = View::u32_le_at(payload, cursor.checked_add(TXT_TAG_FONT_WEIGHT_AT)?)? as i32;
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
        rotation: Some(rotation),
        horizontal_alignment: None,
        vertical_alignment: None,
        owner_reference: reference_index(&owner)?,
    })
}

/// Read the indexed `textex_tag` tail. The member and alignment fields match
/// the common text body. The indexed class then writes a fixed 35-byte suffix:
/// a text-type u32, three fixed u32 values, a zero u32, a two-byte zero run,
/// two positive f32 scales, and a five-byte zero run before the owning-sketch
/// reference. The text-type values are the same frame (`0`) and path (`1`)
/// discriminators used by the legacy class tail. The indexed suffix carries no
/// neutral anchor or rotation; the source record is retained in full.
fn decode_indexed_sketch_text_tail(
    payload: &[u8],
    mut cursor: usize,
    first_slot: TextReferenceSlot,
    second_slot: TextReferenceSlot,
) -> Option<SketchTextTail> {
    let first_reference = read_text_reference(payload, &mut cursor, first_slot)?;
    let horizontal_alignment = Some(View::u32_le_at(payload, cursor)?);
    cursor = cursor.checked_add(7)?;
    let text_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
    if text_count == 0 || text_count > 1_048_576 {
        return None;
    }
    let (text, after_text) = utf16le_at(payload, cursor + 4, text_count)?;
    cursor = after_text;
    let second_reference = read_text_reference(payload, &mut cursor, second_slot)?;
    let vertical_alignment = Some(View::u32_le_at(payload, cursor)?);
    let font_weight = View::u32_le_at(payload, cursor.checked_add(5)?)? as i32;
    matches!(font_weight, 400 | 500 | 750).then_some(())?;
    cursor = cursor.checked_add(9)?;
    if !matches!(View::u32_le_at(payload, cursor)?, 0 | 1)
        || View::u32_le_at(payload, cursor.checked_add(4)?)? != 1
        || View::u32_le_at(payload, cursor.checked_add(8)?)? != 256
        || View::u32_le_at(payload, cursor.checked_add(12)?)? != 0
        || View::u32_le_at(payload, cursor.checked_add(16)?)? != 0
        || payload
            .get(cursor.checked_add(20)?..cursor.checked_add(22)?)?
            .iter()
            .any(|byte| *byte != 0)
    {
        return None;
    }
    let mut scale_view = View::over_retained(payload);
    scale_view.seek(cursor.checked_add(22)?)?;
    let scale_u = scale_view.f32_le()?;
    let scale_v = scale_view.f32_le()?;
    if !scale_u.is_finite() || !scale_v.is_finite() || scale_u <= 0.0 || scale_v <= 0.0 {
        return None;
    }
    (payload
        .get(scale_view.position()..cursor.checked_add(35)?)?
        .iter()
        .all(|byte| *byte == 0))
    .then_some(())?;
    cursor = cursor.checked_add(35)?;
    let owner = take_reference(payload, &mut cursor)?;
    (cursor == payload.len()).then_some(())?;
    Some(SketchTextTail {
        first_reference: reference_index(&first_reference),
        second_reference: reference_index(&second_reference),
        text,
        font_weight,
        anchor: None,
        rotation: None,
        horizontal_alignment,
        vertical_alignment,
        owner_reference: reference_index(&owner)?,
    })
}

fn decode_indexed_sketch_text_record_tail(payload: &[u8], cursor: usize) -> Option<SketchTextTail> {
    let mut closed = None;
    for first_slot in TEXT_REFERENCE_SLOTS {
        for second_slot in TEXT_REFERENCE_SLOTS {
            let Some(tail) =
                decode_indexed_sketch_text_tail(payload, cursor, first_slot, second_slot)
            else {
                continue;
            };
            if closed.replace(tail).is_some() {
                return None;
            }
        }
    }
    closed
}

#[allow(clippy::too_many_arguments)]
fn assemble_sketch_text(
    payload: &[u8],
    stream: &str,
    class_tag: String,
    class_version: u32,
    record_index: u32,
    byte_offset: usize,
    head: SketchTextHead,
    tail: SketchTextTail,
) -> SketchText {
    let layout = match head.identity {
        SketchTextIdentity::TxtTag { .. } => crate::records::SketchTextLayout::TxtTag {
            anchor: tail.anchor.unwrap_or(Point2::new(0.0, 0.0)),
            rotation: tail.rotation.unwrap_or(0.0),
        },
        SketchTextIdentity::TextexTag => crate::records::SketchTextLayout::TextexTag {
            width_factor: head.width_factor.unwrap_or(0.0),
            horizontal_alignment: tail.horizontal_alignment,
            vertical_alignment: tail.vertical_alignment,
            first_reference: tail.first_reference,
            second_reference: tail.second_reference,
            anchor: tail.anchor,
            rotation: tail.rotation,
        },
    };
    SketchText {
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
        color: head.color,
        layout,
        raw_bytes: payload.to_vec(),
    }
}

pub(crate) fn decode_sketch_text_record(
    payload: &[u8],
    stream: &str,
    class_tag: String,
    class_version: u32,
    record_index: u32,
    byte_offset: usize,
) -> Option<SketchText> {
    if let Some(head) = decode_sketch_text_head(payload, class_version) {
        let tail = match head.identity {
            SketchTextIdentity::TextexTag => {
                let mut closed = None;
                let mut ambiguous = false;
                'forms: for first_slot in TEXT_REFERENCE_SLOTS {
                    for second_slot in TEXT_REFERENCE_SLOTS {
                        let Some(tail) =
                            decode_sketch_text_tail(payload, head.cursor, first_slot, second_slot)
                        else {
                            continue;
                        };
                        // Two slot forms both ending on the owning-sketch reference
                        // leave the parameter references undetermined.
                        if closed.replace(tail).is_some() {
                            ambiguous = true;
                            break 'forms;
                        }
                    }
                }
                if ambiguous { None } else { closed }
            }
            SketchTextIdentity::TxtTag { rotation } => {
                decode_txt_tag_sketch_text_tail(payload, head.cursor, class_version, rotation)
            }
        };
        if let Some(tail) = tail {
            return Some(assemble_sketch_text(
                payload,
                stream,
                class_tag,
                class_version,
                record_index,
                byte_offset,
                head,
                tail,
            ));
        }
    }
    let head = decode_indexed_sketch_text_head(payload)?;
    let tail = decode_indexed_sketch_text_record_tail(payload, head.cursor)?;
    Some(assemble_sketch_text(
        payload,
        stream,
        class_tag,
        class_version,
        record_index,
        byte_offset,
        head,
        tail,
    ))
}

#[derive(Debug)]
struct DecodedSketchPoint {
    owner_reference: Option<u32>,
    coordinate_offset: u32,
    entity_genesis: Option<u64>,
    record_form: SketchPointRecordForm,
    paired_reference: u32,
    coordinates: [f64; 3],
}

impl DecodedSketchPoint {
    fn trailing_reference(&self) -> Option<u32> {
        match self.record_form {
            SketchPointRecordForm::Version10InlineTyped {
                trailing_reference, ..
            }
            | SketchPointRecordForm::Version11InlineTyped {
                trailing_reference, ..
            } => Some(trailing_reference),
            _ => self.owner_reference,
        }
    }
}

fn take_local_sketch_reference(
    payload: &[u8],
    cursor: &mut usize,
) -> Option<(u32, Option<String>)> {
    let reference = take_reference(payload, cursor)?;
    if reference.segment.is_some() || reference.link_name.is_some() {
        return None;
    }
    Some((
        u32::try_from(reference.target?).ok()?,
        reference.inline_type_guid,
    ))
}

fn take_same_segment_sketch_reference(payload: &[u8], cursor: &mut usize) -> Option<u32> {
    let (target, inline_type_guid) = take_local_sketch_reference(payload, cursor)?;
    inline_type_guid.is_none().then_some(target)
}

fn decode_version_zero_sketch_point(
    payload: &[u8],
    header_end: usize,
) -> Option<DecodedSketchPoint> {
    let mut cursor = header_end;
    let prefix_end = cursor.checked_add(10)?;
    if payload.get(cursor..prefix_end) != Some(&[0; 10][..]) {
        return None;
    }
    cursor = prefix_end;
    let paired_reference = take_same_segment_sketch_reference(payload, &mut cursor)?;
    let flag = *payload.get(cursor)?;
    if flag > 1 {
        return None;
    }
    cursor = cursor.checked_add(1)?;
    let coordinate_offset = u32::try_from(cursor).ok()?;
    let x = View::f64_le_at(payload, cursor)?;
    let y = View::f64_le_at(payload, cursor.checked_add(8)?)?;
    cursor = cursor.checked_add(16)?;
    if payload.get(cursor..cursor.checked_add(20)?) != Some(&[0; 20][..])
        || View::f32_le_at(payload, cursor + 20) != Some(1.0)
        || payload.get(cursor + 24..cursor + 36) != Some(&[0; 12][..])
        || View::f32_le_at(payload, cursor + 36) != Some(1.0)
        || View::f32_le_at(payload, cursor + 40) != Some(1.0)
        || payload.get(cursor + 44..cursor + 54) != Some(&[1, 1, 0, 0, 0, 0, 1, 0, 0, 0][..])
    {
        return None;
    }
    cursor = cursor.checked_add(54)?;
    if take_same_segment_sketch_reference(payload, &mut cursor)? != paired_reference {
        return None;
    }
    let owner_reference = take_same_segment_sketch_reference(payload, &mut cursor)?;
    if cursor != payload.len() {
        return None;
    }
    Some(DecodedSketchPoint {
        owner_reference: Some(owner_reference),
        coordinate_offset,
        entity_genesis: None,
        record_form: SketchPointRecordForm::Version0 { flag },
        paired_reference,
        coordinates: [x, y, 0.0],
    })
}

fn decode_sketch_point_record(payload: &[u8], class_version: u32) -> Option<DecodedSketchPoint> {
    let (_, after_class_tag) = lp_ascii_filtered(payload, 0, 3..=3, u8::is_ascii_digit)?;
    let header_end = after_class_tag.checked_add(4)?;
    if class_version == 0 {
        return decode_version_zero_sketch_point(payload, header_end);
    }
    if !matches!(class_version, 8 | 10 | 11)
        || payload.get(header_end..header_end.checked_add(9)?) != Some(&[0; 9][..])
    {
        return None;
    }
    let mut cursor = header_end.checked_add(9)?;
    let properties = read_property_block(payload, &mut cursor)?;
    let (entity_genesis, persistent_id) = match properties.as_slice() {
        [(key, persistent_id)] if key == "pt_tag" => (None, *persistent_id),
        [(genesis_key, entity_genesis), (point_key, persistent_id)]
            if class_version == 11 && genesis_key == "EntityGenesis" && point_key == "pt_tag" =>
        {
            (Some(*entity_genesis), *persistent_id)
        }
        _ => return None,
    };
    let (paired_reference, paired_type_guid) = take_local_sketch_reference(payload, &mut cursor)?;
    let inline_typed = match (class_version, paired_type_guid.as_deref()) {
        (8 | 10 | 11, None) => false,
        (10 | 11, Some(type_guid))
            if type_guid.eq_ignore_ascii_case(SKETCH_POINT_COMPANION_TYPE.0) =>
        {
            true
        }
        _ => return None,
    };
    let flag_count = if class_version == 11 { 8 } else { 7 };
    let flags_end = cursor.checked_add(flag_count)?;
    let source_flags = payload.get(cursor..flags_end)?;
    if source_flags.iter().any(|flag| *flag > 1) {
        return None;
    }
    let mut flags = [0; 8];
    flags[..flag_count].copy_from_slice(source_flags);
    cursor = flags_end;
    let coordinate_offset = u32::try_from(cursor).ok()?;
    let coordinates = [
        View::f64_le_at(payload, cursor)?,
        View::f64_le_at(payload, cursor.checked_add(8)?)?,
        View::f64_le_at(payload, cursor.checked_add(16)?)?,
    ];
    cursor = cursor.checked_add(24)?;
    let selector = View::u64_le_at(payload, cursor)?;
    let state = *payload.get(cursor.checked_add(8)?)?;
    let reserved_zero_count = if class_version == 8 { 8 } else { 12 };
    let floats_at = cursor.checked_add(9 + reserved_zero_count)?;
    if payload
        .get(cursor + 9..floats_at)?
        .iter()
        .any(|byte| *byte != 0)
        || View::f32_le_at(payload, floats_at) != Some(1.0)
        || View::f32_le_at(payload, floats_at + 4) != Some(1.0)
        || payload.get(floats_at + 8..floats_at + 13) != Some(&[0, 1, 0, 0, 0][..])
    {
        return None;
    }
    cursor = floats_at.checked_add(13)?;
    let (repeated_reference, repeated_type_guid) =
        take_local_sketch_reference(payload, &mut cursor)?;
    let repeated_encoding_matches = match repeated_type_guid.as_deref() {
        None => !inline_typed,
        Some(type_guid) => {
            inline_typed && type_guid.eq_ignore_ascii_case(SKETCH_POINT_COMPANION_TYPE.0)
        }
    };
    if repeated_reference != paired_reference || !repeated_encoding_matches {
        return None;
    }
    let closure = SketchPointClosure::from_pair(selector, state)?;
    let mut seven = [0; 7];
    seven.copy_from_slice(&flags[..7]);
    let (record_form, owner_reference) =
        match (class_version, inline_typed) {
            (8, false) => {
                if closure != SketchPointClosure::Selector0State0 {
                    return None;
                }
                let owner = take_same_segment_sketch_reference(payload, &mut cursor)?;
                (
                    SketchPointRecordForm::Version8 {
                        persistent_id,
                        flags: seven,
                    },
                    Some(owner),
                )
            }
            (10, false) => {
                let owner = take_same_segment_sketch_reference(payload, &mut cursor)?;
                (
                    SketchPointRecordForm::Version10 {
                        persistent_id,
                        flags: seven,
                        closure: crate::records::SketchPointClosure10::from_closure(closure)?,
                    },
                    Some(owner),
                )
            }
            (10, true) => {
                let (trailing_reference, type_guid) =
                    take_local_sketch_reference(payload, &mut cursor)?;
                if type_guid.as_deref().is_none_or(|type_guid| {
                    !type_guid.eq_ignore_ascii_case(SKETCH_CONTAINER_TYPE_GUID)
                }) {
                    return None;
                }
                (
                    SketchPointRecordForm::Version10InlineTyped {
                        trailing_reference,
                        persistent_id,
                        flags: seven,
                        closure: crate::records::SketchPointClosure10Inline::from_closure(closure)?,
                    },
                    None,
                )
            }
            (11, true) => {
                let (trailing_reference, type_guid) =
                    take_local_sketch_reference(payload, &mut cursor)?;
                if type_guid.as_deref().is_none_or(|type_guid| {
                    !type_guid.eq_ignore_ascii_case(SKETCH_CONTAINER_TYPE_GUID)
                }) {
                    return None;
                }
                (
                    SketchPointRecordForm::Version11InlineTyped {
                        trailing_reference,
                        persistent_id,
                        flags,
                        closure,
                    },
                    None,
                )
            }
            (11, false) => {
                let padded_paired_reference = match payload.len().checked_sub(cursor)? {
                    11 => false,
                    15 if payload.get(cursor..cursor + 4) == Some(&[0; 4][..]) => {
                        cursor += 4;
                        true
                    }
                    _ => return None,
                };
                let owner = take_same_segment_sketch_reference(payload, &mut cursor)?;
                (
                    SketchPointRecordForm::Version11 {
                        padded_paired_reference,
                        persistent_id,
                        flags,
                        closure,
                    },
                    Some(owner),
                )
            }
            _ => return None,
        };
    if cursor != payload.len() {
        return None;
    }
    Some(DecodedSketchPoint {
        owner_reference,
        coordinate_offset,
        entity_genesis,
        record_form,
        paired_reference,
        coordinates,
    })
}

fn point_target_has_guid(
    types_by_entity: &HashMap<u32, (&str, u32, &str)>,
    target: Option<u32>,
    expected_guid: &str,
) -> bool {
    let Some(target) = target else {
        return false;
    };
    types_by_entity
        .get(&target)
        .is_some_and(|(type_guid, _, _)| type_guid.eq_ignore_ascii_case(expected_guid))
}

fn decode_sketch_point_companion(
    payload: &[u8],
    point_record_index: u32,
    reference_encoding: SketchPointCompanionReferenceEncoding,
    allow_present_zero_prefix: bool,
    types_by_entity: &HashMap<u32, (&str, u32, &str)>,
) -> Option<SketchPointCompanion> {
    let (prefix_present_zero, mut cursor) = if payload.get(11..21) == Some(&[0; 10][..]) {
        (false, 21)
    } else if payload.get(11..20) == Some(&[0; 9][..])
        && payload.get(20..25) == Some(&[1, 0, 0, 0, 0][..])
    {
        allow_present_zero_prefix.then_some((true, 25))?
    } else {
        return None;
    };
    let count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
    if count > MAX_RELATION_RUN {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let mut incident_curves = Vec::with_capacity(count);
    for _ in 0..count {
        let (target, type_guid) = take_local_sketch_reference(payload, &mut cursor)?;
        let registered_type = types_by_entity.get(&target)?;
        match reference_encoding {
            SketchPointCompanionReferenceEncoding::SameSegment if type_guid.is_none() => {}
            SketchPointCompanionReferenceEncoding::InlineTyped
                if type_guid
                    .as_deref()
                    .is_some_and(|type_guid| type_guid.eq_ignore_ascii_case(registered_type.0)) => {
            }
            _ => return None,
        }
        incident_curves.push(target);
    }
    if payload.get(cursor) != Some(&0) {
        return None;
    }
    cursor = cursor.checked_add(1)?;
    let (inverse, inverse_type_guid) = take_local_sketch_reference(payload, &mut cursor)?;
    let inverse_encoding_matches = match reference_encoding {
        SketchPointCompanionReferenceEncoding::SameSegment => inverse_type_guid.is_none(),
        SketchPointCompanionReferenceEncoding::InlineTyped => inverse_type_guid
            .as_deref()
            .is_some_and(|type_guid| type_guid.eq_ignore_ascii_case(SKETCH_POINT_TYPE_GUID)),
    };
    if inverse != point_record_index || !inverse_encoding_matches || cursor != payload.len() {
        return None;
    }
    Some(SketchPointCompanion {
        prefix_present_zero,
        reference_encoding,
        incident_curves,
    })
}

pub(crate) const SKETCH_POINT_TYPE_GUID: &str = "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB";
pub(crate) const CURRENT_SKETCH_POINT_TYPE: (&str, u32, &str) =
    (SKETCH_POINT_TYPE_GUID, 11, "Geometry");
pub(crate) const SKETCH_POINT_COMPANION_TYPE: (&str, u32, &str) =
    ("362B7EC3-0F09-47C8-A3BE-DC066715CDAE", 0, "Geometry");
pub(crate) const SKETCH_CONTAINER_TYPE_GUID: &str = "44A64366-4BD3-4B24-881A-F94C206E8F2D";
pub(crate) const CURRENT_SKETCH_LINE_TYPE: (&str, u32, &str) =
    ("DCA267ED-D615-4934-B64F-AD805E8003E2", 2, "Geometry");
pub(crate) const CURRENT_SKETCH_CIRCULAR_TYPE: (&str, u32, &str) =
    ("F0130424-8B7E-4092-93C9-1CA807482534", 0, "Geometry");
pub(crate) const CURRENT_SKETCH_NURBS_TYPE: (&str, u32, &str) =
    ("D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B", 3, "MSketch");

const SKETCH_LINE_TYPES: [(&str, u32, &str); 6] = [
    CURRENT_SKETCH_LINE_TYPE,
    ("DCA267ED-D615-4934-B64F-AD805E8003E2", 1, "Geometry"),
    ("EA3B930A-3383-4AD3-BE25-4B2814EA3985", 0, "Geometry"),
    ("AE42BAB6-643F-4169-A33C-529C8E0A4D84", 0, "Geometry"),
    ("F279874A-17AB-43DA-BF8E-80259802D06E", 0, "Geometry"),
    ("58751243-FEA8-41E6-BBC9-37960EB8164B", 0, "Geometry"),
];
const SKETCH_CIRCULAR_TYPES: [(&str, u32, &str); 2] = [
    CURRENT_SKETCH_CIRCULAR_TYPE,
    ("FF23079A-D99C-47AB-940E-2F4E18F022AB", 2, "Geometry"),
];
const SKETCH_TEXT_FRAME_LINE_TYPE_GUID: &str = "16DEFC4D-1816-4FB0-8E39-9BDA23954248";

/// Geometry grammar selected by a sketch curve's stable type identity and
/// record version. Dynamic class tags are segment-local table ordinals and do
/// not identify a family without this resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SketchCurveClass {
    Line,
    Circular,
    Nurbs,
    TextFrameLine,
}

impl SketchCurveClass {
    fn of(type_guid: &str, version: u32, module: &str) -> Option<Self> {
        let matches = |(known_guid, known_version, known_module): &(&str, u32, &str)| {
            version == *known_version
                && module == *known_module
                && type_guid.eq_ignore_ascii_case(known_guid)
        };
        if SKETCH_LINE_TYPES.iter().any(matches) {
            Some(Self::Line)
        } else if SKETCH_CIRCULAR_TYPES.iter().any(matches) {
            Some(Self::Circular)
        } else if type_guid.eq_ignore_ascii_case(CURRENT_SKETCH_NURBS_TYPE.0)
            && version == CURRENT_SKETCH_NURBS_TYPE.1
            && module == CURRENT_SKETCH_NURBS_TYPE.2
        {
            Some(Self::Nurbs)
        } else if type_guid.eq_ignore_ascii_case(SKETCH_TEXT_FRAME_LINE_TYPE_GUID)
            && version == 0
            && module == "MSketch"
        {
            Some(Self::TextFrameLine)
        } else {
            None
        }
    }
}

/// Decode every sketch-curve record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `crv_primary_id`/
/// `crv_secondary_id`) from each design `BulkStream` entry in `scan`: the
/// curve's persistent primary and secondary identities plus its NURBS, circular
/// arc, line, or referenced analytic geometry.
pub(crate) fn decode_sketch_curve_identities_from_stream(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    let mut out = Vec::new();
    for frame in design_primary_frames(bytes, meta)? {
        let payload = &bytes[frame.start..frame.end];
        let Some((primary_id, secondary_id, geometry_shift, entity_genesis)) =
            decode_sketch_curve_identity(payload)
        else {
            continue;
        };
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-curve entity ID exceeds u32".into()))?;
        let curve_class = SketchCurveClass::of(
            &frame.design_type.type_guid,
            frame.design_type.version,
            &frame.design_type.module,
        );
        let parsed_geometry = curve_class.and_then(|curve_class| {
            decode_sketch_curve_geometry(payload, geometry_shift, record_index, curve_class)
        });
        let (geometry, geometry_offset) = parsed_geometry
            .map_or((None, geometry_shift + 133), |parsed| {
                (Some(parsed.geometry), parsed.geometry_offset)
            });
        out.push(SketchCurveIdentity {
            id: ids::native_sketch_curve_identity_id(stream, frame.start),
            record_index,
            owner_reference: trailing_sketch_owner_reference(payload),
            class_tag: frame.class_tag.to_string(),
            byte_offset: frame.start as u64,
            geometry_offset: geometry_offset as u32,
            entity_genesis,
            primary_id,
            secondary_id,
            geometry,
        });
    }
    Ok(out)
}

/// Decode every sketch-curve record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `crv_primary_id`/
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
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        out.extend(decode_sketch_curve_identities_from_stream(
            bytes,
            &meta,
            &entry.name,
        )?);
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
        || View::u32_le_at(payload, 21) != Some(2)
        || View::u32_le_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || View::u32_le_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
        || View::u32_le_at(payload, 77) != Some(11)
        || payload.get(81..92) != Some(b"surface_tag")
        || View::u32_le_at(payload, 92) != Some(23)
        || payload.get(96..119) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = View::u64_le_at(payload, 69);
    let persistent_id = View::u64_le_at(payload, 119)?;
    let point_count = usize::try_from(View::u32_le_at(payload, 127)?).ok()?;
    if point_count == 0 || point_count > 100_000 {
        return None;
    }
    let coordinate_count = point_count.checked_mul(3)?;
    let coordinate_bytes = point_count.checked_mul(24)?;
    let coordinates = f64s_at(payload, 131, coordinate_count)?;
    let degrees_at = 131usize.checked_add(coordinate_bytes)?;
    let u_degree = View::u32_le_at(payload, degrees_at)?;
    let v_degree = View::u32_le_at(payload, degrees_at.checked_add(4)?)?;
    let u_knot_count =
        usize::try_from(View::u32_le_at(payload, degrees_at.checked_add(8)?)?).ok()?;
    let u_knots_at = degrees_at.checked_add(12)?;
    let u_knots = f64s_at(payload, u_knots_at, u_knot_count)?;
    let v_count_at = u_knots_at.checked_add(u_knot_count.checked_mul(8)?)?;
    let v_knot_count = usize::try_from(View::u32_le_at(payload, v_count_at)?).ok()?;
    let v_knots_at = v_count_at.checked_add(4)?;
    let v_knots = f64s_at(payload, v_knots_at, v_knot_count)?;
    let grid_at = v_knots_at.checked_add(v_knot_count.checked_mul(8)?)?;
    let u_count = usize::try_from(View::u32_le_at(payload, grid_at)?).ok()?;
    let v_count = usize::try_from(View::u32_le_at(payload, grid_at.checked_add(4)?)?).ok()?;
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
        || !knots_nondecreasing(&u_knots)
        || !knots_nondecreasing(&v_knots)
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
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
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
            let Some(record_index) = View::u32_le_at(bytes, after_tag) else {
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
    out.sort_by(|a, b| a.id.cmp(&b.id));
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
            CodecError::malformed(format_args!(
                "Fusion sketch relation {} has no Design stream identity",
                relation.record_index
            ))
        })?;
        relation.owner_entity_id = sketch_owners
            .get(&(scope, relation.owner_reference))
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
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
        for record_index in relation.all_member_indices() {
            if !typed_records.contains(&(scope, record_index)) {
                continue;
            }
            if owners
                .insert((scope, record_index), relation.owner_reference)
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                return Err(CodecError::malformed(format_args!(
                    "Fusion sketch record {record_index} in {scope} belongs to multiple sketches"
                )));
            }
        }
    }
    // A direct backlink, a typed relation, and the sketch container's counted
    // member run are the three independent ownership joins. Payload-internal
    // references in an otherwise unowned Geometry record do not name a Sketch.
    // Apply the container join after relations and require all joins to agree.
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
                return Err(CodecError::malformed(format_args!(
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
                    persistent_id: point.persistent_id(),
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
        let member_indices = relation
            .members
            .iter()
            .map(|member| member.record_index)
            .collect::<Vec<_>>();
        let return_indices = relation
            .return_members
            .iter()
            .map(|member| member.record_index)
            .collect::<Vec<_>>();
        let resolved_members: Vec<SketchRelationOperand> = resolve(scope, &member_indices);
        let resolved_return_members: Vec<SketchRelationOperand> = resolve(scope, &return_indices);
        for (member, resolved) in relation.members.iter_mut().zip(resolved_members) {
            member.resolved = Some(resolved);
        }
        for (member, resolved) in relation
            .return_members
            .iter_mut()
            .zip(resolved_return_members)
        {
            member.resolved = Some(resolved);
        }
    }
    Ok(())
}

fn decode_sketch_curve_identity(payload: &[u8]) -> Option<(u64, u64, usize, Option<u64>)> {
    if let Some((primary, secondary)) = decode_sketch_curve_identity_variant(payload, 0, 2) {
        return Some((primary, secondary, 0, None));
    }
    if View::u32_le_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || View::u32_le_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = View::u64_le_at(payload, 69)?;
    decode_sketch_curve_identity_variant(payload, 52, 3)
        .map(|(primary, secondary)| (primary, secondary, 52, Some(entity_genesis)))
}

fn decode_sketch_curve_identity_variant(
    payload: &[u8],
    shift: usize,
    property_count: u32,
) -> Option<(u64, u64)> {
    if payload.get(20) != Some(&1)
        || View::u32_le_at(payload, 21) != Some(property_count)
        || View::u32_le_at(payload, 25 + shift) != Some(14)
        || payload.get(29 + shift..43 + shift) != Some(b"crv_primary_id")
        || View::u32_le_at(payload, 43 + shift) != Some(23)
        || payload.get(47 + shift..70 + shift) != Some(b"IntrinsicMetaTypeuint64")
        || View::u32_le_at(payload, 78 + shift) != Some(16)
        || payload.get(82 + shift..98 + shift) != Some(b"crv_secondary_id")
        || View::u32_le_at(payload, 98 + shift) != Some(23)
        || payload.get(102 + shift..125 + shift) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    Some((
        View::u64_le_at(payload, 70 + shift)?,
        View::u64_le_at(payload, 125 + shift)?,
    ))
}

struct DecodedSketchCurveGeometry {
    geometry: SketchCurveGeometry,
    geometry_offset: usize,
}

fn decode_sketch_curve_geometry(
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
    class: SketchCurveClass,
) -> Option<DecodedSketchCurveGeometry> {
    let geometry_payload = payload.get(geometry_shift..)?;
    let decoded = match class {
        SketchCurveClass::Line => {
            if let Some((geometry, _)) = decode_line_family(geometry_payload) {
                Some((geometry, 133))
            } else {
                let referenced = referenced_analytic_payload(geometry_payload)?;
                let (geometry, _) = decode_line_family(referenced)?;
                Some((geometry, 11 + 133))
            }
        }
        SketchCurveClass::Circular => {
            if let Some(geometry) = decode_circular_arc(geometry_payload) {
                Some((geometry, 133))
            } else {
                let referenced = referenced_analytic_payload(geometry_payload)?;
                Some((decode_circular_arc(referenced)?, 11 + 133))
            }
        }
        SketchCurveClass::Nurbs => decode_legacy_sketch_nurbs(geometry_payload)
            .or_else(|| decode_sketch_nurbs(geometry_payload))
            .map(|(geometry, _)| (geometry, 133)),
        SketchCurveClass::TextFrameLine => {
            let (geometry, end) = decode_text_frame_line(payload, geometry_shift, record_index)?;
            Some((geometry, end.checked_sub(geometry_shift + 12 * 8)?))
        }
    }?;
    Some(DecodedSketchCurveGeometry {
        geometry: decoded.0,
        geometry_offset: geometry_shift + decoded.1,
    })
}

fn decode_circular_arc(payload: &[u8]) -> Option<SketchCurveGeometry> {
    let values = (0..12)
        .map(|ordinal| View::f64_le_at(payload, 133 + ordinal * 8))
        .collect::<Option<Vec<_>>>()?;
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let normal = Vector3::new(values[3], values[4], values[5]);
    let reference_direction = Vector3::new(values[6], values[7], values[8]);
    let dot = normal.x * reference_direction.x
        + normal.y * reference_direction.y
        + normal.z * reference_direction.z;
    if (normal.norm() - 1.0).abs() > EPS_SKETCH_DECODE_CIRCULAR_ARC_E9
        || (reference_direction.norm() - 1.0).abs() > EPS_SKETCH_DECODE_CIRCULAR_ARC_E9
        || dot.abs() > EPS_SKETCH_DECODE_CIRCULAR_ARC_E9
        || values[9] <= 0.0
        || values[10].abs() > std::f64::consts::TAU + EPS_SKETCH_DECODE_CIRCULAR_ARC_E9
        || values[11].abs() > std::f64::consts::TAU + EPS_SKETCH_DECODE_CIRCULAR_ARC_E9
        || (values[11] - values[10]).abs() < EPS_SKETCH_DECODE_CIRCULAR_ARC_E12
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

fn referenced_analytic_payload(payload: &[u8]) -> Option<&[u8]> {
    if payload.get(133) != Some(&1) || payload.get(138..144) != Some(&[0; 6]) {
        return None;
    }
    payload.get(11..)
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
        || View::u32_le_at(payload, after_tag) != Some(record_index)
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
    let carrier = View::u64_le_at(payload, base)?;
    let carrier_reference = (carrier != u64::MAX).then_some(carrier);
    if View::u32_le_at(payload, base + 8) != Some(3) || payload.get(base + 88) != Some(&1) {
        return None;
    }
    let subtype_class_tag = std::str::from_utf8(payload.get(base + 12..base + 15)?)
        .ok()?
        .to_string();
    if !subtype_class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let degree = View::u32_le_at(payload, base + 90)?;
    let fit_tolerance = View::f64_le_at(payload, base + 94)?;
    let knot_count = usize::try_from(View::u32_le_at(payload, base + 102)?).ok()?;
    if View::u32_le_at(payload, base + 106)? as usize != knot_count
        || View::u32_le_at(payload, base + 110)? != 8
        || knot_count > 100_000
    {
        return None;
    }
    let knots = f64s_at(payload, base + 114, knot_count)?;
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(View::u32_le_at(payload, weights_at)?).ok()?;
    if View::u32_le_at(payload, weights_at + 4)? as usize != weight_count
        || View::u32_le_at(payload, weights_at + 8)? != 8
        || weight_count > 100_000
    {
        return None;
    }
    let weights = f64s_at(payload, weights_at + 12, weight_count)?;
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(View::u32_le_at(payload, points_at)?).ok()?;
    if (weight_count != 0 && point_count != weight_count)
        || View::u32_le_at(payload, points_at + 4)? as usize != point_count
        || View::u32_le_at(payload, points_at + 8)? != 8
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
            subtype_record_index: View::u32_le_at(payload, base + 15)?,
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
    let carrier = View::u64_le_at(payload, base)?;
    let carrier_reference = (carrier != u64::MAX).then_some(carrier);
    if View::u32_le_at(payload, base + 8) != Some(3)
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
    let degree = View::u32_le_at(payload, base + 90)?;
    let fit_tolerance = View::f64_le_at(payload, base + 42)?;
    let knot_count = usize::try_from(View::u32_le_at(payload, base + 102)?).ok()?;
    let knot_capacity = usize::try_from(View::u32_le_at(payload, base + 106)?).ok()?;
    if degree == 0
        || knot_capacity < knot_count
        || View::u32_le_at(payload, base + 110)? != 8
        || knot_capacity > 100_000
    {
        return None;
    }
    let knots = f64s_at(payload, base + 114, knot_count)?;
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(View::u32_le_at(payload, weights_at)?).ok()?;
    let weight_capacity = usize::try_from(View::u32_le_at(payload, weights_at + 4)?).ok()?;
    if weight_capacity < weight_count
        || View::u32_le_at(payload, weights_at + 8)? != 8
        || weight_capacity > 100_000
    {
        return None;
    }
    let weights = f64s_at(payload, weights_at + 12, weight_count)?;
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(View::u32_le_at(payload, points_at)?).ok()?;
    let point_capacity = usize::try_from(View::u32_le_at(payload, points_at + 4)?).ok()?;
    if (weight_count != 0 && point_count != weight_count)
        || point_capacity < point_count
        || point_capacity > 100_000
        || View::u32_le_at(payload, points_at + 8)? != 8
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
            subtype_record_index: View::u32_le_at(payload, base + 15)?,
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
        .map(|ordinal| View::f64_le_at(payload, values_at + ordinal * 8))
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

fn decode_line_family(payload: &[u8]) -> Option<(SketchCurveGeometry, usize)> {
    decode_line(payload)
        .map(|geometry| (geometry, 12))
        .or_else(|| decode_compact_planar_line(payload).map(|geometry| (geometry, 9)))
}

fn decode_line_values(payload: &[u8], values_at: usize) -> Option<SketchCurveGeometry> {
    let values = (0..12)
        .map(|ordinal| View::f64_le_at(payload, values_at + ordinal * 8))
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
    let displacement_direction = displacement.scale(1.0 / length);
    if (direction.norm() - 1.0).abs() > EPS_SKETCH_DECODE_LINE_COMPONENTS_E9
        || (stored_normal.norm() - 1.0).abs() > EPS_SKETCH_DECODE_LINE_COMPONENTS_E9
    {
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
    let dot = direction.dot(stored_normal);
    let projected_normal = stored_normal - direction.scale(dot);
    let projected_length = projected_normal.norm();
    let normal = if projected_length.is_finite()
        && projected_length > EPS_SKETCH_DECODE_LINE_COMPONENTS_E12
    {
        projected_normal.scale(1.0 / projected_length)
    } else {
        // Spatial line carriers can store a unit auxiliary vector parallel to
        // the line. The neutral spatial-line geometry has no plane normal;
        // retain the bounded carrier and choose a stable perpendicular basis
        // vector so the native geometry record still satisfies its typed
        // invariant.
        let basis = [
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ]
        .into_iter()
        .min_by(|left, right| {
            direction
                .dot(*left)
                .abs()
                .total_cmp(&direction.dot(*right).abs())
        })?;
        direction.cross(basis).unit()?
    };
    let start = Point3::new(values[0] * 10.0, values[1] * 10.0, values[2] * 10.0);
    Some(SketchCurveGeometry::Line {
        start,
        end: start.translated(displacement, 10.0),
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
    /// Serialized cardinality of the rectangular class's counted reference
    /// run. `None` for every other relation class.
    pub(crate) rectangular_reference_count: Option<u32>,
    /// Position within `auxiliary_references` of the first direction clause's
    /// count-parameter reference on a rectangular pattern whose four clause
    /// references are all present. `None` for every other relation class and
    /// when a rectangular clause reference is absent.
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

/// Serialized width selected by a sketch relation's leading-block member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SketchRelationMaskWidth {
    U32,
    U64,
}

impl SketchRelationMaskWidth {
    fn from_leading_block(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::U32),
            1 => Some(Self::U64),
            _ => None,
        }
    }

    fn has_paired_member_run(self) -> bool {
        self == Self::U64
    }
}

/// Read the relation's mask width from its leading-block presence member.
pub(crate) fn relation_mask_width(record: &[u8]) -> Option<SketchRelationMaskWidth> {
    let (_, start) = lp_ascii_filtered(record, 15, 0..=256, u8::is_ascii_graphic)?;
    SketchRelationMaskWidth::from_leading_block(*record.get(start)?)
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
    let entries = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
    if entries > MAX_RELATION_RUN {
        return None;
    }
    *cursor += 4;
    for _ in 0..entries {
        let values = usize::try_from(View::u32_le_at(payload, *cursor + 8)?).ok()?;
        if values > MAX_RELATION_RUN {
            return None;
        }
        *cursor += 12 + values * 8;
    }
    let ordinals = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
    if ordinals > MAX_RELATION_RUN {
        return None;
    }
    *cursor += 4 + ordinals * 4;
    (*cursor <= payload.len()).then_some(())
}

/// What a sketch-relation subclass leaves behind after its own members.
struct RelationClassMembers {
    rectangular_reference_count: Option<u32>,
    rectangular_clause_ordinal: Option<usize>,
    text_glyph_transforms: Option<Vec<[[f64; 4]; 4]>>,
}

/// Consume the members `class` writes between the property block and the base
/// class's `ParentNode`, advancing `cursor` past them and recording every
/// present reference among them in the auxiliary run
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
fn parse_relation_class_members(
    payload: &[u8],
    cursor: &mut usize,
    class: SketchRelationClass,
    auxiliary_references: &mut Vec<u32>,
    auxiliary_reference_offsets: &mut Vec<usize>,
) -> Option<RelationClassMembers> {
    let mut members = RelationClassMembers {
        rectangular_reference_count: None,
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
            let reference_count = View::u32_le_at(payload, *cursor)?;
            let references = usize::try_from(reference_count).ok()?;
            if references > MAX_RELATION_RUN {
                return None;
            }
            *cursor += 4;
            for _ in 0..references {
                take!()?;
            }
            members.rectangular_reference_count = Some(reference_count);
            skip_pattern_tables(payload, cursor)?;
            let clause_ordinal = auxiliary_reference_offsets.len();
            let mut complete = true;
            for _ in 0..2 {
                // The evaluated instance count precedes the count-parameter
                // reference; the unit direction and source distance follow it,
                // and the distance parameter closes the clause.
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

/// Parse one sketch-relation record body whose class is known
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
///
/// The payload is `u8 1`, a u32 count and that many `(reference, u32 relation
/// ordinal)` pairs, the property-block presence byte and its block, the
/// class-defined members, the `ParentNode` reference naming the owning sketch,
/// a u64 constraint mask, a u32 count and that many bare references, and one
/// zero byte. At relation base-class version 0 the leading byte is zero, the
/// pair list is absent, and the mask is a u32. Both reference runs hold the
/// same members; only the second is in semantic order.
pub(crate) fn parse_classed_sketch_relation(
    payload: &[u8],
    class: SketchRelationClass,
) -> Option<ParsedSketchRelation> {
    parse_relation(payload, class)
}

fn parse_relation(payload: &[u8], class: SketchRelationClass) -> Option<ParsedSketchRelation> {
    // The record header is the LP-ASCII class tag, the u64 entity id, and the
    // LP-ASCII record name; the member payload follows it.
    let (_, start) = lp_ascii_filtered(payload, 15, 0..=256, u8::is_ascii_graphic)?;
    let mask_width = SketchRelationMaskWidth::from_leading_block(*payload.get(start)?)?;
    let paired_run = mask_width.has_paired_member_run();
    let mut cursor = start + 1;
    let mut members = Vec::new();
    let mut member_offsets = Vec::new();
    let mut member_relation_ordinals = Vec::new();
    if paired_run {
        let member_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
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
            member_relation_ordinals.push(View::u32_le_at(payload, cursor)?);
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
    let class_members = parse_relation_class_members(
        payload,
        &mut cursor,
        class,
        &mut auxiliary_references,
        &mut auxiliary_reference_offsets,
    )?;
    let rectangular_reference_count = class_members.rectangular_reference_count;
    let rectangular_clause_ordinal = class_members.rectangular_clause_ordinal;
    let text_glyph_transforms = class_members.text_glyph_transforms;
    let (owner_reference, owner_reference_offset) = take_relation_reference(payload, &mut cursor)?;
    let state_offset = cursor;
    // The constraint mask follows `ParentNode` directly. It is a u64 in the
    // paired-run form and a u32 at relation base-class version 0.
    let (state, mut cursor) = match mask_width {
        SketchRelationMaskWidth::U64 => (View::u64_le_at(payload, state_offset)?, state_offset + 8),
        SketchRelationMaskWidth::U32 => (
            u64::from(View::u32_le_at(payload, state_offset)?),
            state_offset + 4,
        ),
    };
    let return_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
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
        rectangular_reference_count,
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
    let mut view = View::over_retained(payload);
    view.seek(end)?;
    if view.take(6)? != [0u8; 6] {
        return None;
    }
    let count = usize::try_from(view.u32_le()?).ok()?;
    if !(1..=4096).contains(&count) {
        return None;
    }
    let mut transforms = Vec::with_capacity(count);
    for _ in 0..count {
        if view.u32_le()? != 16 {
            return None;
        }
        let mut transform = [[0.0; 4]; 4];
        for row in &mut transform {
            for cell in row {
                let value = view.f64_le()?;
                if !value.is_finite() {
                    return None;
                }
                *cell = value;
            }
        }
        transforms.push(transform);
    }
    Some((text_reference, transforms, view.position()))
}

/// Whether an indexed-record header starts at `at`: a u32 length prefix of
/// three, a three-digit ASCII class tag, and a u32 record index. The eleven
/// bytes must be present.
fn indexed_record_header_at(bytes: &[u8], at: usize) -> bool {
    View::u32_le_at(bytes, at) == Some(3)
        && bytes
            .get(at + 4..at + 7)
            .is_some_and(|tag| tag.iter().all(u8::is_ascii_digit))
        && bytes.get(at + 7..at + 11).is_some()
}

/// The record index carried by the indexed-record header at `at`. The header
/// spends its first seven bytes on the length-prefixed class tag, so the index
/// always sits at `at + 7`.
pub(crate) fn indexed_record_index(bytes: &[u8], at: usize) -> Option<u32> {
    View::u32_le_at(bytes, at.checked_add(7)?)
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
    (bytes.get(position) == Some(&1))
        .then_some((View::u32_le_at(bytes, position + 1)?, position + 5))
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
    let mut view = View::over_retained(bytes);
    view.seek(position)?;
    let low = view.u32_le()?;
    let high = view.u32_le()?;
    let record_reference = match (low, high) {
        (u32::MAX, u32::MAX) => None,
        (reference, 0) => Some(reference),
        _ => return None,
    };
    if view.u8()? != 1 {
        return None;
    }
    let declared_count = view.u32_le()?;
    let mut references = Vec::new();
    let mut reference_offsets = Vec::new();
    loop {
        let mut probe = view;
        if probe.u8() != Some(1) {
            break;
        }
        let offset = probe.position();
        let Some(reference) = probe.u32_le() else {
            break;
        };
        if probe.take(6) != Some(&[0; 6]) {
            break;
        }
        reference_offsets.push(offset);
        references.push(reference);
        view = probe;
    }
    (references.len() == declared_count as usize).then_some(SketchReferenceList {
        record_reference,
        record_reference_offset: position,
        declared_count,
        references,
        reference_offsets,
        end: view.position(),
    })
}

#[cfg(test)]
mod tests;
