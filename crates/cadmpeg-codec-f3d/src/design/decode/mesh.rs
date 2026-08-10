// SPDX-License-Identifier: Apache-2.0
//! Join mesh-geometry containers to their bodies through the Design segment.
//!
//! A mesh body's geometry lives in a `.paramesh` container ([spec §1.1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#112-mesh-geometry-containers)),
//! and three Design record classes join the container to its body ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)):
//! the entry-name class stores the blob-part entry name, the GUID class stores
//! the GUID the container's protobuf message carries as `fusion_uuid`, and the
//! mesh-body class carries the affine map between container coordinates and
//! model centimetres. The two identity records reference each other, and the
//! body record references the GUID record and its owning feature scope.

use crate::bytes::{is_guid_hyphenated, lp_ascii_strict, lp_utf16_bounded, take_reference};
use crate::container::{role, ContainerScan};
use crate::design::decode::meta::{
    metadata_for_bulk_stream, typed_primary_frames, TypedPrimaryFrame,
};
use crate::ids;
use crate::paramesh::decode_mesh_container;
use cadmpeg_core::le::{f64_at, u32_at};
use cadmpeg_core::CodecError;

const PARAMESH_MODULE: &str = "ParaMesh";
const MESH_ENTRY_NAME_TYPE_GUID: &str = "A1BAA3F6-4B67-4A0D-BACC-75F38A2230F3";
const MESH_ENTRY_NAME_BASE_TYPE_GUID: &str = "130A0711-4E92-4FCD-AADE-B9C82238BB27";
const MESH_ENTRY_NAME_TYPE_VERSION: u32 = 0;
const MESH_GUID_TYPE_GUID: &str = "A8338A26-5436-433C-8BAC-C3CF024AD595";
const MESH_GUID_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const MESH_GUID_TYPE_VERSION: u32 = 1;
const MESH_BODY_TYPE_GUID: &str = "EA90DA22-556C-4C61-89BB-20C2681B7A9D";
const MESH_BODY_BASE_TYPE_GUID: &str = "CB844AB6-240D-4FC9-9C9F-3679DC896D6F";
const MESH_BODY_TYPE_VERSION: u32 = 7;

/// Row-major 4x4 f64 matrix byte length.
const MATRIX_BYTES: usize = 128;
const MESH_BODY_FIRST_MATRIX_AT: usize = 42;
const MESH_BODY_MATRIX_SEPARATOR_BYTES: usize = 1;
const MESH_BODY_SECOND_MATRIX_AT: usize =
    MESH_BODY_FIRST_MATRIX_AT + MATRIX_BYTES + MESH_BODY_MATRIX_SEPARATOR_BYTES;
const MESH_BODY_SCOPE_REFERENCE_AT: usize = 508;
const MESH_BODY_GUID_REFERENCE_AT: usize = 541;
const SAME_SEGMENT_REFERENCE_BYTES: usize = 11;
const MESH_ENTRY_GUID_REFERENCE_AT: usize = 21;
const MESH_ENTRY_NAME_AT: usize = 32;
const MESH_GUID_VALUE_AT: usize = 32;
const MESH_GUID_ENTRY_REFERENCE_AT: usize = 72;

/// One mesh body's geometry, in model millimetres.
pub(crate) struct MeshBody {
    /// Deterministic native identifier, keyed by the mesh-body record.
    pub(crate) id: String,
    /// Native Design stream containing the mesh-body record.
    pub(crate) design_stream: String,
    /// Sole indexed Design feature scope referenced by the mesh-body record.
    pub(crate) design_scope_record_index: u32,
    /// Vertex positions in model millimetres.
    pub(crate) vertices: Vec<cadmpeg_ir::math::Point3>,
    /// Triangle corner indices into `vertices`.
    pub(crate) triangles: Vec<[u32; 3]>,
    /// The attribute channels the container's registry declares.
    pub(crate) attributes: Vec<crate::paramesh::MeshAttribute>,
}

/// A finite, orientation-preserving row-major affine map.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MeshAffineTransform([f64; 16]);

