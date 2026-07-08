// SPDX-License-Identifier: Apache-2.0
//! Fusion Design BulkStream parametric-construction records.

use std::collections::HashMap;

use cadmpeg_ir::codec::{CodecError, ReadSeek};
use cadmpeg_ir::design::{
    ConstructionRecipe, ConstructionRecipeKind, DesignBodyMember, DesignEntityHeader, DesignObject,
    DesignObjectKind, DesignRecordHeader, LostEdgeReference, PersistentReference,
    PersistentReferenceKind, SketchCurveGeometry, SketchCurveIdentity, SketchPoint, SketchRelation,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::provenance::{EntityMeta, Exactness, Provenance};

use crate::container::{self, role, ContainerScan};

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

pub fn decode_recipes(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<ConstructionRecipe>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = container::decompress_entry(reader, &entry.name)?;
        decode_stream(&bytes, &entry.name, &mut out);
    }
    Ok(out)
}

pub fn decode_persistent_references(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<PersistentReference>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = container::decompress_entry(reader, &entry.name)?;
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
                if u32::from_le_bytes(length_bytes.try_into().unwrap()) != 23 {
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
                    kind,
                    value: u64::from_le_bytes(raw.try_into().unwrap()),
                    meta: EntityMeta {
                        provenance: Provenance {
                            format: "f3d".into(),
                            stream: entry.name.clone(),
                            offset: offset as u64,
                            tag: Some(String::from_utf8_lossy(name).into_owned()),
                        },
                        exactness: Exactness::ByteExact,
                    },
                });
            }
        }
    }
    out.sort_by_key(|reference| reference.meta.provenance.offset);
    Ok(out)
}

pub fn decode_lost_edge_references(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<LostEdgeReference>, CodecError> {
    let mut out = Vec::new();
    let marker = b"EDGE_REFERENCE_LOST";
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = container::decompress_entry(reader, &entry.name)?;
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
            if u32::from_le_bytes(length.try_into().unwrap()) != 3 {
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
                class_tag: String::from_utf8_lossy(class_tag).into_owned(),
                record_index: u32::from_le_bytes(index.try_into().unwrap()),
                meta: EntityMeta {
                    provenance: Provenance {
                        format: "f3d".into(),
                        stream: entry.name.clone(),
                        offset: offset as u64,
                        tag: Some("EDGE_REFERENCE_LOST".into()),
                    },
                    exactness: Exactness::ByteExact,
                },
            });
        }
    }
    Ok(out)
}

