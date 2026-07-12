// SPDX-License-Identifier: Apache-2.0
//! Decode Fusion Design object, sketch, identity, and construction records.
//!
//! These functions read Design `MetaStream.dat` and `BulkStream.dat` entries
//! selected by [`crate::container`]. Returned records retain source offsets and
//! stable identifiers for native regeneration.

use std::collections::HashMap;

use cadmpeg_ir::codec::{CodecError, ReadSeek};
use cadmpeg_ir::design::{
    ConstructionRecipe, ConstructionRecipeKind, DesignBodyMember, DesignEntityHeader, DesignObject,
    DesignObjectKind, DesignRecordHeader, LostEdgeReference, PersistentReference,
    PersistentReferenceKind, SketchConstraintKind, SketchCurveGeometry, SketchCurveIdentity,
    SketchPoint, SketchRelation,
};
use cadmpeg_ir::le::{f64_at, lp_u32_bytes_at, u32_at, utf16le_at};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::container::{role, ContainerScan};

const RECIPES: &[(&[u8], ConstructionRecipeKind)] = &[
    (b"body_recipe_data", ConstructionRecipeKind::Body),
    (b"face_recipe_data", ConstructionRecipeKind::Face),
    (
        b"bounded_face_recipe_data",
        ConstructionRecipeKind::BoundedFace,
    ),
    (b"edge_recipe_data", ConstructionRecipeKind::Edge),
    (b"vertex_recipe_data", ConstructionRecipeKind::Vertex),
];

/// Decode every parametric construction-recipe record (`body_recipe_data`,
/// `face_recipe_data`, `bounded_face_recipe_data`, `edge_recipe_data`,
/// `vertex_recipe_data`) from each design `BulkStream` entry in `scan`.
/// `recipe_index` is assigned per `(kind, design_id)` group in stream order.
pub fn decode_recipes(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<ConstructionRecipe>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_stream(bytes, &entry.name, &mut out);
    }
    Ok(out)
}

/// Decode the persistent u64 point and curve identity references
/// (`pt_tag`, `crv_primary_id`, `crv_secondary_id`, each typed
/// `IntrinsicMetaTypeuint64`) from every design `BulkStream` entry in `scan`,
/// sorted by stream offset.
pub fn decode_persistent_references(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<PersistentReference>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
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
                let type_offset = offset + name.len();
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
                out.push(PersistentReference {
                    id: format!("f3d:{}:persistent-reference#{offset}", entry.name),
                    byte_offset: offset as u64,
                    value_offset: (value_offset - offset) as u32,
                    kind,
                    value: u64::from_le_bytes(raw.try_into().expect(
                        "invariant: raw is an 8-byte slice from bytes.get(range) of length 8",
                    )),
                });
            }
        }
    }
    out.sort_by_key(|reference| reference.value);
    Ok(out)
}

/// Decode every `EDGE_REFERENCE_LOST` marker record from each design
/// `BulkStream` entry in `scan`: the ASCII literal, a `u32` length of `3`, a
/// three-digit class tag, and a `u32` record index.
pub fn decode_lost_edge_references(
    _reader: &mut dyn ReadSeek,
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
            let payload = offset + marker.len();
            let Some(length) = bytes.get(payload..payload + 4) else {
                continue;
            };
            if u32::from_le_bytes(
                length.try_into().expect(
                    "invariant: length is a 4-byte slice from bytes.get(range) of length 4",
                ),
            ) != 3
            {
                continue;
            }
            let Some(class_tag) = bytes.get(payload + 4..payload + 7) else {
                continue;
            };
            if !class_tag.iter().all(u8::is_ascii_digit) {
                continue;
            }
            let Some(index) = bytes.get(payload + 7..payload + 11) else {
                continue;
            };
            out.push(LostEdgeReference {
                id: format!("f3d:{}:lost-edge-reference#{offset}", entry.name),
                byte_offset: offset as u64,
                class_tag_offset: (payload + 4) as u64,
                class_tag: String::from_utf8_lossy(class_tag).into_owned(),
                record_index: u32::from_le_bytes(index.try_into().expect(
                    "invariant: index is a 4-byte slice from bytes.get(range) of length 4",
                )),
                record_index_offset: (payload + 7) as u64,
            });
        }
    }
    Ok(out)
}