impl MeshAffineTransform {
    fn parse(bytes: &[u8], at: usize) -> Option<Self> {
        let mut cells = [0.0; 16];
        for (index, cell) in cells.iter_mut().enumerate() {
            *cell = f64_at(bytes, at.checked_add(index.checked_mul(8)?)?)?;
            cell.is_finite().then_some(())?;
        }
        (cells[12..16] == [0.0, 0.0, 0.0, 1.0]).then_some(())?;
        let determinant = cells[0] * (cells[5] * cells[10] - cells[6] * cells[9])
            - cells[1] * (cells[4] * cells[10] - cells[6] * cells[8])
            + cells[2] * (cells[4] * cells[9] - cells[5] * cells[8]);
        (determinant.is_finite() && determinant > 0.0).then_some(Self(cells))
    }

    fn transform(self, point: [f64; 3]) -> cadmpeg_ir::math::Point3 {
        let [x, y, z] = point;
        let cells = self.0;
        cadmpeg_ir::math::Point3::new(
            (cells[0] * x + cells[1] * y + cells[2] * z + cells[3])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
            (cells[4] * x + cells[5] * y + cells[6] * z + cells[7])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
            (cells[8] * x + cells[9] * y + cells[10] * z + cells[11])
                * cadmpeg_asm::nurbs::reader::LEN_TO_MM,
        )
    }
}

/// The two equal affine maps stored by a mesh-body class record.
fn mesh_body_transform(payload: &[u8]) -> Option<MeshAffineTransform> {
    let first = MeshAffineTransform::parse(payload, MESH_BODY_FIRST_MATRIX_AT)?;
    let second = MeshAffineTransform::parse(payload, MESH_BODY_SECOND_MATRIX_AT)?;
    (first == second).then_some(first)
}

/// Result of decoding and joining one `.paramesh` entry.
pub(crate) enum MeshContainerOutcome {
    /// Geometry and its Design body record were decoded and joined.
    Joined(MeshBody),
    /// The container decoded, but no complete Design join named it.
    Unjoined {
        /// Native archive entry name.
        entry_name: String,
    },
    /// Reading or parsing the container failed independently of other entries.
    Failed {
        /// Native archive entry name.
        entry_name: String,
        /// Exact read or parse failure.
        error: CodecError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshEntryNameRecord {
    record_index: u32,
    guid_record_index: u32,
    entry_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MeshGuidRecord {
    record_index: u32,
    entry_name_record_index: u32,
    fusion_uuid: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MeshBodyRecord {
    byte_offset: usize,
    guid_record_index: u32,
    scope_record_index: u32,
    transform: MeshAffineTransform,
}

#[derive(Clone, Debug, PartialEq)]
struct MeshDesignRecords {
    stream: String,
    entry_names: Vec<MeshEntryNameRecord>,
    guids: Vec<MeshGuidRecord>,
    bodies: Vec<MeshBodyRecord>,
}

fn validate_mesh_registration(
    frame: TypedPrimaryFrame<'_>,
    expected_version: u32,
    expected_base_type_guid: &str,
    record_kind: &str,
) -> Result<(), CodecError> {
    if frame.design_type.version != expected_version {
        return Err(CodecError::NotImplemented(format!(
            "F3D Design {record_kind} record version {} is unsupported",
            frame.design_type.version
        )));
    }
    if frame.design_type.module != PARAMESH_MODULE
        || !frame
            .design_type
            .base_type_guid
            .as_deref()
            .is_some_and(|base| base.eq_ignore_ascii_case(expected_base_type_guid))
    {
        return Err(CodecError::Malformed(format!(
            "F3D Design {record_kind} entity {} has incompatible registration metadata",
            frame.entity_id
        )));
    }
    Ok(())
}

fn exact_record_index(
    record: &[u8],
    frame: TypedPrimaryFrame<'_>,
    record_kind: &str,
) -> Result<u32, CodecError> {
    u32_at(record, 7)
        .filter(|record_index| u64::from(*record_index) == frame.entity_id)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "F3D Design {record_kind} entity {} has an invalid record index",
                frame.entity_id
            ))
        })
}