pub fn decode_objects(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignObject>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::METASTREAM && entry.name.contains("Design"))
    {
        let bytes = container::decompress_entry(reader, &entry.name)?;
        let mut offset = 0usize;
        while offset + 8 <= bytes.len() {
            let Some((name, after_name)) = lp_ascii(&bytes, offset) else {
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
            let count = usize::try_from(u32::from_le_bytes(count_raw.try_into().unwrap()))
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
                .map(|raw| u64::from_le_bytes(raw.try_into().unwrap()))
                .collect();
            let Some((self_guid, after_self)) =
                lp_ascii(&bytes, ids_end).filter(|(guid, _)| is_guid(guid))
            else {
                offset += 1;
                continue;
            };
            let mut tail = after_self;
            while bytes.get(tail) == Some(&0) {
                tail += 1;
            }
            let (parent_guid, revision_offset) = lp_ascii(&bytes, tail)
                .filter(|(guid, _)| is_guid(guid))
                .map_or((None, tail), |(guid, end)| (Some(guid), end));
            let Some(revision_raw) = bytes.get(revision_offset..revision_offset + 4) else {
                offset += 1;
                continue;
            };
            let revision = u32::from_le_bytes(revision_raw.try_into().unwrap());
            if revision > 10_000 {
                offset += 1;
                continue;
            }
            out.push(DesignObject {
                kind,
                entity_ids,
                self_guid,
                parent_guid,
                revision,
                meta: EntityMeta {
                    provenance: Provenance {
                        format: "f3d".into(),
                        stream: entry.name.clone(),
                        offset: offset as u64,
                        tag: Some(name),
                    },
                    exactness: Exactness::ByteExact,
                },
            });
            offset = revision_offset + 4;
        }
    }
    Ok(out)
}

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
        let bytes = container::decompress_entry(reader, &entry.name)?;
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
            let entity_suffix = u64::from_le_bytes(entity_raw.try_into().unwrap());
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
            let Some((entity_id, end)) = lp_utf16(&bytes, string_offset) else {
                continue;
            };
            let Some((_, suffix)) = entity_id.rsplit_once('_') else {
                continue;
            };
            if suffix.parse::<u64>().ok() != Some(entity_suffix) {
                continue;
            }
            let object_kind = object_kinds.get(&entity_suffix).copied();
            let (record_reference, declared_reference_count, reference_indices, record_end) =
                if object_kind == Some(DesignObjectKind::Sketch) {
                    decode_reference_list(&bytes, end)
                        .map(|list| {
                            (
                                Some(list.record_reference),
                                Some(list.declared_count),
                                list.references,
                                list.end,
                            )
                        })
                        .unwrap_or_else(|| (None, None, Vec::new(), end))
                } else {
                    (None, None, Vec::new(), end)
                };
            out.push(DesignEntityHeader {
                entity_suffix,
                entity_id,
                class_tag: String::from_utf8_lossy(class_tag).into_owned(),
                optional_slot_present,
                object_kind,
                record_reference,
                declared_reference_count,
                reference_indices,
                meta: EntityMeta {
                    provenance: Provenance {
                        format: "f3d".into(),
                        stream: entry.name.clone(),
                        offset: start as u64,
                        tag: Some("design_entity_header".into()),
                    },
                    exactness: Exactness::ByteExact,
                },
            });
            offset = record_end;
        }
    }
    Ok(out)
}

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
    reader: &mut dyn ReadSeek,
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
        let bytes = container::decompress_entry(reader, &entry.name)?;
        let mut position = 0usize;
        while position + 11 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(&bytes, position) else {
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
            let record_index = u32::from_le_bytes(raw.try_into().unwrap());
            if wanted.contains(&record_index) && emitted.insert(record_index) {
                out.push(DesignRecordHeader {
                    record_index,
                    class_tag,
                    meta: EntityMeta {
                        provenance: Provenance {
                            format: "f3d".into(),
                            stream: entry.name.clone(),
                            offset: position as u64,
                            tag: Some("design_record_header".into()),
                        },
                        exactness: Exactness::ByteExact,
                    },
                });
            }
            position = after_tag + 4;
        }
    }
    out.sort_by_key(|record| record.meta.provenance.offset);
    Ok(out)
}

pub fn decode_sketch_relations(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    records: &[DesignRecordHeader],
) -> Result<Vec<SketchRelation>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = container::decompress_entry(reader, &entry.name)?;
        for record in records
            .iter()
            .filter(|record| record.meta.provenance.stream == entry.name)
        {
            let at = record.meta.provenance.offset as usize;
            let Some(payload) = bytes.get(at..at + 101) else {
                continue;
            };
            let Some((members, owner_reference, state, return_members)) =
                parse_sketch_relation(payload)
            else {
                continue;
            };
            out.push(SketchRelation {
                record_index: record.record_index,
                class_tag: record.class_tag.clone(),
                owner_reference,
                members,
                state,
                return_members,
                raw_bytes: payload.to_vec(),
                meta: record.meta.clone(),
            });
        }
    }
    Ok(out)
}

