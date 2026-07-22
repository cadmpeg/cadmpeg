// SPDX-License-Identifier: Apache-2.0
//! `DisplayLists` descriptor tables.

use crate::container::{ContainerScan, Section};
use cadmpeg_ir::le::u32_at as u32_le;
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
    pub(crate) anonymous_counts: HashMap<String, usize>,
}

fn legacy_scene_object_count(payload: &[u8]) -> usize {
    payload
        .windows(25)
        .filter(|window| {
            window[..8] == [1, 0, 0, 0, 0xff, 0xfe, 0xff, 7]
                && window[9..22].iter().step_by(2).all(|byte| *byte == 0)
                && window[22..25] == [0xff, 0xfe, 0xff]
        })
        .count()
}

type SceneClasses = (Vec<(u32, String)>, Vec<(String, usize)>);

fn scene_classes(payload: &[u8]) -> SceneClasses {
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
    let mut anonymous = Vec::new();
    let sourced = declarations
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
            let start = offset + CLASS_MARKER.len();
            let end = declarations
                .get(index + 1)
                .map_or(payload.len(), |(offset, _)| *offset);
            let records = &payload[start..end];
            let sourced = records
                .windows(SCENE_SOURCE_MARKER.len() + 4)
                .filter_map(|window| {
                    (window.starts_with(SCENE_SOURCE_MARKER))
                        .then(|| u32_le(window, 12))
                        .flatten()
                        .filter(|source| *source != 0)
                        .map(|source| (source, class.clone()))
                })
                .collect::<Vec<_>>();
            if sourced.is_empty() {
                let count = legacy_scene_object_count(records);
                if count > 0 {
                    anonymous.push((class.clone(), count));
                }
            }
            sourced
        })
        .collect();
    (sourced, anonymous)
}

pub(crate) fn scene_feature_classes(scan: &ContainerScan) -> SceneFeatureClasses {
    let mut candidates = HashMap::<u32, Option<String>>::new();
    let mut anonymous = HashMap::<String, Option<usize>>::new();
    for section in scan.sections() {
        let (sourced, counts) = scene_classes(section.payload());
        for (source, class) in sourced {
            candidates
                .entry(source)
                .and_modify(|existing| {
                    if existing.as_deref() != Some(class.as_str()) {
                        *existing = None;
                    }
                })
                .or_insert_with(|| Some(class));
        }
        for (class, count) in counts {
            anonymous
                .entry(class)
                .and_modify(|existing| {
                    if *existing != Some(count) {
                        *existing = None;
                    }
                })
                .or_insert(Some(count));
        }
    }
    SceneFeatureClasses {
        by_source: candidates
            .into_iter()
            .filter_map(|(source, class)| class.map(|class| (source.to_string(), class)))
            .collect(),
        anonymous_counts: anonymous
            .into_iter()
            .filter_map(|(class, count)| count.map(|count| (class, count)))
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

#[cfg(test)]
mod tests {
    use super::*;

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
            scene_classes(&payload).0,
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
    fn legacy_scene_objects_are_counted_from_record_framing() {
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

        assert_eq!(
            scene_classes(&payload).1,
            vec![("moDirectionLight_c".into(), 2)]
        );
    }
}