fn exact_local_record_index(record: &[u8], at: usize) -> Option<u32> {
    let mut cursor = at;
    let reference = take_reference(record, &mut cursor)?;
    if cursor != at.checked_add(SAME_SEGMENT_REFERENCE_BYTES)?
        || reference.segment.is_some()
        || reference.link_name.is_some()
    {
        return None;
    }
    u32::try_from(reference.target?)
        .ok()
        .filter(|target| *target != 0)
}

fn parse_mesh_entry_name_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshEntryNameRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_ENTRY_NAME_TYPE_VERSION,
        MESH_ENTRY_NAME_BASE_TYPE_GUID,
        "mesh-entry-name",
    )?;
    let record = &bytes[frame.start..frame.end];
    let record_index = exact_record_index(record, frame, "mesh-entry-name")?;
    let parsed = (|| {
        (record.get(11..21) == Some(&[0; 10])).then_some(())?;
        let guid_record_index = exact_local_record_index(record, MESH_ENTRY_GUID_REFERENCE_AT)?;
        let (entry_name, end) = lp_utf16_bounded(record, MESH_ENTRY_NAME_AT, 1..=1024)?;
        (end == record.len()).then_some(MeshEntryNameRecord {
            record_index,
            guid_record_index,
            entry_name,
        })
    })();
    parsed.ok_or_else(|| {
        CodecError::Malformed(format!(
            "F3D Design mesh-entry-name entity {} has an invalid primary frame",
            frame.entity_id
        ))
    })
}

fn parse_mesh_guid_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshGuidRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_GUID_TYPE_VERSION,
        MESH_GUID_BASE_TYPE_GUID,
        "mesh-GUID",
    )?;
    let record = &bytes[frame.start..frame.end];
    let record_index = exact_record_index(record, frame, "mesh-GUID")?;
    let parsed = (|| {
        (record.get(11..32) == Some(&[0; 21])).then_some(())?;
        let (fusion_uuid, end) = lp_ascii_strict(record, MESH_GUID_VALUE_AT, 36..=36)?;
        (end == MESH_GUID_ENTRY_REFERENCE_AT && is_guid_hyphenated(&fusion_uuid)).then_some(())?;
        let entry_name_record_index =
            exact_local_record_index(record, MESH_GUID_ENTRY_REFERENCE_AT)?;
        Some(MeshGuidRecord {
            record_index,
            entry_name_record_index,
            fusion_uuid,
        })
    })();
    parsed.ok_or_else(|| {
        CodecError::Malformed(format!(
            "F3D Design mesh-GUID entity {} has an invalid primary frame",
            frame.entity_id
        ))
    })
}

fn parse_mesh_body_record(
    bytes: &[u8],
    frame: TypedPrimaryFrame<'_>,
) -> Result<MeshBodyRecord, CodecError> {
    validate_mesh_registration(
        frame,
        MESH_BODY_TYPE_VERSION,
        MESH_BODY_BASE_TYPE_GUID,
        "mesh-body",
    )?;
    let record = &bytes[frame.start..frame.end];
    exact_record_index(record, frame, "mesh-body")?;
    let parsed = (|| {
        (record.get(11..21) == Some(&[0; 10])).then_some(())?;
        let scope_record_index = exact_local_record_index(record, MESH_BODY_SCOPE_REFERENCE_AT)?;
        let guid_record_index = exact_local_record_index(record, MESH_BODY_GUID_REFERENCE_AT)?;
        let transform = mesh_body_transform(record)?;
        Some(MeshBodyRecord {
            byte_offset: frame.start,
            guid_record_index,
            scope_record_index,
            transform,
        })
    })();
    parsed.ok_or_else(|| {
        CodecError::Malformed(format!(
            "F3D Design mesh-body entity {} has an invalid primary frame",
            frame.entity_id
        ))
    })
}