pub fn decode_sketch_points(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<SketchPoint>, CodecError> {
    let mut out = Vec::new();
    let mut emitted = std::collections::HashSet::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = container::decompress_entry(reader, &entry.name)?;
        let mut at = 0usize;
        while at + 112 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(&bytes, at) else {
                at += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                at += 1;
                continue;
            }
            let Some(record_index) = u32_at(&bytes, after_tag) else {
                break;
            };
            let payload = &bytes[at..at + 112];
            if payload.get(20) != Some(&1)
                || u32_at(payload, 21) != Some(1)
                || u32_at(payload, 25) != Some(6)
                || payload.get(29..35) != Some(b"pt_tag")
                || u32_at(payload, 35) != Some(23)
                || payload.get(39..62) != Some(b"IntrinsicMetaTypeuint64")
                || payload.get(70) != Some(&1)
            {
                at += 1;
                continue;
            }
            let (Some(x), Some(y)) = (f64_at(payload, 96), f64_at(payload, 104)) else {
                at += 1;
                continue;
            };
            if !x.is_finite() || !y.is_finite() {
                at += 1;
                continue;
            }
            if emitted.insert(record_index) {
                out.push(SketchPoint {
                    record_index,
                    class_tag,
                    persistent_id: u64::from_le_bytes(payload[62..70].try_into().unwrap()),
                    paired_reference: u32_at(payload, 71).unwrap(),
                    coordinates: Point2::new(x * 10.0, y * 10.0),
                    meta: EntityMeta {
                        provenance: Provenance {
                            format: "f3d".into(),
                            stream: entry.name.clone(),
                            offset: at as u64,
                            tag: Some("sketch_point".into()),
                        },
                        exactness: Exactness::ByteExact,
                    },
                });
            }
            at += 112;
        }
    }
    Ok(out)
}

pub fn decode_sketch_curve_identities(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    let mut out = Vec::new();
    let mut emitted = std::collections::HashSet::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = container::decompress_entry(reader, &entry.name)?;
        let mut at = 0usize;
        while at + 133 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(&bytes, at) else {
                at += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                at += 1;
                continue;
            }
            let Some(record_index) = u32_at(&bytes, after_tag) else {
                break;
            };
            let payload = &bytes[at..at + 133];
            if payload.get(20) != Some(&1)
                || u32_at(payload, 21) != Some(2)
                || u32_at(payload, 25) != Some(14)
                || payload.get(29..43) != Some(b"crv_primary_id")
                || u32_at(payload, 43) != Some(23)
                || payload.get(47..70) != Some(b"IntrinsicMetaTypeuint64")
                || u32_at(payload, 78) != Some(16)
                || payload.get(82..98) != Some(b"crv_secondary_id")
                || u32_at(payload, 98) != Some(23)
                || payload.get(102..125) != Some(b"IntrinsicMetaTypeuint64")
            {
                at += 1;
                continue;
            }
            if emitted.insert(record_index) {
                out.push(SketchCurveIdentity {
                    record_index,
                    class_tag,
                    primary_id: u64::from_le_bytes(payload[70..78].try_into().unwrap()),
                    secondary_id: u64::from_le_bytes(payload[125..133].try_into().unwrap()),
                    geometry: bytes.get(at..at + 229).and_then(|payload| {
                        decode_circular_arc(payload).or_else(|| decode_line(payload))
                    }),
                    meta: EntityMeta {
                        provenance: Provenance {
                            format: "f3d".into(),
                            stream: entry.name.clone(),
                            offset: at as u64,
                            tag: Some("sketch_curve_identity".into()),
                        },
                        exactness: Exactness::ByteExact,
                    },
                });
            }
            at += 133;
        }
    }
    Ok(out)
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
        || !(values[9] > 0.0)
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
    if !(length > 0.0) {
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

fn parse_sketch_relation(payload: &[u8]) -> Option<(Vec<u32>, u32, u32, Vec<u32>)> {
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
        cursor = next_marker(payload, end)?;
    }
    let (owner_reference, end) = marked_u32(payload, cursor)?;
    cursor = next_marker(payload, end)?;
    let (state, end) = marked_u32(payload, cursor)?;
    cursor = next_nonzero(payload, end)?;
    let return_count = usize::try_from(u32_at(payload, cursor)?).ok()?;
    if return_count > 64 {
        return None;
    }
    cursor += 4;
    let mut return_members = Vec::with_capacity(return_count);
    for ordinal in 0..return_count {
        cursor = next_marker(payload, cursor)?;
        let (value, end) = marked_u32(payload, cursor)?;
        return_members.push(value);
        cursor = end;
        if ordinal + 1 < return_count {
            cursor = next_marker(payload, cursor)?;
        }
    }
    Some((members, owner_reference, state, return_members))
}

