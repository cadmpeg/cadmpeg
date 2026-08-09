// SPDX-License-Identifier: Apache-2.0
//! `DisplayLists` descriptor tables.

use crate::container::{ContainerScan, Section};
use cadmpeg_core::le::u32_at as u32_le;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::TessellationChannel;
use std::collections::HashMap;

const CLASS_MARKER: &[u8] = &[0xff, 0xff, 0x01, 0x00];
const SCENE_SOURCE_MARKER: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0x40, 0x00, 0x00, 0x00, 0x00,
];

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

#[derive(Debug, Clone, Default)]
pub(crate) struct SceneFeatureClasses {
    pub(crate) by_source: HashMap<String, String>,
}

fn scene_classes(payload: &[u8]) -> Vec<(u32, String)> {
    let declarations = payload
        .windows(CLASS_MARKER.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == CLASS_MARKER).then_some(offset))
        .filter_map(|offset| {
            let length = usize::from(u16::from_le_bytes(
                payload.get(offset + 4..offset + 6)?.try_into().ok()?,
            ));
            if !(1..=128).contains(&length) {
                return None;
            }
            let name = payload.get(offset + 6..offset + 6 + length)?;
            if !name
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return None;
            }
            let name = std::str::from_utf8(name).ok()?;
            Some((offset, name.to_string()))
        })
        .collect::<Vec<_>>();
    declarations
        .iter()
        .enumerate()
        .flat_map(|(index, (offset, class))| {
            let role = crate::classification::native_object_class(class).tree_node;
            if !matches!(
                role,
                Some(
                    cadmpeg_ir::features::FeatureTreeNodeRole::AmbientLight
                        | cadmpeg_ir::features::FeatureTreeNodeRole::DirectionalLight
                        | cadmpeg_ir::features::FeatureTreeNodeRole::PointLight
                        | cadmpeg_ir::features::FeatureTreeNodeRole::SpotLight
                )
            ) {
                return Vec::new();
            }
            let start = offset + 6 + class.len();
            let end = declarations
                .get(index + 1)
                .map_or(payload.len(), |(offset, _)| *offset);
            let records = &payload[start..end];
            records
                .windows(SCENE_SOURCE_MARKER.len() + 4)
                .filter_map(|window| {
                    (window.starts_with(SCENE_SOURCE_MARKER))
                        .then(|| u32_le(window, 12))
                        .flatten()
                        .filter(|source| *source != 0)
                        .map(|source| (source, class.clone()))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn scene_feature_classes(scan: &ContainerScan) -> SceneFeatureClasses {
    let mut candidates = HashMap::<u32, Option<String>>::new();
    for section in scan.sections() {
        for (source, class) in scene_classes(section.payload()) {
            candidates
                .entry(source)
                .and_modify(|existing| {
                    if existing.as_deref() != Some(class.as_str()) {
                        *existing = None;
                    }
                })
                .or_insert_with(|| Some(class));
        }
    }
    SceneFeatureClasses {
        by_source: candidates
            .into_iter()
            .filter_map(|(source, class)| class.map(|class| (source.to_string(), class)))
            .collect(),
    }
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
            domain: cadmpeg_ir::tessellation::TessellationChannelDomain::default(),
            item_size: item_size as u32,
            kind,
            flags,
            count: count as u32,
            data: bytes[data..end].to_vec(),
            indices: Vec::new(),
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
    parse_table_sequence(payload, end + descriptor_table_offset(payload, end)).unwrap_or_default()
}

/// Offset of the first descriptor after a face-tessellation class name.
///
/// Both forms begin with two u32 cells. The extended form then carries a
/// fixed 32-byte extension. A compact table begins with item
/// size 4 at the same position, so it cannot satisfy the extension grammar.
fn descriptor_table_offset(payload: &[u8], at: usize) -> usize {
    let extended = u32_le(payload, at + 8) == Some(1)
        && u32_le(payload, at + 12) == Some(0)
        && u32_le(payload, at + 16) == Some(0)
        && u32_le(payload, at + 20).is_some_and(|token| token != 0)
        && payload
            .get(at + 24..at + 40)
            .is_some_and(|tail| tail.iter().all(|byte| *byte == 0));
    if extended {
        40
    } else {
        8
    }
}

fn parse_table_sequence(payload: &[u8], at: usize) -> Option<Vec<Mesh>> {
    let (mesh, mut at) = parse_table(payload, at)?;
    if mesh.vertices.is_empty() {
        return None;
    }
    let mut meshes = vec![mesh];
    while at + 16 <= payload.len() {
        let Some(relative) = payload[at..]
            .windows(4)
            .position(|window| window == [4, 0, 0, 0])
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
    Some(meshes)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(item_size: u32, kind: u32, count: u32, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(item_size.to_le_bytes());
        out.extend(kind.to_le_bytes());
        out.extend(2_u32.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend(data);
        out
    }

    fn table() -> Vec<u8> {
        let mut out = descriptor(4, 8, 1, &3_u32.to_le_bytes());
        let positions = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        out.extend(descriptor(12, 100, 3, &positions));
        out.extend(descriptor(12, 100, 3, &[0; 36]));
        for _ in 0..3 {
            out.extend(descriptor(1, 8, 0, &[]));
        }
        out
    }

    fn class(payload: &mut Vec<u8>, name: &str, sources: &[u32]) {
        payload.extend_from_slice(CLASS_MARKER);
        payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
        payload.extend_from_slice(name.as_bytes());
        for source in sources {
            payload.extend_from_slice(SCENE_SOURCE_MARKER);
            payload.extend_from_slice(&source.to_le_bytes());
        }
    }

    #[test]
    fn scene_objects_carry_history_source_identity() {
        let mut payload = Vec::new();
        class(&mut payload, "moAmbientLight_c", &[12]);
        class(&mut payload, "moDirectionLight_c", &[30, 32]);
        class(&mut payload, "moVisualProperties_c", &[99]);
        class(&mut payload, "moPointLight_c", &[21]);
        class(&mut payload, "moSpotLight_c", &[20]);

        assert_eq!(
            scene_classes(&payload),
            vec![
                (12, "moAmbientLight_c".into()),
                (30, "moDirectionLight_c".into()),
                (32, "moDirectionLight_c".into()),
                (21, "moPointLight_c".into()),
                (20, "moSpotLight_c".into()),
            ]
        );
    }

    #[test]
    fn anonymous_scene_object_counts_do_not_create_source_bindings() {
        let mut payload = Vec::new();
        payload.extend_from_slice(CLASS_MARKER);
        let class = b"moDirectionLight_c";
        payload.extend_from_slice(&(class.len() as u16).to_le_bytes());
        payload.extend_from_slice(class);
        for name in ["UnNamed", "Another"] {
            payload.extend_from_slice(&1_u32.to_le_bytes());
            payload.extend_from_slice(&[0xff, 0xfe, 0xff, 7]);
            for byte in name.bytes() {
                payload.extend_from_slice(&[byte, 0]);
            }
            payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
        }

        assert!(scene_classes(&payload).is_empty());
    }

    #[test]
    fn compact_face_tessellation_header_places_table_at_plus_8() {
        let mut payload = Vec::new();
        payload.extend(2_u32.to_le_bytes());
        payload.extend(1_u32.to_le_bytes());
        payload.extend(table());
        assert_eq!(descriptor_table_offset(&payload, 0), 8);
        assert!(parse_table_sequence(&payload, 8).is_some());
    }

    #[test]
    fn extended_face_tessellation_header_places_table_at_plus_40() {
        let mut payload = Vec::new();
        for word in [2_u32, 1, 1, 0, 0, 0x0020_1296, 0, 0, 0, 0] {
            payload.extend(word.to_le_bytes());
        }
        payload.extend(table());
        assert_eq!(descriptor_table_offset(&payload, 0), 40);
        assert!(parse_table_sequence(&payload, 40).is_some());
    }

    #[test]
    fn incomplete_extended_header_does_not_shift_the_table() {
        let mut payload = Vec::new();
        for word in [2_u32, 1, 1, 0, 0, 0, 0, 0, 0, 0] {
            payload.extend(word.to_le_bytes());
        }
        payload.extend(table());
        assert_eq!(descriptor_table_offset(&payload, 0), 8);
    }
}