fn parse_mesh_design_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: String,
) -> Result<MeshDesignRecords, CodecError> {
    let entry_names =
        typed_primary_frames(bytes, meta, MESH_ENTRY_NAME_TYPE_GUID, "mesh-entry-name")?
            .into_iter()
            .map(|frame| parse_mesh_entry_name_record(bytes, frame))
            .collect::<Result<Vec<_>, _>>()?;
    let guids = typed_primary_frames(bytes, meta, MESH_GUID_TYPE_GUID, "mesh-GUID")?
        .into_iter()
        .map(|frame| parse_mesh_guid_record(bytes, frame))
        .collect::<Result<Vec<_>, _>>()?;
    let bodies = typed_primary_frames(bytes, meta, MESH_BODY_TYPE_GUID, "mesh-body")?
        .into_iter()
        .map(|frame| parse_mesh_body_record(bytes, frame))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(MeshDesignRecords {
        stream,
        entry_names,
        guids,
        bodies,
    })
}

fn decode_mesh_design_records(scan: &ContainerScan) -> Result<Vec<MeshDesignRecords>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let records = parse_mesh_design_records(
            scan.entry_bytes(&entry.name)?,
            &meta,
            ids::native_scope(&entry.name),
        )?;
        if !records.entry_names.is_empty()
            || !records.guids.is_empty()
            || !records.bodies.is_empty()
        {
            out.push(records);
        }
    }
    Ok(out)
}

fn resolve_mesh_body<'a>(
    records: &'a [MeshDesignRecords],
    entry_name: &str,
    fusion_uuid: &str,
) -> Option<(&'a MeshDesignRecords, &'a MeshBodyRecord)> {
    let mut matches = Vec::new();
    for design in records {
        for entry in design
            .entry_names
            .iter()
            .filter(|entry| entry.entry_name == entry_name)
        {
            for guid in design.guids.iter().filter(|guid| {
                guid.record_index == entry.guid_record_index
                    && guid.entry_name_record_index == entry.record_index
                    && guid.fusion_uuid == fusion_uuid
            }) {
                matches.extend(
                    design
                        .bodies
                        .iter()
                        .filter(|body| body.guid_record_index == guid.record_index)
                        .map(|body| (design, body)),
                );
            }
        }
    }
    let [joined] = matches.as_slice() else {
        return None;
    };
    Some(*joined)
}