fn marked_u32(bytes: &[u8], position: usize) -> Option<(u32, usize)> {
    (bytes.get(position) == Some(&1)).then_some((u32_at(bytes, position + 1)?, position + 5))
}

fn next_marker(bytes: &[u8], mut position: usize) -> Option<usize> {
    while bytes.get(position) == Some(&0) {
        position += 1;
    }
    (bytes.get(position) == Some(&1)).then_some(position)
}

fn next_nonzero(bytes: &[u8], mut position: usize) -> Option<usize> {
    while bytes.get(position) == Some(&0) {
        position += 1;
    }
    (position + 4 <= bytes.len()).then_some(position)
}

fn u32_at(bytes: &[u8], position: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(position..position + 4)?.try_into().ok()?,
    ))
}

fn f64_at(bytes: &[u8], position: usize) -> Option<f64> {
    Some(f64::from_le_bytes(
        bytes.get(position..position + 8)?.try_into().ok()?,
    ))
}

struct SketchReferenceList {
    record_reference: u32,
    declared_count: u32,
    references: Vec<u32>,
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
    while bytes.get(cursor) == Some(&1) && bytes.get(cursor + 5..cursor + 11) == Some(&[0; 6]) {
        references.push(u32::from_le_bytes(
            bytes.get(cursor + 1..cursor + 5)?.try_into().ok()?,
        ));
        cursor += 11;
    }
    (references.len() == declared_count as usize).then_some(SketchReferenceList {
        record_reference,
        declared_count,
        references,
        end: cursor,
    })
}

pub fn decode_body_members(
    reader: &mut dyn ReadSeek,
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
        let bytes = container::decompress_entry(reader, &entry.name)?;
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
        let count = usize::try_from(u32::from_le_bytes(count_raw.try_into().unwrap()))
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
                entity_suffix: u64::from_le_bytes(id_raw.try_into().unwrap()),
                flags: u16::from_le_bytes(flags_raw.try_into().unwrap()),
                meta: EntityMeta {
                    provenance: Provenance {
                        format: "f3d".into(),
                        stream: entry.name.clone(),
                        offset: cursor as u64,
                        tag: Some("BodiesRoot.member".into()),
                    },
                    exactness: Exactness::ByteExact,
                },
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
    let length = usize::try_from(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
    .ok()?;
    if length > 2_000 {
        return None;
    }
    let end = offset.checked_add(4 + length)?;
    let raw = bytes.get(offset + 4..end)?;
    raw.iter()
        .all(|byte| byte.is_ascii_graphic())
        .then(|| (String::from_utf8_lossy(raw).into_owned(), end))
}

fn lp_utf16(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
    .ok()?;
    if !(1..=256).contains(&length) {
        return None;
    }
    let end = offset.checked_add(4 + length * 2)?;
    let units = bytes
        .get(offset + 4..end)?
        .chunks_exact(2)
        .map(|raw| u16::from_le_bytes(raw.try_into().unwrap()))
        .collect::<Vec<_>>();
    Some((String::from_utf16(&units).ok()?, end))
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
            let design_id = recipe_design_id(bytes, offset, name);
            let key = (kind, design_id.clone());
            let counter = counters.entry(key).or_default();
            let recipe_index = *counter;
            *counter += 1;
            let record_index = offset
                .checked_sub(16)
                .and_then(|at| bytes.get(at..at + 4))
                .map(|raw| i32::from_le_bytes(raw.try_into().unwrap()))
                .unwrap_or_default();
            out.push(ConstructionRecipe {
                kind,
                design_id,
                recipe_index,
                record_index,
                meta: EntityMeta {
                    provenance: Provenance {
                        format: "f3d".into(),
                        stream: stream.into(),
                        offset: offset as u64,
                        tag: Some(String::from_utf8_lossy(name).into_owned()),
                    },
                    exactness: Exactness::ByteExact,
                },
            });
        }
    }
    out.sort_by_key(|recipe| recipe.meta.provenance.offset);
}

