// SPDX-License-Identifier: Apache-2.0
//! Join mesh-geometry containers to their bodies through the Design segment.
//!
//! A mesh body's geometry lives in a `.paramesh` container ([spec §1.1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#112-mesh-geometry-containers)),
//! and three Design record classes join the container to its body ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)):
//! the entry-name class stores the blob-part entry name, the GUID class stores
//! the GUID the container's protobuf message carries as `fusion_uuid`, and the
//! mesh-body class carries the affine map between container coordinates and
//! model centimetres. The two identity records reference each other, and the
//! body record references the GUID record.

use crate::container::{role, ContainerScan};
use crate::design::decode::sketch::{indexed_record_index, next_indexed_record_offset};
use crate::ids;
use crate::paramesh::decode_mesh_container;
use cadmpeg_codec_core::le::f64_at;
use cadmpeg_codec_core::CodecError;

/// Row-major 4x4 f64 matrix byte length.
const MATRIX_BYTES: usize = 128;
const MESH_BODY_FIRST_MATRIX_AT: usize = 42;
const MESH_BODY_MATRIX_SEPARATOR_BYTES: usize = 1;
const MESH_BODY_SECOND_MATRIX_AT: usize =
    MESH_BODY_FIRST_MATRIX_AT + MATRIX_BYTES + MESH_BODY_MATRIX_SEPARATOR_BYTES;

/// One mesh body's geometry, in model millimetres.
pub struct MeshBody {
    /// Deterministic native identifier, keyed by the mesh-body record.
    pub id: String,
    /// Vertex positions in model millimetres.
    pub vertices: Vec<cadmpeg_ir::math::Point3>,
    /// Triangle corner indices into `vertices`.
    pub triangles: Vec<[u32; 3]>,
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
                * crate::nurbs::reader::LEN_TO_MM,
            (cells[4] * x + cells[5] * y + cells[6] * z + cells[7])
                * crate::nurbs::reader::LEN_TO_MM,
            (cells[8] * x + cells[9] * y + cells[10] * z + cells[11])
                * crate::nurbs::reader::LEN_TO_MM,
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
pub enum MeshContainerOutcome {
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

/// The eleven bytes of a same-segment reference naming `entity`.
fn same_segment_reference(entity: u32) -> Vec<u8> {
    let mut bytes = vec![1u8];
    bytes.extend_from_slice(&u64::from(entity).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

/// A u32-count length-prefixed ASCII string as it appears in a record.
fn lp_ascii_bytes(value: &str) -> Vec<u8> {
    let mut bytes = (value.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(value.as_bytes());
    bytes
}

/// A u32-count length-prefixed UTF-16LE string as it appears in a record.
fn lp_utf16_bytes(value: &str) -> Vec<u8> {
    let encoded = value.encode_utf16().collect::<Vec<_>>();
    let mut bytes = (encoded.len() as u32).to_le_bytes().to_vec();
    bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    bytes
}

/// One framed record of a Design `BulkStream`.
struct IndexedRecord<'a> {
    index: u32,
    offset: usize,
    payload: &'a [u8],
}

/// Frame every indexed record of a Design `BulkStream`.
fn indexed_records(bytes: &[u8]) -> Vec<IndexedRecord<'_>> {
    let mut records = Vec::new();
    let mut at = 0usize;
    while let Some(offset) = next_indexed_record_offset(bytes, at) {
        let Some(index) = indexed_record_index(bytes, offset) else {
            break;
        };
        let end = next_indexed_record_offset(bytes, offset + 7).unwrap_or(bytes.len());
        let Some(payload) = bytes.get(offset..end) else {
            break;
        };
        records.push(IndexedRecord {
            index,
            offset,
            payload,
        });
        at = end;
    }
    records
}

/// Decode every mesh body: one per `.paramesh` container joined to the
/// mesh-body record that names its GUID record.
pub fn decode_mesh_bodies(scan: &ContainerScan) -> Result<Vec<MeshContainerOutcome>, CodecError> {
    let design_streams = scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
        .map(|entry| scan.entry_bytes(&entry.name).map(indexed_records))
        .collect::<Result<Vec<_>, _>>()?;
    let records = design_streams.iter().flatten().collect::<Vec<_>>();
    let mut outcomes = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::PARAMESH)
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
        let guid = lp_ascii_bytes(&container.fusion_uuid);
        let joined = records
            .iter()
            .find_map(|guid_record| {
                contains(guid_record.payload, &guid).then_some(guid_record.index)
            })
            .and_then(|guid_record| {
                let reference = same_segment_reference(guid_record);
                let entry_name = lp_utf16_bytes(base);
                records
                    .iter()
                    .any(|record| {
                        contains(record.payload, &entry_name)
                            && contains(record.payload, &reference)
                    })
                    .then_some(reference)
            })
            .and_then(|reference| {
                records.iter().find_map(|record| {
                    contains(record.payload, &reference)
                        .then(|| mesh_body_transform(record.payload))
                        .flatten()
                        .map(|transform| (record, transform))
                })
            });
        let Some((body, transform)) = joined else {
            outcomes.push(MeshContainerOutcome::Unjoined {
                entry_name: entry.name.clone(),
            });
            continue;
        };
        outcomes.push(MeshContainerOutcome::Joined(MeshBody {
            id: ids::native_mesh_body_id(&entry.name, body.offset),
            vertices: container
                .vertices
                .iter()
                .copied()
                .map(|point| transform.transform(point))
                .collect(),
            triangles: container.triangles,
        }));
    }
    Ok(outcomes)
}

/// Whether `haystack` holds `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(haystack, needle).is_some()
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
