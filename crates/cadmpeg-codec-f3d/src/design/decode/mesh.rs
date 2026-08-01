// SPDX-License-Identifier: Apache-2.0
//! Join mesh-geometry containers to their bodies through the Design segment.
//!
//! A mesh body's geometry lives in a `.paramesh` container ([spec §1.1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#112-mesh-geometry-containers)),
//! and three Design record classes join the container to its body ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)):
//! the entry-name class stores the blob-part entry name, the GUID class stores
//! the GUID the container's protobuf message carries as `fusion_uuid`, and the
//! mesh-body class carries the scale matrix between container coordinates and
//! model centimetres. The two identity records reference each other, and the
//! body record references the GUID record.

use crate::container::{role, ContainerScan};
use crate::design::decode::sketch::{indexed_record_index, next_indexed_record_offset};
use crate::ids;
use crate::paramesh::{decode_mesh_container, MeshContainer};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::le::f64_at;

/// Row-major 4x4 f64 matrix byte length.
const MATRIX_BYTES: usize = 128;

/// One mesh body's geometry, in model millimetres.
pub struct MeshBody {
    /// Deterministic native identifier, keyed by the mesh-body record.
    pub id: String,
    /// Vertex positions in model millimetres.
    pub vertices: Vec<cadmpeg_ir::math::Point3>,
    /// Triangle corner indices into `vertices`.
    pub triangles: Vec<[u32; 3]>,
}

/// The uniform scale a 4x4 row-major diagonal matrix applies, when every
/// off-diagonal cell is zero, the three leading diagonal cells are equal,
/// positive, and finite, and the last diagonal cell is one.
fn diagonal_scale(bytes: &[u8], at: usize) -> Option<f64> {
    let mut scale = None;
    for row in 0..4 {
        for column in 0..4 {
            let value = f64_at(bytes, at + (row * 4 + column) * 8)?;
            if !value.is_finite() {
                return None;
            }
            if row != column {
                (value == 0.0).then_some(())?;
            } else if row == 3 {
                (value == 1.0).then_some(())?;
            } else if let Some(scale) = scale {
                (value == scale).then_some(())?;
            } else {
                (value > 0.0).then_some(())?;
                scale = Some(value);
            }
        }
    }
    scale
}

/// The scale a mesh-body record stores: the first of its two scale matrices.
/// The second follows one byte after the first and carries the same scale.
fn mesh_body_scale(payload: &[u8]) -> Option<f64> {
    (0..payload.len().saturating_sub(MATRIX_BYTES * 2)).find_map(|at| {
        let scale = diagonal_scale(payload, at)?;
        (diagonal_scale(payload, at + MATRIX_BYTES + 1)? == scale).then_some(scale)
    })
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
pub fn decode_mesh_bodies(scan: &ContainerScan) -> Result<Vec<MeshBody>, CodecError> {
    // A container this decoder cannot read leaves its body without geometry,
    // which the caller reports against the container count. It does not fail
    // the document: every other body still decodes.
    let containers = scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::PARAMESH)
        .filter_map(|entry| {
            let container = decode_mesh_container(scan.entry_bytes(&entry.name).ok()?).ok()?;
            Some((entry.name.clone(), container))
        })
        .collect::<Vec<(String, MeshContainer)>>();
    if containers.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = indexed_records(bytes);
        for (name, container) in &containers {
            let base = name.rsplit('/').next().unwrap_or(name);
            // The GUID record stores the container's `fusion_uuid`, and the
            // entry-name record stores the blob-part entry name and references
            // the GUID record. Both identities come from the container, so a
            // record carrying one names that container and no other.
            let guid = lp_ascii_bytes(&container.fusion_uuid);
            let Some(guid_record) = records
                .iter()
                .find(|record| contains(record.payload, &guid))
                .map(|record| record.index)
            else {
                continue;
            };
            let reference = same_segment_reference(guid_record);
            let entry_name = lp_utf16_bytes(base);
            if !records.iter().any(|record| {
                contains(record.payload, &entry_name) && contains(record.payload, &reference)
            }) {
                continue;
            }
            // The body record is the one that names the GUID record and stores
            // the scale matrices; the entry-name record names it and stores no
            // matrix.
            let Some((body, scale)) = records.iter().find_map(|record| {
                contains(record.payload, &reference)
                    .then(|| mesh_body_scale(record.payload))
                    .flatten()
                    .map(|scale| (record, scale))
            }) else {
                continue;
            };
            // Container coordinates scale to model centimetres, and the IR
            // carries model lengths in millimetres.
            let scale = scale * crate::nurbs::reader::LEN_TO_MM;
            out.push(MeshBody {
                id: ids::native_mesh_body_id(&entry.name, body.offset),
                vertices: container
                    .vertices
                    .iter()
                    .map(|point| {
                        cadmpeg_ir::math::Point3::new(
                            point[0] * scale,
                            point[1] * scale,
                            point[2] * scale,
                        )
                    })
                    .collect(),
                triangles: container.triangles.clone(),
            });
        }
    }
    Ok(out)
}

/// Whether `haystack` holds `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(haystack, needle).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row-major 4x4 diagonal matrix.
    fn matrix(scale: f64, last: f64) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(MATRIX_BYTES);
        for row in 0..4 {
            for column in 0..4 {
                let value = match (row == column, row) {
                    (false, _) => 0.0,
                    (true, 3) => last,
                    (true, _) => scale,
                };
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    /// The scale comes from the matrix the record stores, not from a constant.
    #[test]
    fn mesh_body_scale_reads_the_stored_diagonal() {
        let mut payload = vec![0u8; 11];
        payload.extend_from_slice(&matrix(0.25, 1.0));
        payload.push(0);
        payload.extend_from_slice(&matrix(0.25, 1.0));
        payload.extend_from_slice(&[0; 16]);
        assert_eq!(mesh_body_scale(&payload), Some(0.25));
    }

    /// A record carrying one matrix, a matrix whose off-diagonal cells are not
    /// zero, or two matrices of different scales is not a mesh-body record.
    #[test]
    fn mesh_body_scale_refuses_records_without_a_matrix_pair() {
        let single = matrix(0.1, 1.0);
        assert_eq!(mesh_body_scale(&single), None);

        let mut mismatched = matrix(0.1, 1.0);
        mismatched.push(0);
        mismatched.extend_from_slice(&matrix(0.2, 1.0));
        assert_eq!(mesh_body_scale(&mismatched), None);

        let mut sheared = matrix(0.1, 1.0);
        sheared[8..16].copy_from_slice(&1.0f64.to_le_bytes());
        sheared.push(0);
        sheared.extend_from_slice(&matrix(0.1, 1.0));
        assert_eq!(mesh_body_scale(&sheared), None);

        let mut unnormalized = matrix(0.1, 2.0);
        unnormalized.push(0);
        unnormalized.extend_from_slice(&matrix(0.1, 2.0));
        assert_eq!(mesh_body_scale(&unnormalized), None);
    }
}