fn recipe_design_id(bytes: &[u8], offset: usize, name: &[u8]) -> Option<String> {
    let pre = offset.checked_sub(27)?;
    if let Some(id) = ascii_id_at(bytes, pre) {
        return Some(id);
    }
    if offset >= 23 {
        let candidate = bytes.get(offset - 23..offset - 20)?;
        if candidate.iter().all(u8::is_ascii_digit) {
            return Some(String::from_utf8_lossy(candidate).into_owned());
        }
    }
    if name == b"bounded_face_recipe_data" && offset >= 16 {
        let id = u32::from_le_bytes(bytes[offset - 16..offset - 12].try_into().ok()?);
        let zeros = bytes.get(offset - 12..offset - 4)?;
        if (100..100_000).contains(&id) && zeros.iter().all(|byte| *byte == 0) {
            return Some(id.to_string());
        }
    }
    ascii_id_at(bytes, offset + name.len() + 8)
}

fn ascii_id_at(bytes: &[u8], length_offset: usize) -> Option<String> {
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
    value
        .iter()
        .all(u8::is_ascii_alphanumeric)
        .then(|| String::from_utf8_lossy(value).into_owned())
}

#[cfg(test)]
mod diagnostics {
    #[test]
    #[ignore]
    fn inspect_relation_operands() {
        use std::{env, fs::File};
        let mut file = File::open(env::var("F3D_INSPECT").unwrap()).unwrap();
        let scan = crate::container::scan(&mut file).unwrap();
        let entities = super::decode_entity_headers(&mut file, &scan).unwrap();
        let roots = super::decode_record_headers(&mut file, &scan, &entities).unwrap();
        let relations = super::decode_sketch_relations(&mut file, &scan, &roots).unwrap();
        let indices = relations
            .iter()
            .flat_map(|r| r.members.iter().chain(&r.return_members))
            .copied()
            .collect::<Vec<_>>();
        let operands = super::decode_related_record_headers(&mut file, &scan, &indices).unwrap();
        let curves = super::decode_sketch_curve_identities(&mut file, &scan).unwrap();
        for entry in scan.entries.iter().filter(|entry| {
            entry.role == crate::container::role::BULKSTREAM && entry.name.contains("Design")
        }) {
            let bytes = crate::container::decompress_entry(&mut file, &entry.name).unwrap();
            for record in operands.iter().take(8) {
                let at = record.meta.provenance.offset as usize;
                eprintln!(
                    "OPERAND {} {} @{at} {:?}",
                    record.record_index,
                    record.class_tag,
                    &bytes[at..(at + 160).min(bytes.len())]
                );
            }
            if let Some(at) = bytes
                .windows(14)
                .position(|window| window == b"crv_primary_id")
            {
                let start = at.saturating_sub(48);
                eprintln!(
                    "CURVE @{at} {:?}",
                    &bytes[start..(at + 240).min(bytes.len())]
                );
                let record = at - 29;
                for offset in (133..245).step_by(8) {
                    eprintln!("D {offset} {:?}", super::f64_at(&bytes[record..], offset));
                }
            }
            if let Some(curve) = curves.iter().find(|curve| {
                curve.geometry.is_none() && curve.meta.provenance.stream == entry.name
            }) {
                let at = curve.meta.provenance.offset as usize;
                let next = curves
                    .iter()
                    .map(|other| other.meta.provenance.offset as usize)
                    .filter(|offset| *offset > at)
                    .min();
                eprintln!(
                    "NONARC {} @{at} next={next:?} delta={:?}",
                    curve.record_index,
                    next.map(|next| next - at)
                );
                eprintln!("RAW {:?}", &bytes[at + 133..(at + 420).min(bytes.len())]);
                for (line, chunk) in bytes[at + 133..at + 293].chunks(16).enumerate() {
                    eprintln!("S {:03} {:02x?}", line * 16, chunk);
                }
                for offset in (133..229).step_by(8) {
                    eprintln!("N {offset} {:?}", super::f64_at(&bytes[at..], offset));
                }
            }
        }
    }
}