/// Decode every mesh body: one per `.paramesh` container joined to the
/// mesh-body record that names its GUID record.
pub(crate) fn decode_mesh_bodies(
    scan: &ContainerScan,
) -> Result<Vec<MeshContainerOutcome>, CodecError> {
    let design_records = decode_mesh_design_records(scan)?;
    let mut outcomes = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_asset_entry(entry, role::PARAMESH))
    {
        let container = match scan
            .entry_bytes(&entry.name)
            .and_then(decode_mesh_container)
        {
            Ok(container) => container,
            Err(error) => {
                outcomes.push(MeshContainerOutcome::Failed {
                    entry_name: entry.name.clone(),
                    error,
                });
                continue;
            }
        };
        let name = &entry.name;
        let base = name.rsplit('/').next().unwrap_or(name);
        let Some((design, body)) = resolve_mesh_body(&design_records, base, &container.fusion_uuid)
        else {
            outcomes.push(MeshContainerOutcome::Unjoined {
                entry_name: entry.name.clone(),
            });
            continue;
        };
        outcomes.push(MeshContainerOutcome::Joined(MeshBody {
            id: ids::native_mesh_body_id(&entry.name, body.byte_offset),
            design_stream: design.stream.clone(),
            design_scope_record_index: body.scope_record_index,
            vertices: container
                .vertices
                .iter()
                .copied()
                .map(|point| body.transform.transform(point))
                .collect(),
            triangles: container.triangles,
            attributes: container.attributes,
        }));
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(cells: [f64; 16]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(MATRIX_BYTES);
        for value in cells {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn mesh_body_payload(cells: [f64; 16]) -> Vec<u8> {
        let mut payload = vec![0; MESH_BODY_FIRST_MATRIX_AT];
        payload.extend_from_slice(&matrix(cells));
        payload.push(0);
        payload.extend_from_slice(&matrix(cells));
        payload
    }

    fn push_indexed_header(bytes: &mut Vec<u8>, class_tag: u32, record_index: u32) {
        let class_tag = class_tag.to_string();
        assert_eq!(class_tag.len(), 3);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag.as_bytes());
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn put_reference(bytes: &mut [u8], at: usize, target: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 9].copy_from_slice(&u64::from(target).to_le_bytes());
        bytes[at + 9..at + 11].copy_from_slice(&[0, 0]);
    }

    fn push_reference(bytes: &mut Vec<u8>, target: u32) {
        let at = bytes.len();
        bytes.resize(at + SAME_SEGMENT_REFERENCE_BYTES, 0);
        put_reference(bytes, at, target);
    }

    fn push_ascii(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(
            &u32::try_from(value.len())
                .expect("test ASCII length")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    }

    fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
        let encoded = value.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(
            &u32::try_from(encoded.len())
                .expect("test UTF-16 length")
                .to_le_bytes(),
        );
        bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    }

    fn mesh_entry_record(
        class_tag: u32,
        record_index: u32,
        guid_record_index: u32,
        entry_name: &str,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 10]);
        push_reference(&mut bytes, guid_record_index);
        push_utf16(&mut bytes, entry_name);
        bytes
    }

    fn mesh_guid_record(
        class_tag: u32,
        record_index: u32,
        entry_name_record_index: u32,
        fusion_uuid: &str,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.extend_from_slice(&[0; 21]);
        push_ascii(&mut bytes, fusion_uuid);
        push_reference(&mut bytes, entry_name_record_index);
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    fn mesh_body_record(
        class_tag: u32,
        record_index: u32,
        guid_record_index: u32,
        scope_record_index: u32,
        decoy_scope_record_index: u32,
    ) -> Vec<u8> {
        let identity = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let mut bytes = Vec::new();
        push_indexed_header(&mut bytes, class_tag, record_index);
        bytes.resize(600, 0);
        bytes[MESH_BODY_FIRST_MATRIX_AT..MESH_BODY_FIRST_MATRIX_AT + MATRIX_BYTES]
            .copy_from_slice(&matrix(identity));
        bytes[MESH_BODY_SECOND_MATRIX_AT..MESH_BODY_SECOND_MATRIX_AT + MATRIX_BYTES]
            .copy_from_slice(&matrix(identity));
        put_reference(&mut bytes, MESH_BODY_SCOPE_REFERENCE_AT, scope_record_index);
        put_reference(&mut bytes, MESH_BODY_GUID_REFERENCE_AT, guid_record_index);
        put_reference(&mut bytes, 560, decoy_scope_record_index);
        let tail_at = bytes.len() - SAME_SEGMENT_REFERENCE_BYTES;
        put_reference(&mut bytes, tail_at, decoy_scope_record_index);
        bytes
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
            bulk_offset: u64::try_from(bulk_offset).expect("test bulk offset"),
        }
    }

    #[test]
    fn typed_mesh_join_ignores_unregistered_collision_chain() {
        const ENTRY_NAME: &str = "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh";
        const FUSION_UUID: &str = "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
        const UNKNOWN_TYPE_GUID: &str = "00000000-0000-0000-0000-000000000001";

        let mut bytes = Vec::new();
        let mut records = Vec::new();
        for (entity_id, frame) in [
            (100, mesh_guid_record(256, 100, 101, FUSION_UUID)),
            (101, mesh_entry_record(257, 101, 100, ENTRY_NAME)),
            (102, mesh_body_record(258, 102, 100, 999, 998)),
            (210, mesh_guid_record(260, 210, 211, FUSION_UUID)),
            (211, mesh_entry_record(259, 211, 210, ENTRY_NAME)),
            (212, mesh_body_record(261, 212, 210, 208, 777)),
        ] {
            records.push(primary_record(entity_id, bytes.len()));
            bytes.extend_from_slice(&frame);
        }
        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(UNKNOWN_TYPE_GUID, None, 0, "", vec![100]),
                design_type(UNKNOWN_TYPE_GUID, None, 0, "", vec![101]),
                design_type(UNKNOWN_TYPE_GUID, None, 0, "", vec![102]),
                design_type(
                    MESH_ENTRY_NAME_TYPE_GUID,
                    Some(MESH_ENTRY_NAME_BASE_TYPE_GUID),
                    MESH_ENTRY_NAME_TYPE_VERSION,
                    PARAMESH_MODULE,
                    vec![211],
                ),
                design_type(
                    MESH_GUID_TYPE_GUID,
                    Some(MESH_GUID_BASE_TYPE_GUID),
                    MESH_GUID_TYPE_VERSION,
                    PARAMESH_MODULE,
                    vec![210],
                ),
                design_type(
                    MESH_BODY_TYPE_GUID,
                    Some(MESH_BODY_BASE_TYPE_GUID),
                    MESH_BODY_TYPE_VERSION,
                    PARAMESH_MODULE,
                    vec![212],
                ),
            ],
            records,
        };

        let design = parse_mesh_design_records(&bytes, &meta, "design-stream".into())
            .expect("typed mesh records");
        assert_eq!(design.entry_names.len(), 1);
        assert_eq!(design.guids.len(), 1);
        assert_eq!(design.bodies.len(), 1);
        let valid_body_offset =
            usize::try_from(meta.records[5].bulk_offset).expect("test body offset");
        let designs = [design];
        let (joined_design, joined_body) =
            resolve_mesh_body(&designs, ENTRY_NAME, FUSION_UUID).expect("exact mesh join");
        assert_eq!(joined_design.stream, "design-stream");
        assert_eq!(joined_body.byte_offset, valid_body_offset);
        assert_eq!(joined_body.guid_record_index, 210);
        assert_eq!(joined_body.scope_record_index, 208);
    }

    #[test]
    fn mesh_join_rejects_multiple_typed_body_candidates() {
        let mut design = MeshDesignRecords {
            stream: "design-stream".into(),
            entry_names: vec![MeshEntryNameRecord {
                record_index: 11,
                guid_record_index: 10,
                entry_name: "mesh.paramesh".into(),
            }],
            guids: vec![MeshGuidRecord {
                record_index: 10,
                entry_name_record_index: 11,
                fusion_uuid: "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".into(),
            }],
            bodies: vec![MeshBodyRecord {
                byte_offset: 100,
                guid_record_index: 10,
                scope_record_index: 12,
                transform: MeshAffineTransform([0.0; 16]),
            }],
        };
        design.bodies.push(MeshBodyRecord {
            byte_offset: 200,
            ..design.bodies[0]
        });

        assert!(resolve_mesh_body(
            &[design],
            "mesh.paramesh",
            "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE"
        )
        .is_none());
    }

    #[test]
    fn mesh_body_transform_applies_nonuniform_scale_and_translation() {
        let cells = [
            0.175, 0.0, 0.0, 0.4, 0.0, 0.06, 0.0, 0.7, 0.0, 0.0, 0.125, 0.3, 0.0, 0.0, 0.0, 1.0,
        ];
        let transform = mesh_body_transform(&mesh_body_payload(cells)).expect("affine map");
        assert_eq!(
            transform.transform([2.0, 5.0, 8.0]),
            cadmpeg_ir::math::Point3::new(7.5, 10.0, 13.0)
        );
    }

    #[test]
    fn mesh_body_transform_refuses_mismatched_projective_and_reflected_pairs() {
        let identity = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let mut mismatched = mesh_body_payload(identity);
        mismatched[MESH_BODY_SECOND_MATRIX_AT..MESH_BODY_SECOND_MATRIX_AT + 8]
            .copy_from_slice(&2.0f64.to_le_bytes());
        assert!(mesh_body_transform(&mismatched).is_none());

        let mut projective = identity;
        projective[12] = 1.0;
        assert!(mesh_body_transform(&mesh_body_payload(projective)).is_none());

        let mut reflected = identity;
        reflected[0] = -1.0;
        assert!(mesh_body_transform(&mesh_body_payload(reflected)).is_none());
    }
}