/// Decode every GUID-owned design object record from each design
/// `MetaStream` entry in `scan` ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)): an ASCII type name, the design
/// entity IDs it owns, its self GUID, an optional parent GUID, and a
/// revision. Records whose type name does not match a known
/// [`DesignObjectKind`] are skipped.
pub fn decode_objects(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignObject>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::METASTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut offset = 0usize;
        while offset + 8 <= bytes.len() {
            let Some((name, after_name)) = lp_ascii(bytes, offset) else {
                offset += 1;
                continue;
            };
            let Some(kind) = object_kind(&name) else {
                offset += 1;
                continue;
            };
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
                lp_ascii(bytes, ids_end).filter(|(guid, _)| is_guid(guid))
            else {
                offset += 1;
                continue;
            };
            let mut tail = after_self;
            while bytes.get(tail) == Some(&0) {
                tail += 1;
            }
            let (parent_guid, parent_guid_offset, revision_offset) = lp_ascii(bytes, tail)
                .filter(|(guid, _)| is_guid(guid))
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
                id: format!("f3d:{}:design-object#{offset}", entry.name),
                byte_offset: offset as u64,
                kind,
                entity_ids,
                entity_id_offsets,
                self_guid,
                self_guid_offset: (ids_end + 4) as u64,
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

/// Decode every self-validating per-entity design `BulkStream` header (spec
/// [§8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)): a three-digit class tag, an entity suffix, a UTF-16LE entity ID
/// whose numeric suffix must match the header's entity suffix, and, for
/// sketch-typed entities, the trailing reference-list header.
pub fn decode_entity_headers(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignEntityHeader>, CodecError> {
    let mut out = Vec::new();
    let mut object_kinds = HashMap::new();
    for object in decode_objects(reader, scan)? {
        for entity_id in object.entity_ids {
            object_kinds.entry(entity_id).or_insert(object.kind);
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
            let Some(entity_raw) = bytes.get(start + 7..start + 15) else {
                break;
            };
            let entity_suffix = u64::from_le_bytes(entity_raw.try_into().expect(
                "invariant: entity_raw is an 8-byte slice from bytes.get(range) of length 8",
            ));
            if entity_suffix == 0
                || entity_suffix >= 1 << 32
                || bytes.get(start + 15..start + 20) != Some(&[0u8; 5])
            {
                continue;
            }
            let (optional_slot_present, string_offset) = match bytes[start + 20] {
                0 => (false, start + 21),
                1 if bytes.get(start + 21..start + 25) == Some(&[0u8; 4]) => (true, start + 25),
                _ => continue,
            };
            let Some((entity_id, end)) = lp_utf16(bytes, string_offset) else {
                continue;
            };
            let Some((_, suffix)) = entity_id.rsplit_once('_') else {
                continue;
            };
            if suffix.parse::<u64>().ok() != Some(entity_suffix) {
                continue;
            }
            let object_kind = object_kinds.get(&entity_suffix).copied();
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
                            Some(list.record_reference),
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
            out.push(DesignEntityHeader {
                id: format!("f3d:{}:design-entity-header#{start}", entry.name),
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
            });
            offset = record_end;
        }
    }
    Ok(out)
}

/// Decode the indexed dynamic-class record headers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)) that `entities`'
/// reference-list entries point at: a `u32` record index and a three-digit
/// class tag, for each record index named by any [`DesignEntityHeader`] in
/// `entities`.
pub fn decode_record_headers(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let wanted = entities
        .iter()
        .flat_map(|entity| &entity.reference_indices)
        .copied()
        .collect::<std::collections::HashSet<_>>();
    decode_headers_for_indices(reader, scan, &wanted)
}

/// Decode the indexed dynamic-class record headers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)) named by
/// `indices` directly, bypassing entity reference lists. Used to fetch record
/// headers referenced by records other than [`DesignEntityHeader`] (for
/// example, sketch relation records).
pub fn decode_related_record_headers(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    indices: &[u32],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let wanted = indices
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    decode_headers_for_indices(reader, scan, &wanted)
}

fn decode_headers_for_indices(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    wanted: &std::collections::HashSet<u32>,
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut emitted = std::collections::HashSet::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while position + 11 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(bytes, position) else {
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
            if wanted.contains(&record_index) && emitted.insert(record_index) {
                out.push(DesignRecordHeader {
                    id: format!("f3d:{}:design-record-header#{position}", entry.name),
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
    out.sort_by_key(|record| record.record_index);
    Ok(out)
}

/// Decode the sketch-relation body at each `records` entry's offset: the
/// owning sketch relation's member reference list, owner reference, state,
/// and return-member list. `records` supplies the byte offsets and class tags
/// (typically from [`decode_related_record_headers`]).
pub fn decode_sketch_relations(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    records: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
) -> Result<Vec<SketchRelation>, CodecError> {
    let mut out = Vec::new();
    let owners = entities
        .iter()
        .filter(|entity| entity.object_kind == Some(DesignObjectKind::Sketch))
        .map(|entity| entity.entity_suffix as u32)
        .collect::<std::collections::HashSet<_>>();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        for record in records {
            let Ok(at) = usize::try_from(record.byte_offset) else {
                continue;
            };
            let Some(payload) = bytes.get(at..at + 101) else {
                continue;
            };
            let Some((
                members,
                auxiliary_references,
                owner_reference,
                state,
                state_offset,
                return_members,
            )) = parse_sketch_relation(payload, &owners)
            else {
                continue;
            };
            let (constraint_kinds, unknown_constraint_bits) = decode_constraint_kinds(state);
            out.push(SketchRelation {
                id: format!("f3d:{}:sketch-relation#{}", entry.name, record.record_index),
                record_index: record.record_index,
                class_tag: record.class_tag.clone(),
                byte_offset: record.byte_offset,
                state_offset: state_offset as u32,
                owner_reference,
                auxiliary_references,
                members,
                state,
                constraint_kinds,
                unknown_constraint_bits,
                return_members,
                raw_bytes: payload.to_vec(),
            });
        }
    }
    Ok(out)
}

pub(crate) fn decode_constraint_kinds(state: u32) -> (Vec<SketchConstraintKind>, u32) {
    let definitions = [
        (0x0000_0001, SketchConstraintKind::Coincident),
        (0x0000_0002, SketchConstraintKind::Colinear),
        (0x0000_0004, SketchConstraintKind::Concentric),
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
    ];
    let mut kinds = Vec::new();
    let mut recognized = 0u32;
    for (bit, kind) in definitions {
        if state & bit != 0 {
            kinds.push(kind);
            recognized |= bit;
        }
    }
    (kinds, state & !recognized)
}

/// Decode every sketch-point record ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata), `pt_tag`) from each design
/// `BulkStream` entry in `scan`: the persistent point id, a paired record
/// reference, and the sketch `(u, v)` coordinates, converted centimetre→
/// millimetre. Records whose scaled coordinates are non-finite are skipped.
pub fn decode_sketch_points(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<SketchPoint>, CodecError> {
    let mut out = Vec::new();
    let mut emitted = std::collections::HashSet::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while at + 112 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(bytes, at) else {
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
            let Some((persistent_id, paired_reference, x, y, shift)) = decode_sketch_point(payload)
            else {
                at += 1;
                continue;
            };
            let (u, v) = (x * 10.0, y * 10.0);
            if !u.is_finite() || !v.is_finite() {
                at += 1;
                continue;
            }
            if emitted.insert(record_index) {
                out.push(SketchPoint {
                    id: format!("f3d:{}:sketch-point#{at}", entry.name),
                    record_index,
                    class_tag,
                    byte_offset: at as u64,
                    coordinate_offset: (89 + shift) as u32,
                    persistent_id,
                    paired_reference,
                    coordinates: Point2::new(u, v),
                    raw_bytes: payload[..112 + shift].to_vec(),
                });
            }
            at += 112;
        }
    }
    Ok(out)
}

fn decode_sketch_point(payload: &[u8]) -> Option<(u64, u32, f64, f64, usize)> {
    if let Some(point) = decode_sketch_point_variant(payload, 0, 1) {
        return Some((point.0, point.1, point.2, point.3, 0));
    }
    if u32_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || u32_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    decode_sketch_point_variant(payload, 52, 2)
        .map(|point| (point.0, point.1, point.2, point.3, 52))
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
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    let mut out = Vec::new();
    let mut emitted = std::collections::HashSet::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while at + 133 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(bytes, at) else {
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
            let Some((primary_id, secondary_id, geometry_shift)) =
                decode_sketch_curve_identity(payload)
            else {
                at += 1;
                continue;
            };
            if emitted.insert(record_index) {
                let geometry_payload = payload
                    .get(geometry_shift..)
                    .expect("invariant: geometry_shift (0 or 52) is <= payload.len() (checked >= 133 by the at + 133 <= bytes.len() loop guard)");
                out.push(SketchCurveIdentity {
                    id: format!("f3d:{}:sketch-curve-identity#{at}", entry.name),
                    record_index,
                    class_tag,
                    byte_offset: at as u64,
                    geometry_offset: (133 + geometry_shift) as u32,
                    primary_id,
                    secondary_id,
                    geometry: decode_sketch_nurbs(geometry_payload)
                        .or_else(|| decode_circular_arc(geometry_payload))
                        .or_else(|| decode_line(geometry_payload))
                        .or_else(|| decode_referenced_analytic(geometry_payload)),
                });
            }
            at += 133;
        }
    }
    Ok(out)
}

fn decode_sketch_curve_identity(payload: &[u8]) -> Option<(u64, u64, usize)> {
    if let Some((primary, secondary)) = decode_sketch_curve_identity_variant(payload, 0, 2) {
        return Some((primary, secondary, 0));
    }
    if u32_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || u32_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    decode_sketch_curve_identity_variant(payload, 52, 3)
        .map(|(primary, secondary)| (primary, secondary, 52))
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

fn decode_referenced_analytic(payload: &[u8]) -> Option<SketchCurveGeometry> {
    if payload.get(133) != Some(&1) || payload.get(138..144) != Some(&[0; 6]) {
        return None;
    }
    let shifted = payload.get(11..)?;
    decode_circular_arc(shifted).or_else(|| decode_line(shifted))
}

fn decode_sketch_nurbs(payload: &[u8]) -> Option<SketchCurveGeometry> {
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
    let knots = read_f64s(payload, base + 114, knot_count)?;
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(u32_at(payload, weights_at)?).ok()?;
    if u32_at(payload, weights_at + 4)? as usize != weight_count
        || u32_at(payload, weights_at + 8)? != 8
        || weight_count > 100_000
    {
        return None;
    }
    let weights = read_f64s(payload, weights_at + 12, weight_count)?;
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(u32_at(payload, points_at)?).ok()?;
    if (weight_count != 0 && point_count != weight_count)
        || u32_at(payload, points_at + 4)? as usize != point_count
        || u32_at(payload, points_at + 8)? != 8
        || knot_count != point_count.checked_add(degree as usize + 1)?
    {
        return None;
    }
    let coordinates = read_f64s(payload, points_at + 12, point_count.checked_mul(3)?)?;
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
    Some(SketchCurveGeometry::Nurbs {
        carrier_reference,
        subtype_class_tag,
        subtype_record_index: u32_at(payload, base + 15)?,
        degree,
        fit_tolerance: fit_tolerance * 10.0,
        scalar_width: 8,
        knots,
        weights,
        control_points,
    })
}

fn read_f64s(bytes: &[u8], position: usize, count: usize) -> Option<Vec<f64>> {
    (0..count)
        .map(|ordinal| f64_at(bytes, position + ordinal * 8))
        .collect()
}

fn decode_line(payload: &[u8]) -> Option<SketchCurveGeometry> {
    let values = (0..12)
        .map(|ordinal| f64_at(payload, 133 + ordinal * 8))
        .collect::<Option<Vec<_>>>()?;
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let displacement = Vector3::new(values[3], values[4], values[5]);
    let direction = Vector3::new(values[6], values[7], values[8]);
    let normal = Vector3::new(values[9], values[10], values[11]);
    let length = displacement.norm();
    if length <= 0.0 {
        return None;
    }
    let parallel_error = Vector3::new(
        displacement.x / length - direction.x,
        displacement.y / length - direction.y,
        displacement.z / length - direction.z,
    )
    .norm();
    let dot = direction.x * normal.x + direction.y * normal.y + direction.z * normal.z;
    if (direction.norm() - 1.0).abs() > 1.0e-9
        || (normal.norm() - 1.0).abs() > 1.0e-9
        || parallel_error > 1.0e-9
        || dot.abs() > 1.0e-9
    {
        return None;
    }
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

type ParsedSketchRelation = (Vec<u32>, Vec<u32>, u32, u32, usize, Vec<u32>);

fn parse_sketch_relation(
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
    for _ in 0..member_count {
        let (value, end) = marked_u32(payload, cursor)?;
        members.push(value);
        cursor = next_reference_marker(payload, end)?;
    }
    let mut auxiliary_references = Vec::new();
    let (owner_reference, end) = loop {
        let (reference, end) = marked_u32(payload, cursor)?;
        if owners.contains(&reference) {
            break (reference, end);
        }
        auxiliary_references.push(reference);
        cursor = next_reference_marker(payload, end)?;
    };
    cursor = next_nonzero(payload, end)?;
    let state_offset = cursor + usize::from(payload.get(cursor) == Some(&1));
    let (state, end) = if payload.get(cursor) == Some(&1) {
        marked_u32(payload, cursor)?
    } else {
        (u32_at(payload, cursor)?, cursor + 4)
    };
    cursor = next_nonzero(payload, end)?;
    let return_count = usize::try_from(u32_at(payload, cursor)?).ok()?;
    if return_count > 64 {
        return None;
    }
    cursor += 4;
    let mut return_members = Vec::with_capacity(return_count);
    for ordinal in 0..return_count {
        cursor = next_reference_marker(payload, cursor)?;
        let (value, end) = marked_u32(payload, cursor)?;
        return_members.push(value);
        cursor = end;
        if ordinal + 1 < return_count {
            cursor = next_reference_marker(payload, cursor)?;
        }
    }
    Some((
        members,
        auxiliary_references,
        owner_reference,
        state,
        state_offset,
        return_members,
    ))
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
    record_reference: u32,
    record_reference_offset: usize,
    declared_count: u32,
    references: Vec<u32>,
    reference_offsets: Vec<usize>,
    end: usize,
}

fn decode_reference_list(bytes: &[u8], position: usize) -> Option<SketchReferenceList> {
    let record_reference = u32::from_le_bytes(bytes.get(position..position + 4)?.try_into().ok()?);
    if bytes.get(position + 4..position + 8) != Some(&[0; 4]) || bytes.get(position + 8) != Some(&1)
    {
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

/// Decode the `BodiesRoot` member list following the doubled `BodiesRoot`
/// marker in each design `BulkStream` entry in `scan`: each member's entity
/// suffix and flags. The decode is rejected (no members returned for that
/// stream) unless the declared count is fully consumed and immediately
/// followed by a zero byte.
pub fn decode_body_members(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignBodyMember>, CodecError> {
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
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
        else {
            continue;
        };
        let count_offset = start + prefix.len();
        let Some(count_raw) = bytes.get(count_offset..count_offset + 4) else {
            continue;
        };
        let count =
            usize::try_from(u32::from_le_bytes(count_raw.try_into().expect(
                "invariant: count_raw is a 4-byte slice from bytes.get(range) of length 4",
            )))
            .unwrap_or(usize::MAX);
        if count > 100_000 {
            continue;
        }
        let mut cursor = count_offset + 4;
        let mut decoded = Vec::with_capacity(count);
        for _ in 0..count {
            if bytes.get(cursor) != Some(&1) {
                decoded.clear();
                break;
            }
            let Some(id_raw) = bytes.get(cursor + 1..cursor + 9) else {
                decoded.clear();
                break;
            };
            let Some(flags_raw) = bytes.get(cursor + 9..cursor + 11) else {
                decoded.clear();
                break;
            };
            decoded.push(DesignBodyMember {
                id: format!("f3d:{}:design-body-member#{cursor}", entry.name),
                byte_offset: cursor as u64,
                entity_suffix: u64::from_le_bytes(id_raw.try_into().expect(
                    "invariant: id_raw is an 8-byte slice from bytes.get(range) of length 8",
                )),
                flags: u16::from_le_bytes(flags_raw.try_into().expect(
                    "invariant: flags_raw is a 2-byte slice from bytes.get(range) of length 2",
                )),
            });
            cursor += 11;
        }
        if decoded.len() == count && bytes.get(cursor) == Some(&0) {
            out.extend(decoded);
        }
    }
    Ok(out)
}

fn object_kind(name: &str) -> Option<DesignObjectKind> {
    match name {
        "Fusion" => Some(DesignObjectKind::Fusion),
        "Body" => Some(DesignObjectKind::Body),
        "Component" => Some(DesignObjectKind::Component),
        "Geometry" => Some(DesignObjectKind::Geometry),
        "MSketch" => Some(DesignObjectKind::Sketch),
        "Dimension" => Some(DesignObjectKind::Dimension),
        "Scene" => Some(DesignObjectKind::Scene),
        "EntityTracking" => Some(DesignObjectKind::EntityTracking),
        "CommonData" => Some(DesignObjectKind::CommonData),
        _ => None,
    }
}

fn lp_ascii(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32_at(bytes, offset)?).ok()?;
    if length > 2_000 {
        return None;
    }
    let (raw, end) = lp_u32_bytes_at(bytes, offset)?;
    raw.iter()
        .all(u8::is_ascii_graphic)
        .then(|| (String::from_utf8_lossy(raw).into_owned(), end))
}

fn lp_utf16(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32_at(bytes, offset)?).ok()?;
    if !(1..=256).contains(&length) {
        return None;
    }
    utf16le_at(bytes, offset.checked_add(4)?, length)
}

fn is_guid(value: &str) -> bool {
    matches!(value.len(), 36..=38)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn decode_stream(bytes: &[u8], stream: &str, out: &mut Vec<ConstructionRecipe>) {
    let mut counters: HashMap<(ConstructionRecipeKind, Option<String>), u32> = HashMap::new();
    for &(name, kind) in RECIPES {
        let mut cursor = 0;
        while let Some(relative) = bytes[cursor..].windows(name.len()).position(|w| w == name) {
            let offset = cursor + relative;
            cursor = offset + 1;
            if kind == ConstructionRecipeKind::Face
                && offset >= 8
                && &bytes[offset - 8..offset] == b"bounded_"
            {
                continue;
            }
            let design_id_field = recipe_design_id(bytes, offset, name);
            let design_id = design_id_field.as_ref().map(|field| field.0.clone());
            let key = (kind, design_id.clone());
            let counter = counters.entry(key).or_default();
            let recipe_index = *counter;
            *counter += 1;
            let record_index_offset = offset.checked_sub(16);
            let record_index = record_index_offset
                .and_then(|at| bytes.get(at..at + 4))
                .map(|raw| {
                    i32::from_le_bytes(
                        raw.try_into()
                            .expect("invariant: bytes.get(at..at+4) is a 4-byte slice"),
                    )
                })
                .unwrap_or_default();
            out.push(ConstructionRecipe {
                id: format!("f3d:{stream}:construction-recipe#{offset}"),
                byte_offset: offset as u64,
                record_index_offset: record_index_offset.map(|offset| offset as u64),
                kind,
                design_id,
                design_id_offset: design_id_field.as_ref().map(|field| field.1 as u64),
                design_id_binary_u32: design_id_field.is_some_and(|field| field.2),
                recipe_index,
                record_index,
            });
        }
    }
    out.sort_by_key(|recipe| recipe.record_index);
}

fn recipe_design_id(bytes: &[u8], offset: usize, name: &[u8]) -> Option<(String, usize, bool)> {
    let pre = offset.checked_sub(27)?;
    if let Some((id, value_offset)) = ascii_id_at(bytes, pre) {
        return Some((id, value_offset, false));
    }
    if offset >= 23 {
        let candidate = bytes.get(offset - 23..offset - 20)?;
        if candidate.iter().all(u8::is_ascii_digit) {
            return Some((
                String::from_utf8_lossy(candidate).into_owned(),
                offset - 23,
                false,
            ));
        }
    }
    if name == b"bounded_face_recipe_data" && offset >= 16 {
        let id = u32::from_le_bytes(bytes[offset - 16..offset - 12].try_into().ok()?);
        let zeros = bytes.get(offset - 12..offset - 4)?;
        if (100..100_000).contains(&id) && zeros.iter().all(|byte| *byte == 0) {
            return Some((id.to_string(), offset - 16, true));
        }
    }
    ascii_id_at(bytes, offset + name.len() + 8).map(|(id, value_offset)| (id, value_offset, false))
}

fn ascii_id_at(bytes: &[u8], length_offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32::from_le_bytes(
        bytes
            .get(length_offset..length_offset + 4)?
            .try_into()
            .ok()?,
    ))
    .ok()?;
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
    /// The referenced ASM body key.
    pub asm_key: u64,
    /// The body's design-entity suffix.
    pub entity_suffix: u64,
    /// Byte offset of `entity_suffix` within the stream.
    pub entity_suffix_offset: usize,
}

/// Parse every BREP body-map record in a Design `BulkStream`: a `u32` pair
/// count, `count` pairs of `(u64 asm_body_key, u64 entity_suffix)`, the
/// trailing record ref and pad, then the length-prefixed UTF-16 blob name
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
pub(crate) fn body_bindings(bytes: &[u8]) -> Vec<BodyBinding> {
    let needle: Vec<u8> = "BREP.".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = Vec::new();
    for offset in bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
    {
        let Some(name_chars) = offset
            .checked_sub(4)
            .and_then(|at| read_u32(bytes, at))
            .map(|chars| chars as usize)
        else {
            continue;
        };
        let Some(blob_name) = bytes
            .get(offset..offset + name_chars * 2)
            .map(utf16_le_string)
        else {
            continue;
        };
        // 16 bytes separate the pairs from the name: the 12-byte record tail
        // and the name's u32 length prefix.
        let Some(pairs_end) = offset.checked_sub(16) else {
            continue;
        };
        // The pair count precedes the pairs; scanning ascending is unambiguous
        // because the high halves of the little-endian ids are zero.
        for count in 1usize..=64 {
            let span = 16 * count;
            let Some(count_at) = pairs_end.checked_sub(span + 4) else {
                break;
            };
            if read_u32(bytes, count_at) != Some(count as u32) {
                continue;
            }
            for pair in 0..count {
                let at = count_at + 4 + pair * 16;
                if let (Some(key), Some(suffix)) = (read_u64(bytes, at), read_u64(bytes, at + 8)) {
                    out.push(BodyBinding {
                        blob_name: blob_name.clone(),
                        asm_key: key,
                        entity_suffix: suffix,
                        entity_suffix_offset: at + 8,
                    });
                }
            }
            break;
        }
    }
    out
}

/// Decode per-body display visibility from the Design `BulkStream`.
///
/// The BREP body-map record resolves ASM body keys of `active_brep_entry` to
/// design-entity suffixes, and each entity's browser-node record carries a
/// hidden flag directly after the node GUID
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
/// The result maps each ASM body key to its display visibility; bodies
/// without records are absent.
pub fn decode_body_visibility(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    active_brep_entry: &str,
) -> Result<HashMap<u64, bool>, CodecError> {
    let Some(basename) = active_brep_entry
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
    else {
        return Ok(HashMap::new());
    };
    let mut out = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let hidden_by_entity = browser_node_hidden_flags(bytes);
        for binding in body_bindings(bytes) {
            if binding.blob_name != basename {
                continue;
            }
            if let Some(hidden) = hidden_by_entity.get(&binding.entity_suffix) {
                out.insert(binding.asm_key, !hidden);
            }
        }
    }
    Ok(out)
}

/// Scan for browser-node records: a length-prefixed 36-character UTF-16 GUID,
/// one hidden-flag byte, the `01 01` marker, and the `u64` design-entity
/// suffix.
fn browser_node_hidden_flags(bytes: &[u8]) -> HashMap<u64, bool> {
    const GUID_CHARS: usize = 36;
    const GUID_BYTES: usize = GUID_CHARS * 2;
    let mut out = HashMap::new();
    let mut at = 0usize;
    while at + 4 + GUID_BYTES + 3 + 8 <= bytes.len() {
        if read_u32(bytes, at) != Some(GUID_CHARS as u32)
            || !is_utf16_guid(&bytes[at + 4..at + 4 + GUID_BYTES])
        {
            at += 1;
            continue;
        }
        let flag_at = at + 4 + GUID_BYTES;
        if bytes.get(flag_at + 1..flag_at + 3) == Some(&[0x01, 0x01]) {
            if let (flag @ (0 | 1), Some(member)) = (bytes[flag_at], read_u64(bytes, flag_at + 3)) {
                out.insert(member, flag == 1);
            }
        }
        at += 1;
    }
    out
}

fn utf16_le_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn is_utf16_guid(bytes: &[u8]) -> bool {
    bytes
        .chunks_exact(2)
        .all(|pair| pair[1] == 0 && (pair[0].is_ascii_hexdigit() || pair[0] == b'-'))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}
