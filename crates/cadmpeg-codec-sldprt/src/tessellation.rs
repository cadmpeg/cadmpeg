// SPDX-License-Identifier: Apache-2.0
//! `DisplayLists` descriptor tables.

use crate::container::{ContainerScan, Section};
use cadmpeg_ir::le::u32_at as u32_le;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::TessellationChannel;

#[derive(Debug, Clone, Copy, Default)]
pub struct Summary {
    pub vertices: usize,
    pub triangles: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub vertices: Vec<Point3>,
    pub triangles: Vec<[u32; 3]>,
    pub strip_lengths: Vec<u32>,
    pub normals: Vec<Vector3>,
    pub channels: Vec<TessellationChannel>,
}

fn parse_table(bytes: &[u8], mut at: usize) -> Option<(Mesh, usize)> {
    let mut strips = Vec::new();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut channels = Vec::new();
    for index in 0..6 {
        let item_size = u32_le(bytes, at)? as usize;
        let kind = u32_le(bytes, at + 4)?;
        let flags = u32_le(bytes, at + 8)?;
        let count = u32_le(bytes, at + 12)? as usize;
        let data = at + 16;
        let end = data.checked_add(item_size.checked_mul(count)?)?;
        if end > bytes.len() {
            return None;
        }
        channels.push(TessellationChannel {
            item_size: item_size as u32,
            kind,
            flags,
            count: count as u32,
            data: bytes[data..end].to_vec(),
        });
        if index == 0 && item_size == 4 && kind == 8 {
            strips = (0..count)
                .map(|i| u32_le(bytes, data + i * 4).map(|v| v as usize))
                .collect::<Option<Vec<_>>>()?;
        } else if index == 1 && item_size == 12 && kind == 100 {
            for i in 0..count {
                let p = data + i * 12;
                let read = |at| {
                    bytes
                        .get(at..at + 4)
                        .map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]) as f64)
                        .filter(|value| value.is_finite())
                };
                vertices.push(Point3::new(
                    read(p)? * 1000.0,
                    read(p + 4)? * 1000.0,
                    read(p + 8)? * 1000.0,
                ));
            }
        } else if index == 2 && item_size == 12 && kind == 100 {
            for i in 0..count {
                let p = data + i * 12;
                let read = |at| {
                    bytes
                        .get(at..at + 4)
                        .map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]) as f64)
                        .filter(|value| value.is_finite())
                };
                normals.push(Vector3::new(read(p)?, read(p + 4)?, read(p + 8)?));
            }
        }
        at = end;
    }
    let vertex_count = strips
        .iter()
        .try_fold(0usize, |total, length| total.checked_add(*length))?;
    if strips.is_empty()
        || vertices.is_empty()
        || vertex_count != vertices.len()
        || !normals.is_empty() && normals.len() != vertices.len()
    {
        return None;
    }
    let mut triangles = Vec::new();
    let mut base = 0usize;
    for length in &strips {
        for i in 0..length.saturating_sub(2) {
            let [a, b, c] = if i % 2 == 0 {
                [base + i, base + i + 1, base + i + 2]
            } else {
                [base + i, base + i + 2, base + i + 1]
            };
            triangles.push([
                u32::try_from(a).ok()?,
                u32::try_from(b).ok()?,
                u32::try_from(c).ok()?,
            ]);
        }
        base = base.checked_add(*length)?;
    }
    Some((
        Mesh {
            vertices,
            triangles,
            strip_lengths: strips.into_iter().map(|length| length as u32).collect(),
            normals,
            channels,
        },
        at,
    ))
}

pub fn section_meshes(section: Section<'_>) -> Vec<Mesh> {
    const MARKER: &[u8] = b"uoTempFaceTessData_c";
    let payload = section.payload();
    let Some(marker) = payload.windows(MARKER.len()).position(|w| w == MARKER) else {
        return Vec::new();
    };
    let end = marker + MARKER.len();
    for relative in [8usize, 40] {
        if let Some((mesh, mut at)) = parse_table(payload, end + relative) {
            if !mesh.vertices.is_empty() {
                let mut meshes = vec![mesh];
                while at + 16 <= payload.len() {
                    let Some(relative) = payload[at..].windows(4).position(|w| w == [4, 0, 0, 0])
                    else {
                        break;
                    };
                    at += relative;
                    if let Some((next, end)) = parse_table(payload, at) {
                        if !next.vertices.is_empty() {
                            meshes.push(next);
                        }
                        at = end;
                    } else {
                        at += 4;
                    }
                }
                return meshes;
            }
        }
    }
    Vec::new()
}

pub fn section_summary(section: Section<'_>) -> Option<Summary> {
    let meshes = section_meshes(section);
    (!meshes.is_empty()).then(|| Summary {
        vertices: meshes.iter().map(|mesh| mesh.vertices.len()).sum(),
        triangles: meshes.iter().map(|mesh| mesh.triangles.len()).sum(),
    })
}

pub fn summary(scan: &ContainerScan) -> Summary {
    scan.sections()
        .filter_map(section_summary)
        .fold(Summary::default(), |mut total, next| {
            total.vertices += next.vertices;
            total.triangles += next.triangles;
            total
        })
}
