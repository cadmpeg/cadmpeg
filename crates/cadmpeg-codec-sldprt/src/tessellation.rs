// SPDX-License-Identifier: Apache-2.0
//! `DisplayLists` descriptor tables.

use crate::brep::PersistentFaceIdentity;
use crate::container::{ContainerScan, Section};
use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::tessellation::TessellationChannel;
use cadmpeg_ir::topology::Sense;
use std::collections::HashMap;

use crate::layout::display_lists_compact_face_header as compact_face;
use crate::layout::display_lists_extended_face_header as extended_face;
use crate::layout::display_lists_scene_source_binding as scene_src;

const CLASS_MARKER: &[u8] = &[0xff, 0xff, 0x01, 0x00];
const SCENE_SOURCE_MARKER: &[u8] = &scene_src::MARKER_VALUE;
const EPS_DISPLAY_QUANTIZATION: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;
const EPS_CYLINDER_ANGLE: f64 = 1.0e-12;
const DISPLAY_QUANTIZATION_ULPS: f64 = 8.0;
const MAX_PLANAR_TRIM_ARC_SEGMENTS: usize = 4096;
const MIN_TESSELLATION_NORMAL_ALIGNMENT: f64 = 1.0 - 1.0e-4;
const FACE_TESSELLATION_CLASS: &[u8] = b"uoTempFaceTessData_c";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ByteRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// One decoded `uoTempFaceTessData_c` descriptor table.
#[derive(Debug, Clone)]
pub(crate) struct DisplayFace {
    pub(crate) mesh: Mesh,
    pub(crate) table_index: usize,
    pub(crate) table: ByteRange,
    pub(crate) metadata: ByteRange,
    pub(crate) surface_references: Vec<PersistentSurfaceReference>,
}

/// One framed persistent-surface reference in a display-face metadata slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PersistentSurfaceReference {
    /// A complete identity whose optional tail is entirely numeric.
    Complete(PersistentFaceIdentity),
    /// A source-level reference whose trailing fields are opaque.
    SourceOnly {
        feature_source_id: u32,
        local_surface_id: u32,
    },
}

impl PersistentSurfaceReference {
    pub(crate) fn feature_source_id(&self) -> u32 {
        match self {
            Self::Complete(identity) => identity.feature_source_id,
            Self::SourceOnly {
                feature_source_id, ..
            } => *feature_source_id,
        }
    }

    fn complete_identity(&self) -> Option<&PersistentFaceIdentity> {
        match self {
            Self::Complete(identity) => Some(identity),
            Self::SourceOnly { .. } => None,
        }
    }
}

/// One `DisplayLists` table whose persistent surface identity can bind a B-rep face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersistentFaceBinding {
    pub(crate) tessellation: String,
    pub(crate) identity: PersistentFaceIdentity,
}

impl DisplayFace {
    /// Return the source ID only when all duplicated references agree.
    pub(crate) fn feature_source_id(&self) -> Option<u32> {
        let mut sources = self
            .surface_references
            .iter()
            .map(PersistentSurfaceReference::feature_source_id);
        let source = sources.next()?;
        sources
            .all(|candidate| candidate == source)
            .then_some(source)
    }

    /// Return the complete identity only when every duplicate reference agrees.
    pub(crate) fn persistent_surface_identity(&self) -> Option<PersistentFaceIdentity> {
        let mut references = self.surface_references.iter();
        let first = references.next()?.complete_identity()?.clone();
        references
            .all(|reference| reference.complete_identity() == Some(&first))
            .then_some(first)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ClassInterval {
    pub(crate) name: String,
    pub(crate) class_offset: usize,
    pub(crate) content: ByteRange,
    source_ids: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SceneFeatureClasses {
    pub(crate) by_source: HashMap<String, String>,
}

pub(crate) fn class_intervals(payload: &[u8]) -> Vec<ClassInterval> {
    let declarations = payload
        .windows(CLASS_MARKER.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == CLASS_MARKER).then_some(offset))
        .filter_map(|offset| {
            let length = usize::from(View::u16_le_at(payload, offset + 4)?);
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
        .map(|(index, (offset, class))| {
            let start = offset + 6 + class.len();
            let end = declarations
                .get(index + 1)
                .map_or(payload.len(), |(offset, _)| *offset);
            let records = &payload[start..end];
            let source_ids = records
                .windows(scene_src::LEN)
                .filter_map(|window| {
                    (window.starts_with(SCENE_SOURCE_MARKER))
                        .then(|| View::u32_le_at(window, scene_src::SOURCE_ID))
                        .flatten()
                        .filter(|source| *source != 0)
                })
                .collect();
            ClassInterval {
                name: class.clone(),
                class_offset: *offset,
                content: ByteRange { start, end },
                source_ids,
            }
        })
        .collect()
}

fn scene_classes(payload: &[u8]) -> Vec<(u32, String)> {
    class_intervals(payload)
        .into_iter()
        .filter(|class| {
            matches!(
                crate::classification::native_object_class(&class.name).tree_node,
                Some(
                    cadmpeg_ir::features::FeatureTreeNodeRole::AmbientLight
                        | cadmpeg_ir::features::FeatureTreeNodeRole::DirectionalLight
                        | cadmpeg_ir::features::FeatureTreeNodeRole::PointLight
                        | cadmpeg_ir::features::FeatureTreeNodeRole::SpotLight
                )
            )
        })
        .flat_map(|class| {
            class
                .source_ids
                .into_iter()
                .map(move |source| (source, class.name.clone()))
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

pub(crate) fn auxiliary_channels_are_consistent(
    strips: &[usize],
    channels: &[TessellationChannel],
) -> bool {
    let [b, c, d] = channels else { return false };
    if (b.item_size, b.kind, b.flags) != (4, 8, 2)
        || (c.item_size, c.kind, c.flags) != (4, 8, 2)
        || (d.item_size, d.kind, d.flags) != (1, 8, 2)
    {
        return false;
    }
    let Some(list_c) = strips
        .iter()
        .map(|length| length.checked_mul(2)?.checked_sub(2))
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let Some(endpoint_count) = list_c
        .iter()
        .try_fold(0usize, |total, count| total.checked_add(*count))
    else {
        return false;
    };
    let stored_list_c = c
        .data
        .chunks_exact(4)
        .map(|bytes| usize::try_from(View::u32_le_at(bytes, 0)?).ok())
        .collect::<Option<Vec<_>>>();
    let counts = (usize::try_from(b.count).ok(), usize::try_from(d.count).ok());
    let payload_lengths = channels.iter().all(|channel| {
        usize::try_from(channel.item_size)
            .ok()
            .and_then(|size| usize::try_from(channel.count).ok()?.checked_mul(size))
            == Some(channel.data.len())
    });
    payload_lengths
        && (counts == (Some(0), Some(0)) || counts == (Some(endpoint_count), Some(endpoint_count)))
        && usize::try_from(c.count).ok() == Some(strips.len())
        && stored_list_c.as_deref() == Some(list_c.as_slice())
        && b.data
            .chunks_exact(4)
            .all(|bytes| View::f32_le_at(bytes, 0).is_some_and(f32::is_finite))
}

fn parse_table(bytes: &[u8], mut at: usize) -> Option<(Mesh, usize)> {
    let mut strips = Vec::new();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut channels = Vec::new();
    for index in 0..6 {
        let item_size = View::u32_le_at(bytes, at)? as usize;
        let kind = View::u32_le_at(bytes, at + 4)?;
        let flags = View::u32_le_at(bytes, at + 8)?;
        let count = View::u32_le_at(bytes, at + 12)? as usize;
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
                .map(|i| View::u32_le_at(bytes, data + i * 4).map(|v| v as usize))
                .collect::<Option<Vec<_>>>()?;
        } else if index == 1 && item_size == 12 && kind == 100 {
            for i in 0..count {
                let p = data + i * 12;
                let read = |at| {
                    View::f32_le_at(bytes, at)
                        .map(f64::from)
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
                    View::f32_le_at(bytes, at)
                        .map(f64::from)
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
    if !matches!(channels.as_slice(), [a, positions, normals, ..]
        if (a.item_size, a.kind, a.flags) == (4, 8, 2)
            && (positions.item_size, positions.kind, positions.flags) == (12, 100, 2)
            && (normals.item_size, normals.kind, normals.flags) == (12, 100, 2))
    {
        return None;
    }
    if strips.is_empty()
        || vertices.is_empty()
        || vertex_count != vertices.len()
        || !normals.is_empty() && normals.len() != vertices.len()
        || !auxiliary_channels_are_consistent(&strips, &channels[3..])
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

pub(crate) fn section_display_faces(section: Section<'_>) -> Vec<DisplayFace> {
    let payload = section.payload();
    let markers = payload
        .windows(FACE_TESSELLATION_CLASS.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == FACE_TESSELLATION_CLASS).then_some(offset))
        .collect::<Vec<_>>();
    let mut faces = Vec::new();
    for (marker_index, marker) in markers.iter().copied().enumerate() {
        let header = marker + FACE_TESSELLATION_CLASS.len();
        let (Some(triangle_count), Some(strip_count)) = (
            View::u32_le_at(payload, header + compact_face::TRIANGLE_COUNT),
            View::u32_le_at(payload, header + compact_face::STRIP_COUNT),
        ) else {
            continue;
        };
        // `uoBodyPropInfo_c` declarations separate body-property groups, but
        // face descriptor tables continue after them without repeating the
        // `uoTempFaceTessData_c` declaration. Only another face declaration
        // starts a new sequence.
        let limit = markers
            .get(marker_index + 1)
            .copied()
            .unwrap_or(payload.len());
        let start = header + descriptor_table_offset(payload, header);
        let Some(tables) = parse_table_sequence(payload, start, limit) else {
            continue;
        };
        if !tables.first().is_some_and(|(_, _, mesh)| {
            usize::try_from(triangle_count).ok() == Some(mesh.triangles.len())
                && usize::try_from(strip_count).ok() == Some(mesh.strip_lengths.len())
        }) {
            continue;
        }
        for (start, end, mesh) in tables {
            faces.push(DisplayFace {
                mesh,
                table_index: 0,
                table: ByteRange { start, end },
                metadata: ByteRange {
                    start: end,
                    end: limit,
                },
                surface_references: Vec::new(),
            });
        }
    }
    faces.sort_by_key(|face| face.table.start);
    for index in 0..faces.len() {
        let metadata_end = faces
            .get(index + 1)
            .map_or(faces[index].metadata.end, |next| next.table.start)
            .min(faces[index].metadata.end);
        faces[index].table_index = index;
        faces[index].metadata.end = metadata_end;
        faces[index].surface_references =
            persistent_surface_references(payload, faces[index].metadata);
    }
    faces
}

pub fn section_meshes(section: Section<'_>) -> Vec<Mesh> {
    section_display_faces(section)
        .into_iter()
        .map(|face| face.mesh)
        .collect()
}

/// Offset of the first descriptor after a face-tessellation class name.
///
/// Both forms begin with two u32 cells. The extended form then carries a
/// fixed 32-byte extension. A compact table begins with item
/// size 4 at the same position, so it cannot satisfy the extension grammar.
fn descriptor_table_offset(payload: &[u8], at: usize) -> usize {
    let extended = View::u32_le_at(payload, at + extended_face::FORM) == Some(1)
        && View::u32_le_at(payload, at + extended_face::ZERO_AT_12) == Some(0)
        && View::u32_le_at(payload, at + extended_face::ZERO_AT_16) == Some(0)
        && View::u32_le_at(payload, at + extended_face::FORM_TOKEN).is_some_and(|token| token != 0)
        && payload
            .get(at + extended_face::ZERO_TAIL..at + extended_face::LEN)
            .is_some_and(|tail| tail.iter().all(|byte| *byte == 0));
    if extended {
        extended_face::LEN
    } else {
        compact_face::LEN
    }
}

fn parse_table_sequence(
    payload: &[u8],
    at: usize,
    limit: usize,
) -> Option<Vec<(usize, usize, Mesh)>> {
    let first_start = at;
    let (mesh, mut at) = parse_table(payload, at)?;
    if at > limit {
        return None;
    }
    if mesh.vertices.is_empty() {
        return None;
    }
    let mut meshes = vec![(first_start, at, mesh)];
    while at + 16 <= limit {
        let Some(relative) = payload[at..limit]
            .windows(4)
            .position(|window| window == [4, 0, 0, 0])
        else {
            break;
        };
        at += relative;
        let start = at;
        if let Some((next, end)) = parse_table(payload, at) {
            if end <= limit && !next.vertices.is_empty() {
                meshes.push((start, end, next));
                at = end;
            } else {
                at += 4;
            }
        } else {
            at += 4;
        }
    }
    Some(meshes)
}

fn persistent_surface_references(
    payload: &[u8],
    range: ByteRange,
) -> Vec<PersistentSurfaceReference> {
    const MARKER: &[u8] = &[0xff, 0xfe, 0xff];
    let mut references = Vec::new();
    let mut at = range.start;
    while at + 4 <= range.end && at + 4 <= payload.len() {
        if payload.get(at..at + MARKER.len()) != Some(MARKER) {
            at += 1;
            continue;
        }
        let count = usize::from(payload[at + 3]);
        let start = at + 4;
        let Some(end) = count
            .checked_mul(2)
            .and_then(|length| start.checked_add(length))
            .filter(|end| *end <= range.end)
        else {
            at += 1;
            continue;
        };
        let Some(raw) = payload.get(start..end) else {
            at += 1;
            continue;
        };
        let Some(units) = raw
            .chunks_exact(2)
            .enumerate()
            .map(|(index, _)| View::u16_le_at(raw, index * 2))
            .collect::<Option<Vec<_>>>()
        else {
            at = end;
            continue;
        };
        let Ok(text) = String::from_utf16(&units) else {
            at = end;
            continue;
        };
        let mut fields = text.trim().split(',');
        let Some(_class_name) = fields
            .next()
            .filter(|name| name.starts_with("mo") && name.ends_with("SurfIdRep_c"))
        else {
            at = end;
            continue;
        };
        let Some(feature_source_id) = fields
            .next()
            .and_then(|field| field.parse::<u32>().ok())
            .filter(|source| *source != 0 && *source != u32::MAX)
        else {
            at = end;
            continue;
        };
        let Some(local_surface_id) = fields.next().and_then(|field| field.parse::<u32>().ok())
        else {
            at = end;
            continue;
        };
        let mut trailing_fields = fields.collect::<Vec<_>>();
        if trailing_fields
            .last()
            .is_some_and(|field| field.trim().is_empty())
        {
            trailing_fields.pop();
        }
        let trailing_fields = trailing_fields
            .into_iter()
            .map(|field| {
                field
                    .parse::<i32>()
                    .ok()
                    .map(|value| u32::from_ne_bytes(value.to_ne_bytes()))
            })
            .collect::<Option<Vec<_>>>();
        references.push(match trailing_fields {
            Some(trailing_fields) => PersistentSurfaceReference::Complete(PersistentFaceIdentity {
                feature_source_id,
                local_id: local_surface_id,
                trailing_fields,
            }),
            None => PersistentSurfaceReference::SourceOnly {
                feature_source_id,
                local_surface_id,
            },
        });
        at = end;
    }
    references
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

struct SurfaceCandidate<'a> {
    face: &'a FaceId,
    body: &'a cadmpeg_ir::ids::BodyId,
    surface: &'a SurfaceGeometry,
    tolerance: f64,
    inverse: cadmpeg_ir::transform::Transform,
    trim: Option<AnalyticTrim>,
}

/// Bind a face-tessellation table when its vertices select one surface face.
///
/// Display coordinates are stored as f32, while the B-rep carriers are f64.
/// The relative tolerance below covers that quantization. Complete analytic
/// trims can distinguish faces on a shared analytic carrier. A NURBS support
/// must provide a forward-evaluated parameter witness within the face or
/// display quantization tolerance; an unconstrained nearest-support fit is
/// not an ownership witness.
pub(crate) fn assign_unique_surface_owners(model: &mut cadmpeg_ir::document::Model) -> Vec<String> {
    let surfaces = model
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<HashMap<_, _>>();
    let regions = model
        .regions
        .iter()
        .map(|region| (&region.id, &region.body))
        .collect::<HashMap<_, _>>();
    let shell_bodies = model
        .shells
        .iter()
        .filter_map(|shell| Some((&shell.id, *regions.get(&shell.region)?)))
        .collect::<HashMap<_, _>>();
    let body_transforms = model
        .bodies
        .iter()
        .map(|body| (&body.id, body.transform))
        .collect::<HashMap<_, _>>();
    let loops = model
        .loops
        .iter()
        .map(|loop_| (&loop_.id, loop_))
        .collect::<HashMap<_, _>>();
    let coedges = model
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect::<HashMap<_, _>>();
    let edges = model
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let vertices = model
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, vertex))
        .collect::<HashMap<_, _>>();
    let points = model
        .points
        .iter()
        .map(|point| (&point.id, point.position))
        .collect::<HashMap<_, _>>();
    let curves = model
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<HashMap<_, _>>();
    let candidates = model
        .faces
        .iter()
        .filter_map(|face| {
            let body = *shell_bodies.get(&face.shell)?;
            let inverse = match body_transforms.get(body).copied().flatten() {
                Some(transform) if transform.is_proper_rigid() => transform.try_inverse_affine()?,
                Some(_) => return None,
                None => cadmpeg_ir::transform::Transform::identity(),
            };
            Some(SurfaceCandidate {
                face: &face.id,
                body,
                surface: *surfaces.get(&face.surface)?,
                tolerance: face.tolerance.unwrap_or(0.0),
                inverse,
                trim: analytic_trim(
                    face,
                    *surfaces.get(&face.surface)?,
                    &loops,
                    &coedges,
                    &edges,
                    &vertices,
                    &points,
                    &curves,
                ),
            })
        })
        .collect::<Vec<_>>();

    let mut assigned = Vec::new();
    for mesh in &mut model.tessellations {
        if mesh.body.is_some() || !mesh.faces.is_empty() || mesh.vertices.is_empty() {
            continue;
        }
        let coordinate_scale = mesh
            .vertices
            .iter()
            .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
            .fold(1.0_f64, f64::max);
        let quantization_tolerance =
            coordinate_scale * f64::from(f32::EPSILON) * DISPLAY_QUANTIZATION_ULPS
                + EPS_DISPLAY_QUANTIZATION;
        let mut owners = candidates
            .iter()
            .filter(|candidate| {
                let tolerance = candidate.tolerance.max(quantization_tolerance);
                mesh.vertices.iter().all(|point| {
                    surface_measure(
                        candidate.surface,
                        candidate.inverse.apply_point(*point),
                        Some(tolerance),
                    )
                    .is_some_and(|measure| measure.residual <= tolerance)
                })
            })
            .collect::<Vec<_>>();
        if owners.len() > 1 {
            owners.retain(|candidate| {
                candidate.trim.as_ref().is_none_or(|trim| {
                    trim.contains_mesh(
                        mesh,
                        candidate.inverse,
                        candidate.tolerance.max(quantization_tolerance),
                    )
                })
            });
        }
        let (face, body, chordal_deflection) = match owners.as_slice() {
            [owner] => (owner.face, owner.body, None),
            _ => {
                if owners
                    .iter()
                    .any(|candidate| contains_nurbs_surface(candidate.surface))
                {
                    continue;
                }
                let Some((index, deflection)) =
                    approximate_surface_owner(mesh, &candidates, quantization_tolerance)
                else {
                    continue;
                };
                let owner = &candidates[index];
                (owner.face, owner.body, Some(deflection))
            }
        };
        mesh.faces.push((*face).clone());
        mesh.body = Some((*body).clone());
        if let Some(deflection) = chordal_deflection {
            mesh.chordal_deflection = Some(deflection);
        }
        assigned.push(mesh.id.clone());
    }
    assigned
}

fn approximate_surface_owner(
    mesh: &cadmpeg_ir::tessellation::Tessellation,
    candidates: &[SurfaceCandidate<'_>],
    quantization_tolerance: f64,
) -> Option<(usize, f64)> {
    if mesh.normals.len() != mesh.vertices.len() || mesh.normals.is_empty() {
        return None;
    }
    let mut fits = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let mut max_residual = 0.0_f64;
            for (point, normal) in mesh.vertices.iter().zip(&mesh.normals) {
                let local_point = candidate.inverse.apply_point(*point);
                let measure = surface_measure(candidate.surface, local_point, None)?;
                let residual = measure.residual;
                let surface_normal = measure.normal?;
                let mesh_normal = candidate.inverse.apply_vector(*normal).unit()?;
                if surface_normal.dot(mesh_normal).abs() < MIN_TESSELLATION_NORMAL_ALIGNMENT {
                    return None;
                }
                max_residual = max_residual.max(residual);
            }
            if is_planar_surface(candidate.surface) && max_residual > quantization_tolerance {
                return None;
            }
            Some((index, max_residual))
        })
        .collect::<Vec<_>>();
    if fits.is_empty() {
        return approximate_trimmed_surface_owner(mesh, candidates, quantization_tolerance);
    }
    fits.sort_by(|left, right| left.1.total_cmp(&right.1));
    let best_deflection = fits[0].1;
    fits.retain(|(_, deflection)| *deflection <= best_deflection + quantization_tolerance);
    fits.retain(|(index, _)| {
        candidates[*index].trim.as_ref().is_none_or(|trim| {
            trim.contains_mesh(mesh, candidates[*index].inverse, quantization_tolerance)
        })
    });
    let [(index, deflection), rest @ ..] = fits.as_slice() else {
        return None;
    };
    if rest
        .first()
        .is_some_and(|(_, next, ..)| *next <= *deflection + quantization_tolerance)
    {
        return None;
    }
    Some((*index, *deflection))
}

/// Use a unique analytic trim as an ownership witness when stored display
/// normals are absent or inconsistent. This path requires a bounded trim, so
/// geometric coincidence on an unbounded analytic carrier cannot fabricate an
/// owner without the normal agreement required above.
fn approximate_trimmed_surface_owner(
    mesh: &cadmpeg_ir::tessellation::Tessellation,
    candidates: &[SurfaceCandidate<'_>],
    quantization_tolerance: f64,
) -> Option<(usize, f64)> {
    let mut fits = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let trim = candidate.trim.as_ref()?;
            let mut max_residual = 0.0_f64;
            for point in &mesh.vertices {
                let measure = surface_measure(
                    candidate.surface,
                    candidate.inverse.apply_point(*point),
                    None,
                )?;
                max_residual = max_residual.max(measure.residual);
            }
            if is_planar_surface(candidate.surface) && max_residual > quantization_tolerance {
                return None;
            }
            trim.contains_mesh(mesh, candidate.inverse, quantization_tolerance)
                .then_some((index, max_residual))
        })
        .collect::<Vec<_>>();
    fits.sort_by(|left, right| left.1.total_cmp(&right.1));
    let best_deflection = fits.first()?.1;
    fits.retain(|(_, deflection)| *deflection <= best_deflection + quantization_tolerance);
    let [(index, deflection)] = fits.as_slice() else {
        return None;
    };
    Some((*index, *deflection))
}

fn is_planar_surface(surface: &SurfaceGeometry) -> bool {
    match surface {
        SurfaceGeometry::Plane { .. } => true,
        SurfaceGeometry::Transformed { basis, .. } => is_planar_surface(basis),
        _ => false,
    }
}

fn contains_nurbs_surface(surface: &SurfaceGeometry) -> bool {
    match surface {
        SurfaceGeometry::Nurbs(_) => true,
        SurfaceGeometry::Transformed { basis, .. } => contains_nurbs_surface(basis),
        _ => false,
    }
}

/// Bind `DisplayLists` tables to faces through their complete persistent identity.
///
/// The identity is a source-declared face key, so it is stronger than a
/// geometric coincidence test. A repeated key with different B-rep targets or
/// repeated table IDs with different identities is rejected as ambiguous.
pub(crate) fn assign_persistent_owners(
    model: &mut cadmpeg_ir::document::Model,
    face_identities: &[(String, PersistentFaceIdentity)],
    bindings: &[PersistentFaceBinding],
) -> Vec<String> {
    let mut faces_by_identity = HashMap::<PersistentFaceIdentity, Option<FaceId>>::new();
    for (target, identity) in face_identities {
        let candidate = FaceId(target.clone());
        match faces_by_identity.entry(identity.clone()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(candidate));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry.get().as_ref().is_some_and(|face| face != &candidate) {
                    *entry.get_mut() = None;
                }
            }
        }
    }

    let regions = model
        .regions
        .iter()
        .map(|region| (&region.id, &region.body))
        .collect::<HashMap<_, _>>();
    let shell_bodies = model
        .shells
        .iter()
        .filter_map(|shell| Some((&shell.id, *regions.get(&shell.region)?)))
        .collect::<HashMap<_, _>>();
    let face_bodies = model
        .faces
        .iter()
        .filter_map(|face| Some((face.id.clone(), (*shell_bodies.get(&face.shell)?).clone())))
        .collect::<HashMap<_, _>>();

    let mut bindings_by_mesh = HashMap::<String, Option<PersistentFaceIdentity>>::new();
    for binding in bindings {
        let identity = binding.identity.clone();
        match bindings_by_mesh.entry(binding.tessellation.clone()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(identity));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry.get() != &Some(identity) {
                    *entry.get_mut() = None;
                }
            }
        }
    }

    let mut assigned = Vec::new();
    for mesh in &mut model.tessellations {
        if mesh.body.is_some() || !mesh.faces.is_empty() {
            continue;
        }
        let Some(Some(identity)) = bindings_by_mesh.get(&mesh.id) else {
            continue;
        };
        let Some(Some(face)) = faces_by_identity.get(identity) else {
            continue;
        };
        let Some(body) = face_bodies.get(face) else {
            continue;
        };
        mesh.faces.push(face.clone());
        mesh.body = Some(body.clone());
        assigned.push(mesh.id.clone());
    }
    assigned
}

#[derive(Debug, Clone, Copy)]
struct PlaneFrame {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
    v_axis: Vector3,
}

impl PlaneFrame {
    fn project(self, point: Point3) -> Point2 {
        let delta = point.vector_from(self.origin);
        Point2::new(delta.dot(self.u_axis), delta.dot(self.v_axis))
    }
}

#[derive(Debug, Clone, Copy)]
struct CircularHole {
    center: Point2,
    radius: f64,
}

#[derive(Debug, Clone)]
enum AnalyticTrim {
    Planar(PlanarTrim),
    Cylindrical(CylindricalTrim),
    Conical(ConicalTrim),
}

impl AnalyticTrim {
    fn contains_mesh(
        &self,
        mesh: &cadmpeg_ir::tessellation::Tessellation,
        inverse_body: cadmpeg_ir::transform::Transform,
        tolerance: f64,
    ) -> bool {
        match self {
            Self::Planar(trim) => trim.contains_mesh(mesh, inverse_body, tolerance),
            Self::Cylindrical(trim) => trim.contains_mesh(mesh, inverse_body, tolerance),
            Self::Conical(trim) => trim.contains_mesh(mesh, inverse_body, tolerance),
        }
    }
}

#[derive(Debug, Clone)]
enum PlanarOuter {
    Polygon(Vec<Point2>),
    Circle(CircularHole),
}

#[derive(Debug, Clone)]
enum PlanarHole {
    Polygon {
        boundary: Vec<Point2>,
        triangles: Vec<[Point2; 3]>,
    },
    Circle(CircularHole),
}

impl PlanarHole {
    fn polygon(boundary: Vec<Point2>, tolerance: f64) -> Option<Self> {
        let triangles = triangulate_polygon(&boundary, tolerance)?;
        Some(Self::Polygon {
            boundary,
            triangles,
        })
    }
}

#[derive(Debug, Clone)]
struct PlanarTrim {
    frame: PlaneFrame,
    outer: Option<PlanarOuter>,
    holes: Vec<PlanarHole>,
    boundary_tolerance: f64,
}

enum HoleConstraint<'a> {
    Polygon {
        boundary: &'a [Point2],
        triangles: &'a [[Point2; 3]],
    },
    Circle {
        exclusion: CircularHole,
        boundary: CircularHole,
    },
}

#[derive(Debug, Clone, Copy)]
struct CylindricalTrim {
    origin: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
    min_axial: f64,
    max_axial: f64,
    angular_start: f64,
    angular_span: f64,
}

#[derive(Debug, Clone, Copy)]
struct ConicalTrim {
    origin: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
    ratio: f64,
    slope: f64,
    min_axial: f64,
    max_axial: f64,
    angular_start: f64,
    angular_span: f64,
}

impl CylindricalTrim {
    fn contains_mesh(
        self,
        mesh: &cadmpeg_ir::tessellation::Tessellation,
        inverse_body: cadmpeg_ir::transform::Transform,
        tolerance: f64,
    ) -> bool {
        mesh.vertices.iter().all(|point| {
            let point = inverse_body.apply_point(*point);
            let axial = point.vector_from(self.origin).dot(self.axis);
            let angular = cylinder_angle(point, self.origin, self.axis, self.ref_direction);
            axial.is_finite()
                && axial >= self.min_axial - tolerance
                && axial <= self.max_axial + tolerance
                && angular.is_some_and(|angular| {
                    self.angular_span >= std::f64::consts::TAU - EPS_CYLINDER_ANGLE
                        || circular_interval_contains(
                            self.angular_start,
                            self.angular_span,
                            angular,
                            tolerance / self.radius,
                        )
                })
        })
    }
}

impl ConicalTrim {
    fn contains_mesh(
        self,
        mesh: &cadmpeg_ir::tessellation::Tessellation,
        inverse_body: cadmpeg_ir::transform::Transform,
        tolerance: f64,
    ) -> bool {
        mesh.vertices.iter().all(|point| {
            let point = inverse_body.apply_point(*point);
            let axial = point.vector_from(self.origin).dot(self.axis);
            let local_radius = self.radius + axial * self.slope;
            let angular = cone_angle(
                point,
                self.origin,
                self.axis,
                self.ref_direction,
                self.ratio,
            );
            axial.is_finite()
                && local_radius.is_finite()
                && axial >= self.min_axial - tolerance
                && axial <= self.max_axial + tolerance
                && angular.is_some_and(|angular| {
                    self.angular_span >= std::f64::consts::TAU - EPS_CYLINDER_ANGLE
                        || circular_interval_contains(
                            self.angular_start,
                            self.angular_span,
                            angular,
                            tolerance
                                / (local_radius.abs() * self.ratio.min(1.0))
                                    .max(EPS_DISPLAY_QUANTIZATION),
                        )
                })
        })
    }
}

impl PlanarTrim {
    fn contains_mesh(
        &self,
        mesh: &cadmpeg_ir::tessellation::Tessellation,
        inverse_body: cadmpeg_ir::transform::Transform,
        tolerance: f64,
    ) -> bool {
        let tolerance = tolerance + self.boundary_tolerance;
        let projected = mesh
            .vertices
            .iter()
            .map(|point| self.frame.project(inverse_body.apply_point(*point)))
            .collect::<Vec<_>>();
        let holes = self
            .holes
            .iter()
            .map(|hole| match hole {
                PlanarHole::Polygon {
                    boundary,
                    triangles,
                } => Some(HoleConstraint::Polygon {
                    boundary,
                    triangles,
                }),
                PlanarHole::Circle(hole) => chordal_hole_constraint(*hole, &projected, tolerance)
                    .map(|(exclusion, boundary)| HoleConstraint::Circle {
                        exclusion,
                        boundary,
                    }),
            })
            .collect::<Option<Vec<_>>>();
        let Some(holes) = holes else {
            return false;
        };
        if projected.iter().any(|point| {
            self.outer.as_ref().is_some_and(|outer| !match outer {
                PlanarOuter::Polygon(outer) => polygon_contains(outer, *point, tolerance),
                PlanarOuter::Circle(outer) => {
                    point_distance(*point, outer.center) <= outer.radius + tolerance
                }
            }) || holes
                .iter()
                .any(|hole| hole.contains_interior(*point, tolerance))
        }) {
            return false;
        }
        mesh.triangles.iter().all(|triangle| {
            let [Some(a), Some(b), Some(c)] =
                triangle.map(|index| projected.get(index as usize).copied())
            else {
                return false;
            };
            holes
                .iter()
                .all(|hole| !hole.crosses_triangle([a, b, c], tolerance))
        })
    }
}

impl HoleConstraint<'_> {
    fn contains_interior(&self, point: Point2, tolerance: f64) -> bool {
        match self {
            Self::Polygon { boundary, .. } => polygon_strictly_contains(boundary, point, tolerance),
            Self::Circle { exclusion, .. } => {
                point_distance(point, exclusion.center) < exclusion.radius - tolerance
            }
        }
    }

    fn crosses_triangle(&self, triangle: [Point2; 3], tolerance: f64) -> bool {
        match self {
            Self::Polygon { triangles, .. } => triangles.iter().any(|hole_triangle| {
                triangles_have_positive_overlap(triangle, *hole_triangle, tolerance)
            }),
            Self::Circle {
                exclusion,
                boundary,
            } => triangle_crosses_hole(triangle, *exclusion, *boundary, tolerance),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn closed_planar_circle(
    loop_: &cadmpeg_ir::topology::Loop,
    surface: &SurfaceGeometry,
    frame: PlaneFrame,
    tolerance: f64,
    coedges: &HashMap<&cadmpeg_ir::ids::CoedgeId, &cadmpeg_ir::topology::Coedge>,
    edges: &HashMap<&cadmpeg_ir::ids::EdgeId, &cadmpeg_ir::topology::Edge>,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::topology::Vertex>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
) -> Option<CircularHole> {
    let coedge = *coedges.get(&loop_.coedges()[0])?;
    let edge = *edges.get(&coedge.edge)?;
    if coedge.owner_loop != loop_.id || coedge.next != coedge.id || coedge.previous != coedge.id {
        return None;
    }
    let CurveGeometry::Circle {
        center,
        axis,
        radius,
        ..
    } = *curves.get(edge.curve.as_ref()?)?
    else {
        return None;
    };
    let axis = axis.unit()?;
    let boundary_point = *points.get(&vertices.get(&edge.start)?.point)?;
    let end_point = *points.get(&vertices.get(&edge.end)?.point)?;
    if !radius.is_finite()
        || *radius <= tolerance
        || axis.dot(frame.normal).abs() < 1.0 - EPS_AXIS_ALIGNMENT
        || analytic_surface_residual(surface, *center)? > tolerance
        || analytic_surface_residual(surface, boundary_point)? > tolerance
        || boundary_point.distance(end_point) > tolerance
        || (boundary_point.distance(*center) - radius).abs() > tolerance
    {
        return None;
    }
    Some(CircularHole {
        center: frame.project(*center),
        radius: *radius,
    })
}

fn planar_boundary_samples(
    curve: &CurveGeometry,
    start: Point3,
    end: Point3,
    surface: &SurfaceGeometry,
    frame: PlaneFrame,
    tolerance: f64,
    sampling_tolerance: f64,
) -> Option<(Vec<Point2>, f64)> {
    match curve {
        CurveGeometry::Line { .. } => Some((vec![frame.project(start)], 0.0)),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let axis = axis.unit()?;
            let reference = (*ref_direction - axis.scale(ref_direction.dot(axis))).unit()?;
            let transverse = axis.cross(reference).unit()?;
            if axis.dot(frame.normal).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                || !radius.is_finite()
                || *radius <= tolerance
                || analytic_surface_residual(surface, *center)? > tolerance
            {
                return None;
            }
            let endpoint_tolerance = tolerance.max(sampling_tolerance);
            let start_parameter = ellipse_parameter(
                start,
                *center,
                reference,
                transverse,
                *radius,
                *radius,
                endpoint_tolerance,
            )?;
            let end_parameter = ellipse_parameter(
                end,
                *center,
                reference,
                transverse,
                *radius,
                *radius,
                endpoint_tolerance,
            )?;
            let span = shortest_arc_span(start_parameter, end_parameter)?;
            PlanarArc {
                center: *center,
                first_direction: reference,
                second_direction: transverse,
                first_radius: *radius,
                second_radius: *radius,
            }
            .samples(
                start_parameter,
                span,
                surface,
                frame,
                tolerance,
                sampling_tolerance,
            )
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let axis = axis.unit()?;
            if major_direction.dot(axis).abs() > EPS_AXIS_ALIGNMENT {
                return None;
            }
            let major_direction = major_direction.unit()?;
            let minor_direction = axis.cross(major_direction).unit()?;
            if axis.dot(frame.normal).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                || !major_radius.is_finite()
                || !minor_radius.is_finite()
                || *major_radius <= tolerance
                || *minor_radius <= tolerance
                || *major_radius < *minor_radius
                || analytic_surface_residual(surface, *center)? > tolerance
            {
                return None;
            }
            let endpoint_tolerance = tolerance.max(sampling_tolerance);
            let start_parameter = ellipse_parameter(
                start,
                *center,
                major_direction,
                minor_direction,
                *major_radius,
                *minor_radius,
                endpoint_tolerance,
            )?;
            let end_parameter = ellipse_parameter(
                end,
                *center,
                major_direction,
                minor_direction,
                *major_radius,
                *minor_radius,
                endpoint_tolerance,
            )?;
            let span = shortest_arc_span(start_parameter, end_parameter)?;
            PlanarArc {
                center: *center,
                first_direction: major_direction,
                second_direction: minor_direction,
                first_radius: *major_radius,
                second_radius: *minor_radius,
            }
            .samples(
                start_parameter,
                span,
                surface,
                frame,
                tolerance,
                sampling_tolerance,
            )
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct PlanarArc {
    center: Point3,
    first_direction: Vector3,
    second_direction: Vector3,
    first_radius: f64,
    second_radius: f64,
}

impl PlanarArc {
    fn samples(
        self,
        start_parameter: f64,
        span: f64,
        surface: &SurfaceGeometry,
        frame: PlaneFrame,
        tolerance: f64,
        sampling_tolerance: f64,
    ) -> Option<(Vec<Point2>, f64)> {
        let radius = self.first_radius.max(self.second_radius);
        let (segments, boundary_tolerance) = planar_arc_segments(span, radius, sampling_tolerance);
        let points = (0..segments)
            .map(|index| {
                let parameter =
                    start_parameter + span * f64::from(index as u32) / f64::from(segments as u32);
                let point = self
                    .center
                    .translated(self.first_direction, self.first_radius * parameter.cos())
                    .translated(self.second_direction, self.second_radius * parameter.sin());
                (point, frame.project(point))
            })
            .collect::<Vec<_>>();
        if points.iter().any(|(point, _)| {
            analytic_surface_residual(surface, *point).is_none_or(|residual| residual > tolerance)
        }) {
            return None;
        }
        Some((
            points.into_iter().map(|(_, projected)| projected).collect(),
            boundary_tolerance,
        ))
    }
}

fn ellipse_parameter(
    point: Point3,
    center: Point3,
    major_direction: Vector3,
    minor_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
    tolerance: f64,
) -> Option<f64> {
    let delta = point.vector_from(center);
    let cosine = delta.dot(major_direction) / major_radius;
    let sine = delta.dot(minor_direction) / minor_radius;
    let normalization = cosine.hypot(sine);
    (normalization.is_finite()
        && (normalization - 1.0).abs() <= tolerance / major_radius.min(minor_radius))
    .then_some(sine.atan2(cosine).rem_euclid(std::f64::consts::TAU))
}

fn shortest_arc_span(start: f64, end: f64) -> Option<f64> {
    let forward = (end - start).rem_euclid(std::f64::consts::TAU);
    let span = if forward <= std::f64::consts::PI {
        forward
    } else {
        forward - std::f64::consts::TAU
    };
    (span.abs() > EPS_CYLINDER_ANGLE && span.abs() < std::f64::consts::PI - EPS_CYLINDER_ANGLE)
        .then_some(span)
}

fn planar_arc_segments(span: f64, radius: f64, tolerance: f64) -> (usize, f64) {
    let cosine = (1.0 - tolerance / radius).clamp(-1.0, 1.0);
    let maximum_span = 2.0 * cosine.acos();
    let requested = if maximum_span.is_finite() && maximum_span > EPS_CYLINDER_ANGLE {
        (span.abs() / maximum_span).ceil() as usize
    } else {
        MAX_PLANAR_TRIM_ARC_SEGMENTS
    };
    let segments = requested.clamp(1, MAX_PLANAR_TRIM_ARC_SEGMENTS);
    let actual_span = span.abs() / f64::from(segments as u32);
    (segments, radius * (1.0 - (actual_span / 2.0).cos()))
}

#[allow(clippy::too_many_arguments)]
fn planar_trim(
    face: &cadmpeg_ir::topology::Face,
    surface: &SurfaceGeometry,
    loops: &HashMap<&cadmpeg_ir::ids::LoopId, &cadmpeg_ir::topology::Loop>,
    coedges: &HashMap<&cadmpeg_ir::ids::CoedgeId, &cadmpeg_ir::topology::Coedge>,
    edges: &HashMap<&cadmpeg_ir::ids::EdgeId, &cadmpeg_ir::topology::Edge>,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::topology::Vertex>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
) -> Option<PlanarTrim> {
    let frame = plane_frame(surface)?;
    let tolerance = face.tolerance.unwrap_or(0.0).max(EPS_DISPLAY_QUANTIZATION);
    let coordinate_scale = points
        .values()
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .fold(1.0_f64, f64::max);
    let sampling_tolerance = tolerance.max(
        coordinate_scale * f64::from(f32::EPSILON) * DISPLAY_QUANTIZATION_ULPS
            + EPS_DISPLAY_QUANTIZATION,
    );
    let mut polygons = Vec::new();
    let mut circles = Vec::new();
    let mut boundary_tolerance = 0.0_f64;
    for loop_id in &face.loops {
        let loop_ = *loops.get(loop_id)?;
        if loop_.face != face.id || loop_.coedges().is_empty() || loop_.vertices().next().is_some()
        {
            return None;
        }
        if loop_.coedges().len() == 1 {
            circles.push(closed_planar_circle(
                loop_, surface, frame, tolerance, coedges, edges, vertices, points, curves,
            )?);
            continue;
        }

        let mut polygon = Vec::with_capacity(loop_.coedges().len());
        let mut first_start = None;
        let mut previous_end = None;
        for (index, coedge_id) in loop_.coedges().iter().enumerate() {
            let coedge = *coedges.get(coedge_id)?;
            let edge = *edges.get(&coedge.edge)?;
            if coedge.owner_loop != loop_.id
                || coedge.next != loop_.coedges()[(index + 1) % loop_.coedges().len()]
                || coedge.previous
                    != loop_.coedges()[(index + loop_.coedges().len() - 1) % loop_.coedges().len()]
            {
                return None;
            }
            let (start, end) = match coedge.sense {
                Sense::Forward => (&edge.start, &edge.end),
                Sense::Reversed => (&edge.end, &edge.start),
            };
            let start = *points.get(&vertices.get(start)?.point)?;
            let end = *points.get(&vertices.get(end)?.point)?;
            if analytic_surface_residual(surface, start)? > tolerance
                || analytic_surface_residual(surface, end)? > tolerance
                || previous_end.is_some_and(|previous: Point3| previous.distance(start) > tolerance)
            {
                return None;
            }
            let (samples, sample_tolerance) = planar_boundary_samples(
                curves.get(edge.curve.as_ref()?)?,
                start,
                end,
                surface,
                frame,
                tolerance,
                sampling_tolerance,
            )?;
            polygon.extend(samples);
            boundary_tolerance = boundary_tolerance.max(sample_tolerance);
            first_start.get_or_insert(start);
            previous_end = Some(end);
        }
        if previous_end?.distance(first_start?) > tolerance {
            return None;
        }
        polygons.push(polygon);
    }
    let (outer, holes) = if polygons.is_empty() {
        if circles.is_empty() {
            return None;
        }
        let (outer, holes) = circular_outer_and_holes(&circles, tolerance)?;
        (
            PlanarOuter::Circle(outer),
            holes.into_iter().map(PlanarHole::Circle).collect(),
        )
    } else {
        let outer_candidates = polygons
            .iter()
            .enumerate()
            .filter(|(index, outer)| {
                polygons
                    .iter()
                    .enumerate()
                    .filter(|(inner_index, _)| index != inner_index)
                    .all(|(_, inner)| polygon_inside_polygon(inner, outer, sampling_tolerance))
                    && circles
                        .iter()
                        .all(|circle| circle_inside_polygon(outer, *circle, sampling_tolerance))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [outer_index] = outer_candidates.as_slice() else {
            return None;
        };
        let outer_polygon = &polygons[*outer_index];
        let polygon_holes = polygons
            .iter()
            .enumerate()
            .filter_map(|(index, polygon)| (index != *outer_index).then_some(polygon))
            .collect::<Vec<_>>();
        if polygon_holes
            .iter()
            .any(|hole| !polygon_inside_polygon(hole, outer_polygon, sampling_tolerance))
            || polygon_holes.iter().enumerate().any(|(index, left)| {
                polygon_holes[index + 1..]
                    .iter()
                    .any(|right| polygons_overlap(left, right, sampling_tolerance))
            })
            || polygon_holes.iter().any(|polygon| {
                circles
                    .iter()
                    .any(|circle| circle_overlaps_polygon(*circle, polygon, sampling_tolerance))
            })
            || circles.iter().enumerate().any(|(index, left)| {
                circles[index + 1..].iter().any(|right| {
                    point_distance(left.center, right.center)
                        < left.radius + right.radius - sampling_tolerance
                })
            })
        {
            return None;
        }
        let mut holes = circles
            .into_iter()
            .map(PlanarHole::Circle)
            .collect::<Vec<_>>();
        holes.extend(
            polygon_holes
                .into_iter()
                .map(|polygon| PlanarHole::polygon(polygon.clone(), sampling_tolerance))
                .collect::<Option<Vec<_>>>()?,
        );
        (PlanarOuter::Polygon(outer_polygon.clone()), holes)
    };
    Some(PlanarTrim {
        frame,
        outer: Some(outer),
        holes,
        boundary_tolerance,
    })
}

#[allow(clippy::too_many_arguments)]
fn planar_hole_trim(
    face: &cadmpeg_ir::topology::Face,
    surface: &SurfaceGeometry,
    loops: &HashMap<&cadmpeg_ir::ids::LoopId, &cadmpeg_ir::topology::Loop>,
    coedges: &HashMap<&cadmpeg_ir::ids::CoedgeId, &cadmpeg_ir::topology::Coedge>,
    edges: &HashMap<&cadmpeg_ir::ids::EdgeId, &cadmpeg_ir::topology::Edge>,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::topology::Vertex>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
) -> Option<PlanarTrim> {
    let frame = plane_frame(surface)?;
    let tolerance = face.tolerance.unwrap_or(0.0).max(EPS_DISPLAY_QUANTIZATION);
    let face_loops = face
        .loops
        .iter()
        .map(|loop_id| loops.get(loop_id).copied())
        .collect::<Option<Vec<_>>>()?;
    if !face_loops.iter().any(|loop_| loop_.coedges().len() > 1) {
        return None;
    }
    let holes = face_loops
        .iter()
        .filter(|loop_| {
            loop_.face == face.id && loop_.coedges().len() == 1 && loop_.vertices().next().is_none()
        })
        .filter_map(|loop_| {
            closed_planar_circle(
                loop_, surface, frame, tolerance, coedges, edges, vertices, points, curves,
            )
        })
        .collect::<Vec<_>>();
    (!holes.is_empty()).then_some(PlanarTrim {
        frame,
        outer: None,
        holes: holes.into_iter().map(PlanarHole::Circle).collect(),
        boundary_tolerance: 0.0,
    })
}

#[allow(clippy::too_many_arguments)]
fn cylindrical_trim(
    face: &cadmpeg_ir::topology::Face,
    surface: &SurfaceGeometry,
    loops: &HashMap<&cadmpeg_ir::ids::LoopId, &cadmpeg_ir::topology::Loop>,
    coedges: &HashMap<&cadmpeg_ir::ids::CoedgeId, &cadmpeg_ir::topology::Coedge>,
    edges: &HashMap<&cadmpeg_ir::ids::EdgeId, &cadmpeg_ir::topology::Edge>,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::topology::Vertex>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
) -> Option<CylindricalTrim> {
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    } = surface
    else {
        return None;
    };
    let axis = axis.unit()?;
    if !radius.is_finite() || *radius <= EPS_DISPLAY_QUANTIZATION {
        return None;
    }
    let [loop_id] = face.loops.as_slice() else {
        return None;
    };
    let loop_ = *loops.get(loop_id)?;
    if loop_.face != face.id || loop_.coedges().is_empty() || loop_.vertices().next().is_some() {
        return None;
    }
    let tolerance = face.tolerance.unwrap_or(0.0).max(EPS_DISPLAY_QUANTIZATION);
    let mut axial_bounds = None::<(f64, f64)>;
    for (index, coedge_id) in loop_.coedges().iter().enumerate() {
        let coedge = *coedges.get(coedge_id)?;
        if coedge.owner_loop != loop_.id
            || coedge.next != loop_.coedges()[(index + 1) % loop_.coedges().len()]
            || coedge.previous
                != loop_.coedges()[(index + loop_.coedges().len() - 1) % loop_.coedges().len()]
        {
            return None;
        }
        let edge = *edges.get(&coedge.edge)?;
        let curve = curves.get(edge.curve.as_ref()?)?;
        match curve {
            CurveGeometry::Line { direction, .. } => {
                if direction.unit()?.dot(axis).abs() < 1.0 - EPS_AXIS_ALIGNMENT {
                    return None;
                }
            }
            CurveGeometry::Circle {
                axis: curve_axis,
                radius: curve_radius,
                ..
            } => {
                if curve_axis.unit()?.dot(axis).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                    || !curve_radius.is_finite()
                    || (*curve_radius - *radius).abs() > tolerance
                {
                    return None;
                }
            }
            _ => return None,
        }
        for vertex_id in [edge.start.clone(), edge.end.clone()] {
            let point = *points.get(&vertices.get(&vertex_id)?.point)?;
            if analytic_surface_residual(surface, point)? > tolerance {
                return None;
            }
            let axial = point.vector_from(*origin).dot(axis);
            if !axial.is_finite() {
                return None;
            }
            axial_bounds = Some(match axial_bounds {
                Some((min_axial, max_axial)) => (min_axial.min(axial), max_axial.max(axial)),
                None => (axial, axial),
            });
        }
    }
    let (min_axial, max_axial) = axial_bounds?;
    let angles = loop_
        .coedges()
        .iter()
        .map(|coedge_id| {
            let coedge = coedges.get(coedge_id)?;
            let edge = edges.get(&coedge.edge)?;
            [edge.start.clone(), edge.end.clone()]
                .into_iter()
                .map(|vertex_id| {
                    let point = *points.get(&vertices.get(&vertex_id)?.point)?;
                    cylinder_angle(point, *origin, axis, *ref_direction)
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<Vec<_>>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let (angular_start, angular_span) = circular_interval(&angles)?;
    (max_axial - min_axial > tolerance).then_some(CylindricalTrim {
        origin: *origin,
        axis,
        ref_direction: *ref_direction,
        radius: *radius,
        min_axial,
        max_axial,
        angular_start,
        angular_span,
    })
}

#[allow(clippy::too_many_arguments)]
fn conical_trim(
    face: &cadmpeg_ir::topology::Face,
    surface: &SurfaceGeometry,
    loops: &HashMap<&cadmpeg_ir::ids::LoopId, &cadmpeg_ir::topology::Loop>,
    coedges: &HashMap<&cadmpeg_ir::ids::CoedgeId, &cadmpeg_ir::topology::Coedge>,
    edges: &HashMap<&cadmpeg_ir::ids::EdgeId, &cadmpeg_ir::topology::Edge>,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::topology::Vertex>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
) -> Option<ConicalTrim> {
    let SurfaceGeometry::Cone {
        origin,
        axis,
        ref_direction,
        radius,
        ratio,
        half_angle,
    } = surface
    else {
        return None;
    };
    let axis = axis.unit()?;
    let slope = half_angle.tan();
    if !radius.is_finite()
        || !ratio.is_finite()
        || *radius <= EPS_DISPLAY_QUANTIZATION
        || *ratio <= 0.0
        || !slope.is_finite()
    {
        return None;
    }
    let [loop_id] = face.loops.as_slice() else {
        return None;
    };
    let loop_ = *loops.get(loop_id)?;
    if loop_.face != face.id || loop_.coedges().is_empty() || loop_.vertices().next().is_some() {
        return None;
    }
    let tolerance = face.tolerance.unwrap_or(0.0).max(EPS_DISPLAY_QUANTIZATION);
    let mut axial_bounds = None::<(f64, f64)>;
    let mut angles = Vec::new();
    for (index, coedge_id) in loop_.coedges().iter().enumerate() {
        let coedge = *coedges.get(coedge_id)?;
        if coedge.owner_loop != loop_.id
            || coedge.next != loop_.coedges()[(index + 1) % loop_.coedges().len()]
            || coedge.previous
                != loop_.coedges()[(index + loop_.coedges().len() - 1) % loop_.coedges().len()]
        {
            return None;
        }
        let edge = *edges.get(&coedge.edge)?;
        let curve = curves.get(edge.curve.as_ref()?)?;
        match curve {
            CurveGeometry::Line { direction, .. } => {
                if direction.unit()?.dot(axis).abs() < 1.0 - EPS_AXIS_ALIGNMENT {
                    return None;
                }
            }
            CurveGeometry::Nurbs(nurbs) => {
                if nurbs.degree != 1
                    || nurbs.periodic
                    || nurbs.control_points.len() != 2
                    || nurbs.control_points.iter().any(|point| {
                        analytic_surface_residual(surface, *point)
                            .is_none_or(|residual| residual > tolerance)
                    })
                {
                    return None;
                }
            }
            CurveGeometry::Ellipse {
                center,
                axis: curve_axis,
                major_direction,
                major_radius,
                minor_radius,
            } => {
                let reference = (*ref_direction - axis.scale(ref_direction.dot(axis))).unit()?;
                let transverse = axis.cross(reference).unit()?;
                let major_direction = major_direction.unit()?;
                let center_delta = center.vector_from(*origin);
                let center_axial = center_delta.dot(axis);
                let center_radial = center_delta - axis.scale(center_axial);
                let expected_radius = (*radius + center_axial * slope).abs();
                let reference_aligned = major_direction.dot(reference).abs();
                let transverse_aligned = major_direction.dot(transverse).abs();
                let aligned_radii = if reference_aligned >= 1.0 - EPS_AXIS_ALIGNMENT {
                    (*major_radius - expected_radius).abs() <= tolerance
                        && (*minor_radius - expected_radius * *ratio).abs() <= tolerance
                } else if transverse_aligned >= 1.0 - EPS_AXIS_ALIGNMENT {
                    (*major_radius - expected_radius * *ratio).abs() <= tolerance
                        && (*minor_radius - expected_radius).abs() <= tolerance
                } else {
                    false
                };
                if curve_axis.unit()?.dot(axis).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                    || !major_radius.is_finite()
                    || !minor_radius.is_finite()
                    || *major_radius <= tolerance
                    || *minor_radius <= tolerance
                    || center_radial.norm() > tolerance
                    || !aligned_radii
                {
                    return None;
                }
            }
            CurveGeometry::Circle {
                center,
                axis: curve_axis,
                radius: curve_radius,
                ..
            } => {
                if (*ratio - 1.0).abs() > EPS_AXIS_ALIGNMENT
                    || curve_axis.unit()?.dot(axis).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                    || !curve_radius.is_finite()
                {
                    return None;
                }
                let center_delta = center.vector_from(*origin);
                let center_axial = center_delta.dot(axis);
                let center_radial = center_delta - axis.scale(center_axial);
                let expected_radius = (*radius + center_axial * slope).abs();
                if center_radial.norm() > tolerance
                    || (*curve_radius - expected_radius).abs() > tolerance
                {
                    return None;
                }
            }
            _ => return None,
        }
        for vertex_id in [edge.start.clone(), edge.end.clone()] {
            let point = *points.get(&vertices.get(&vertex_id)?.point)?;
            if analytic_surface_residual(surface, point)? > tolerance {
                return None;
            }
            let axial = point.vector_from(*origin).dot(axis);
            let angle = cone_angle(point, *origin, axis, *ref_direction, *ratio)?;
            if !axial.is_finite() || !angle.is_finite() {
                return None;
            }
            axial_bounds = Some(match axial_bounds {
                Some((min_axial, max_axial)) => (min_axial.min(axial), max_axial.max(axial)),
                None => (axial, axial),
            });
            angles.push(angle);
        }
    }
    let (min_axial, max_axial) = axial_bounds?;
    let (angular_start, angular_span) = circular_interval(&angles)?;
    (max_axial - min_axial > tolerance).then_some(ConicalTrim {
        origin: *origin,
        axis,
        ref_direction: *ref_direction,
        radius: *radius,
        ratio: *ratio,
        slope,
        min_axial,
        max_axial,
        angular_start,
        angular_span,
    })
}

fn cylinder_angle(
    point: Point3,
    origin: Point3,
    axis: Vector3,
    ref_direction: Vector3,
) -> Option<f64> {
    let axis = axis.unit()?;
    let reference = (ref_direction - axis.scale(ref_direction.dot(axis))).unit()?;
    let transverse = axis.cross(reference).unit()?;
    let delta = point.vector_from(origin);
    let angle = delta.dot(transverse).atan2(delta.dot(reference));
    angle
        .is_finite()
        .then_some(angle.rem_euclid(std::f64::consts::TAU))
}

fn cone_angle(
    point: Point3,
    origin: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    ratio: f64,
) -> Option<f64> {
    if !ratio.is_finite() || ratio <= 0.0 {
        return None;
    }
    let axis = axis.unit()?;
    let reference = (ref_direction - axis.scale(ref_direction.dot(axis))).unit()?;
    let transverse = axis.cross(reference).unit()?;
    let delta = point.vector_from(origin);
    let major = delta.dot(reference);
    let minor = delta.dot(transverse);
    let angle = (minor / ratio).atan2(major);
    angle
        .is_finite()
        .then_some(angle.rem_euclid(std::f64::consts::TAU))
}

fn circular_interval(angles: &[f64]) -> Option<(f64, f64)> {
    let mut angles = angles
        .iter()
        .copied()
        .filter(|angle| angle.is_finite())
        .collect::<Vec<_>>();
    if angles.is_empty() {
        return None;
    }
    angles.sort_by(f64::total_cmp);
    angles.dedup_by(|left, right| (*left - *right).abs() <= EPS_CYLINDER_ANGLE);
    if angles.len() == 1 {
        return Some((angles[0], std::f64::consts::TAU));
    }
    let (largest_gap_index, largest_gap) = (0..angles.len())
        .map(|index| {
            let next = angles[(index + 1) % angles.len()];
            let gap = if index + 1 == angles.len() {
                next + std::f64::consts::TAU - angles[index]
            } else {
                next - angles[index]
            };
            (index, gap)
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))?;
    let start = angles[(largest_gap_index + 1) % angles.len()];
    Some((start, std::f64::consts::TAU - largest_gap))
}

fn circular_interval_contains(start: f64, span: f64, angle: f64, tolerance: f64) -> bool {
    let distance = (angle - start).rem_euclid(std::f64::consts::TAU);
    // Quantized display-list angles can fall just before the start boundary.
    // The wrapped distance is then near TAU, rather than near zero.
    distance <= span + tolerance || distance + tolerance >= std::f64::consts::TAU
}

// The trim grammars share the same indexed topology maps.
#[allow(clippy::too_many_arguments)]
fn analytic_trim(
    face: &cadmpeg_ir::topology::Face,
    surface: &SurfaceGeometry,
    loops: &HashMap<&cadmpeg_ir::ids::LoopId, &cadmpeg_ir::topology::Loop>,
    coedges: &HashMap<&cadmpeg_ir::ids::CoedgeId, &cadmpeg_ir::topology::Coedge>,
    edges: &HashMap<&cadmpeg_ir::ids::EdgeId, &cadmpeg_ir::topology::Edge>,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::topology::Vertex>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
) -> Option<AnalyticTrim> {
    match surface {
        SurfaceGeometry::Plane { .. } => planar_trim(
            face, surface, loops, coedges, edges, vertices, points, curves,
        )
        .or_else(|| {
            planar_hole_trim(
                face, surface, loops, coedges, edges, vertices, points, curves,
            )
        })
        .map(AnalyticTrim::Planar),
        SurfaceGeometry::Cylinder { .. } => cylindrical_trim(
            face, surface, loops, coedges, edges, vertices, points, curves,
        )
        .map(AnalyticTrim::Cylindrical),
        SurfaceGeometry::Cone { .. } => conical_trim(
            face, surface, loops, coedges, edges, vertices, points, curves,
        )
        .map(AnalyticTrim::Conical),
        _ => None,
    }
}

fn plane_frame(surface: &SurfaceGeometry) -> Option<PlaneFrame> {
    let (origin, normal, u_axis) = match surface {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => (*origin, *normal, *u_axis),
        SurfaceGeometry::Transformed { basis, transform } if transform.is_proper_rigid() => {
            let basis = plane_frame(basis)?;
            (
                transform.apply_point(basis.origin),
                transform.apply_vector(basis.normal),
                transform.apply_vector(basis.u_axis),
            )
        }
        _ => return None,
    };
    let normal = normal.unit()?;
    let u_axis = (u_axis - normal.scale(u_axis.dot(normal))).unit()?;
    let v_axis = normal.cross(u_axis).unit()?;
    [origin.x, origin.y, origin.z, normal.x, normal.y, normal.z]
        .into_iter()
        .all(f64::is_finite)
        .then_some(PlaneFrame {
            origin,
            normal,
            u_axis,
            v_axis,
        })
}

fn point_distance(left: Point2, right: Point2) -> f64 {
    ((left.u - right.u).powi(2) + (left.v - right.v).powi(2)).sqrt()
}

fn point_segment_distance(point: Point2, start: Point2, end: Point2) -> f64 {
    let du = end.u - start.u;
    let dv = end.v - start.v;
    let length_squared = du * du + dv * dv;
    if length_squared <= f64::EPSILON {
        return point_distance(point, start);
    }
    let t =
        (((point.u - start.u) * du + (point.v - start.v) * dv) / length_squared).clamp(0.0, 1.0);
    point_distance(point, Point2::new(start.u + t * du, start.v + t * dv))
}

fn signed_area_twice(left: Point2, middle: Point2, right: Point2) -> f64 {
    (middle.u - left.u) * (right.v - middle.v) - (middle.v - left.v) * (right.u - middle.u)
}

fn polygon_area_twice(polygon: &[Point2]) -> f64 {
    (0..polygon.len())
        .map(|index| {
            let left = polygon[index];
            let right = polygon[(index + 1) % polygon.len()];
            left.u * right.v - left.v * right.u
        })
        .sum()
}

fn is_simple_polygon(polygon: &[Point2], tolerance: f64) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let area = polygon_area_twice(polygon);
    if !area.is_finite() || area.abs() <= tolerance * tolerance {
        return false;
    }
    for left in 0..polygon.len() {
        for right in left + 1..polygon.len() {
            if right == left + 1 || (left == 0 && right + 1 == polygon.len()) {
                continue;
            }
            if segments_intersect(
                polygon[left],
                polygon[(left + 1) % polygon.len()],
                polygon[right],
                polygon[(right + 1) % polygon.len()],
                tolerance,
            ) {
                return false;
            }
        }
    }
    true
}

fn triangulate_polygon(polygon: &[Point2], tolerance: f64) -> Option<Vec<[Point2; 3]>> {
    if !is_simple_polygon(polygon, tolerance)
        || !polygon
            .iter()
            .all(|point| point.u.is_finite() && point.v.is_finite())
    {
        return None;
    }
    let orientation = polygon_area_twice(polygon).signum();
    let mut remaining = (0..polygon.len()).collect::<Vec<_>>();
    let mut triangles = Vec::with_capacity(polygon.len() - 2);
    while remaining.len() > 3 {
        let ear_position = (0..remaining.len()).find(|position| {
            let previous_position = (position + remaining.len() - 1) % remaining.len();
            let next_position = (position + 1) % remaining.len();
            let previous = polygon[remaining[previous_position]];
            let current = polygon[remaining[*position]];
            let next = polygon[remaining[next_position]];
            let scale = point_distance(previous, current)
                .max(point_distance(current, next))
                .max(1.0);
            let cross = signed_area_twice(previous, current, next);
            if !cross.is_finite() || orientation * cross <= tolerance * scale {
                return false;
            }
            if remaining.iter().enumerate().any(|(edge_position, _)| {
                let edge_next_position = (edge_position + 1) % remaining.len();
                if edge_position == previous_position
                    || edge_position == *position
                    || edge_position == next_position
                    || edge_next_position == previous_position
                    || edge_next_position == *position
                    || edge_next_position == next_position
                {
                    return false;
                }
                segments_intersect(
                    previous,
                    next,
                    polygon[remaining[edge_position]],
                    polygon[remaining[edge_next_position]],
                    tolerance,
                )
            }) {
                return false;
            }
            let triangle = [previous, current, next];
            !remaining.iter().enumerate().any(|(other_position, index)| {
                other_position != previous_position
                    && other_position != *position
                    && other_position != next_position
                    && polygon_strictly_contains(&triangle, polygon[*index], tolerance)
            })
        });
        let position = ear_position?;
        let previous_position = (position + remaining.len() - 1) % remaining.len();
        let next_position = (position + 1) % remaining.len();
        triangles.push([
            polygon[remaining[previous_position]],
            polygon[remaining[position]],
            polygon[remaining[next_position]],
        ]);
        remaining.remove(position);
    }
    let final_triangle = remaining
        .into_iter()
        .map(|index| polygon[index])
        .collect::<Vec<_>>();
    let [first, second, third] = final_triangle.as_slice() else {
        return None;
    };
    let scale = point_distance(*first, *second)
        .max(point_distance(*second, *third))
        .max(point_distance(*third, *first))
        .max(1.0);
    (signed_area_twice(*first, *second, *third).abs() > tolerance * scale).then(|| {
        triangles.push([*first, *second, *third]);
        triangles
    })
}

fn segments_intersect(
    first_start: Point2,
    first_end: Point2,
    second_start: Point2,
    second_end: Point2,
    tolerance: f64,
) -> bool {
    let bounds_overlap = |left: f64, right: f64, other_left: f64, other_right: f64| {
        left.min(right) <= other_left.max(other_right) + tolerance
            && other_left.min(other_right) <= left.max(right) + tolerance
    };
    if !bounds_overlap(first_start.u, first_end.u, second_start.u, second_end.u)
        || !bounds_overlap(first_start.v, first_end.v, second_start.v, second_end.v)
    {
        return false;
    }
    let scale = point_distance(first_start, first_end)
        .max(point_distance(second_start, second_end))
        .max(1.0);
    let orientation_tolerance = tolerance * scale;
    let first_left = signed_area_twice(first_start, first_end, second_start);
    let first_right = signed_area_twice(first_start, first_end, second_end);
    let second_left = signed_area_twice(second_start, second_end, first_start);
    let second_right = signed_area_twice(second_start, second_end, first_end);
    if first_left.abs() <= orientation_tolerance
        && point_segment_distance(second_start, first_start, first_end) <= tolerance
    {
        return true;
    }
    if first_right.abs() <= orientation_tolerance
        && point_segment_distance(second_end, first_start, first_end) <= tolerance
    {
        return true;
    }
    if second_left.abs() <= orientation_tolerance
        && point_segment_distance(first_start, second_start, second_end) <= tolerance
    {
        return true;
    }
    if second_right.abs() <= orientation_tolerance
        && point_segment_distance(first_end, second_start, second_end) <= tolerance
    {
        return true;
    }
    (first_left > orientation_tolerance && first_right < -orientation_tolerance
        || first_left < -orientation_tolerance && first_right > orientation_tolerance)
        && (second_left > orientation_tolerance && second_right < -orientation_tolerance
            || second_left < -orientation_tolerance && second_right > orientation_tolerance)
}

fn polygon_contains(polygon: &[Point2], point: Point2, tolerance: f64) -> bool {
    if polygon.iter().enumerate().any(|(index, start)| {
        point_segment_distance(point, *start, polygon[(index + 1) % polygon.len()]) <= tolerance
    }) {
        return true;
    }
    let mut inside = false;
    for (index, start) in polygon.iter().enumerate() {
        let end = polygon[(index + 1) % polygon.len()];
        if (start.v > point.v) == (end.v > point.v) {
            continue;
        }
        let crossing = start.u + (point.v - start.v) * (end.u - start.u) / (end.v - start.v);
        if crossing > point.u {
            inside = !inside;
        }
    }
    inside
}

fn polygon_strictly_contains(polygon: &[Point2], point: Point2, tolerance: f64) -> bool {
    polygon_contains(polygon, point, tolerance)
        && !polygon.iter().enumerate().any(|(index, start)| {
            point_segment_distance(point, *start, polygon[(index + 1) % polygon.len()]) <= tolerance
        })
}

fn polygon_inside_polygon(inner: &[Point2], outer: &[Point2], tolerance: f64) -> bool {
    is_simple_polygon(inner, tolerance)
        && is_simple_polygon(outer, tolerance)
        && inner
            .iter()
            .all(|point| polygon_contains(outer, *point, tolerance))
        && !inner.iter().enumerate().any(|(left_index, left)| {
            outer.iter().enumerate().any(|(right_index, right)| {
                segments_intersect(
                    *left,
                    inner[(left_index + 1) % inner.len()],
                    *right,
                    outer[(right_index + 1) % outer.len()],
                    tolerance,
                )
            })
        })
}

fn polygons_overlap(first: &[Point2], second: &[Point2], tolerance: f64) -> bool {
    first.iter().enumerate().any(|(first_index, first_start)| {
        second
            .iter()
            .enumerate()
            .any(|(second_index, second_start)| {
                segments_intersect(
                    *first_start,
                    first[(first_index + 1) % first.len()],
                    *second_start,
                    second[(second_index + 1) % second.len()],
                    tolerance,
                )
            })
    }) || first
        .iter()
        .any(|point| polygon_strictly_contains(second, *point, tolerance))
        || second
            .iter()
            .any(|point| polygon_strictly_contains(first, *point, tolerance))
}

fn triangles_have_positive_overlap(
    first: [Point2; 3],
    second: [Point2; 3],
    tolerance: f64,
) -> bool {
    for triangle in [first, second] {
        for index in 0..triangle.len() {
            let start = triangle[index];
            let end = triangle[(index + 1) % triangle.len()];
            let length = point_distance(start, end);
            if !length.is_finite() || length <= f64::EPSILON {
                return false;
            }
            let axis = Point2::new((start.v - end.v) / length, (end.u - start.u) / length);
            let first_projection = triangle_projection(first, axis);
            let second_projection = triangle_projection(second, axis);
            let overlap = first_projection.1.min(second_projection.1)
                - first_projection.0.max(second_projection.0);
            if !overlap.is_finite() || overlap <= tolerance {
                return false;
            }
        }
    }
    true
}

fn triangle_projection(triangle: [Point2; 3], axis: Point2) -> (f64, f64) {
    triangle
        .into_iter()
        .map(|point| point.u * axis.u + point.v * axis.v)
        .fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
        )
}

fn convex_polygon_contains(polygon: &[Point2], point: Point2, tolerance: f64) -> bool {
    let mut sign = 0.0_f64;
    for index in 0..polygon.len() {
        let start = polygon[index];
        let end = polygon[(index + 1) % polygon.len()];
        let cross = signed_area_twice(start, end, point);
        if point_segment_distance(point, start, end) <= tolerance {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn circle_inside_polygon(polygon: &[Point2], hole: CircularHole, tolerance: f64) -> bool {
    polygon_contains(polygon, hole.center, tolerance)
        && (0..polygon.len()).all(|index| {
            point_segment_distance(
                hole.center,
                polygon[index],
                polygon[(index + 1) % polygon.len()],
            ) >= hole.radius - tolerance
        })
}

fn circle_inside_circle(outer: CircularHole, inner: CircularHole, tolerance: f64) -> bool {
    point_distance(outer.center, inner.center) + inner.radius <= outer.radius + tolerance
}

fn circular_outer_and_holes(
    circles: &[CircularHole],
    tolerance: f64,
) -> Option<(CircularHole, Vec<CircularHole>)> {
    let enclosing = circles
        .iter()
        .enumerate()
        .filter(|(index, outer)| {
            circles.iter().enumerate().all(|(inner_index, inner)| {
                index == &inner_index || circle_inside_circle(**outer, *inner, tolerance)
            })
        })
        .collect::<Vec<_>>();
    let [(outer_index, outer)] = enclosing.as_slice() else {
        return None;
    };
    let holes = circles
        .iter()
        .enumerate()
        .filter_map(|(index, hole)| (index != *outer_index).then_some(*hole))
        .collect::<Vec<_>>();
    if holes.iter().enumerate().any(|(index, left)| {
        holes[index + 1..].iter().any(|right| {
            point_distance(left.center, right.center) < left.radius + right.radius - tolerance
        })
    }) {
        return None;
    }
    Some((**outer, holes))
}

fn chordal_hole_constraint(
    hole: CircularHole,
    points: &[Point2],
    tolerance: f64,
) -> Option<(CircularHole, CircularHole)> {
    let distances = points
        .iter()
        .map(|point| point_distance(*point, hole.center))
        .collect::<Vec<_>>();
    let minimum = distances.iter().copied().reduce(f64::min)?;
    if minimum >= hole.radius - tolerance {
        return Some((hole, hole));
    }

    let mut boundary_angles = points
        .iter()
        .zip(&distances)
        .filter_map(|(point, distance)| {
            ((*distance - hole.radius).abs() <= tolerance)
                .then_some((point.v - hole.center.v).atan2(point.u - hole.center.u))
        })
        .collect::<Vec<_>>();
    boundary_angles.sort_by(f64::total_cmp);
    boundary_angles.dedup_by(|left, right| (*left - *right).abs() <= tolerance / hole.radius);
    if boundary_angles.len() < 3 {
        return None;
    }
    let wrap_gap = boundary_angles[0] + std::f64::consts::TAU - *boundary_angles.last()?;
    let maximum_gap = boundary_angles
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .chain(std::iter::once(wrap_gap))
        .reduce(f64::max)?;
    if maximum_gap > std::f64::consts::PI {
        return None;
    }
    let maximum_sagitta = hole.radius * (1.0 - (maximum_gap / 2.0).cos());
    let inward_deflection = hole.radius - minimum;
    if inward_deflection > maximum_sagitta + tolerance {
        return None;
    }
    Some((
        CircularHole {
            radius: hole.radius - maximum_sagitta,
            ..hole
        },
        hole,
    ))
}

fn triangle_crosses_hole(
    triangle: [Point2; 3],
    exclusion: CircularHole,
    boundary: CircularHole,
    tolerance: f64,
) -> bool {
    if convex_polygon_contains(&triangle, exclusion.center, tolerance) {
        return true;
    }
    (0..3).any(|index| {
        let start = triangle[index];
        let end = triangle[(index + 1) % 3];
        point_segment_distance(exclusion.center, start, end) < exclusion.radius - tolerance
            && ![start, end].iter().all(|point| {
                (point_distance(*point, boundary.center) - boundary.radius).abs() <= tolerance
            })
    })
}

fn circle_overlaps_polygon(circle: CircularHole, polygon: &[Point2], tolerance: f64) -> bool {
    polygon_strictly_contains(polygon, circle.center, tolerance)
        || polygon
            .iter()
            .any(|point| point_distance(*point, circle.center) < circle.radius + tolerance)
        || (0..polygon.len()).any(|index| {
            point_segment_distance(
                circle.center,
                polygon[index],
                polygon[(index + 1) % polygon.len()],
            ) < circle.radius + tolerance
        })
}

fn analytic_surface_normal(surface: &SurfaceGeometry, point: Point3) -> Option<Vector3> {
    let subtract = |left: Point3, right: Point3| {
        Vector3::new(left.x - right.x, left.y - right.y, left.z - right.z)
    };
    match surface {
        SurfaceGeometry::Plane { normal, .. } => normal.unit(),
        SurfaceGeometry::Cylinder { origin, axis, .. } => {
            let axis = axis.unit()?;
            let delta = subtract(point, *origin);
            let radial = delta - axis.scale(delta.dot(axis));
            radial.unit()
        }
        SurfaceGeometry::Sphere { center, .. } => subtract(point, *center).unit(),
        SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            ..
        } => {
            let axis = axis.unit()?;
            let delta = subtract(point, *center);
            let axial = delta.dot(axis);
            let radial = delta - axis.scale(axial);
            let radial_unit = radial.unit()?;
            (radial_unit.scale(radial.norm() - major_radius) + axis.scale(axial)).unit()
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            let axis = axis.unit()?;
            let reference = (*ref_direction - axis.scale(ref_direction.dot(axis))).unit()?;
            let transverse = axis.cross(reference).unit()?;
            let slope = half_angle.tan();
            if !radius.is_finite() || !ratio.is_finite() || *ratio <= 0.0 || !slope.is_finite() {
                return None;
            }
            let delta = point.vector_from(*origin);
            let axial = delta.dot(axis);
            let major = delta.dot(reference);
            let minor = delta.dot(transverse);
            let elliptical_radius = major.hypot(minor / *ratio);
            if elliptical_radius <= f64::EPSILON {
                return None;
            }
            let local_radius = *radius + axial * slope;
            (reference.scale(major / elliptical_radius)
                + transverse.scale(minor / (*ratio * *ratio * elliptical_radius))
                - axis.scale(slope * local_radius.signum()))
            .unit()
        }
        SurfaceGeometry::Transformed { basis, transform } if transform.is_proper_rigid() => {
            transform
                .apply_vector(analytic_surface_normal(
                    basis,
                    transform.try_inverse_affine()?.apply_point(point),
                )?)
                .unit()
        }
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

fn analytic_surface_residual(surface: &SurfaceGeometry, point: Point3) -> Option<f64> {
    let subtract = |left: Point3, right: Point3| {
        Vector3::new(left.x - right.x, left.y - right.y, left.z - right.z)
    };
    match surface {
        SurfaceGeometry::Plane { origin, normal, .. } => {
            Some(subtract(point, *origin).dot(*normal).abs() / normal.norm())
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => {
            let delta = subtract(point, *origin);
            let axis_length = axis.norm();
            let axial = delta.dot(*axis) / axis_length;
            let radial = Vector3::new(
                delta.x - axis.x * axial / axis_length,
                delta.y - axis.y * axial / axis_length,
                delta.z - axis.z * axial / axis_length,
            );
            Some((radial.norm() - radius).abs())
        }
        SurfaceGeometry::Sphere { center, radius, .. } => {
            Some((subtract(point, *center).norm() - radius).abs())
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let delta = subtract(point, *center);
            let axis_length = axis.norm();
            let axial = delta.dot(*axis) / axis_length;
            let radial = Vector3::new(
                delta.x - axis.x * axial / axis_length,
                delta.y - axis.y * axial / axis_length,
                delta.z - axis.z * axial / axis_length,
            );
            Some(
                (((radial.norm() - major_radius).powi(2) + axial.powi(2)).sqrt() - minor_radius)
                    .abs(),
            )
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            let unit = |vector: Vector3| {
                let length = vector.norm();
                (length.is_finite() && length > f64::EPSILON).then(|| vector.scale(1.0 / length))
            };
            let axis = unit(*axis)?;
            let reference = unit(*ref_direction - axis.scale((*ref_direction).dot(axis)))?;
            let transverse = unit(axis.cross(reference))?;
            let slope = half_angle.tan();
            if !radius.is_finite() || !ratio.is_finite() || *ratio <= 0.0 || !slope.is_finite() {
                return None;
            }
            let delta = subtract(point, *origin);
            let axial = delta.dot(axis);
            let major = delta.dot(reference);
            let minor = delta.dot(transverse);
            let local_radius = radius + axial * slope;
            let elliptical_radius = major.hypot(minor / ratio);
            Some((elliptical_radius - local_radius.abs()).abs())
        }
        SurfaceGeometry::Transformed { basis, transform } if transform.is_proper_rigid() => {
            analytic_surface_residual(basis, transform.try_inverse_affine()?.apply_point(point))
        }
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
    .filter(|residual| residual.is_finite())
}

#[derive(Debug, Clone, Copy)]
struct SurfaceMeasure {
    residual: f64,
    normal: Option<Vector3>,
}

fn surface_measure(
    surface: &SurfaceGeometry,
    point: Point3,
    fit_tolerance: Option<f64>,
) -> Option<SurfaceMeasure> {
    if let SurfaceGeometry::Nurbs(nurbs) = surface {
        let tolerance = fit_tolerance?;
        let parameters = cadmpeg_ir::eval::nurbs_surface_parameter_near_point(nurbs, point, None)?;
        let partials = cadmpeg_ir::eval::nurbs_surface_partials(nurbs, parameters.u, parameters.v)?;
        let residual = point.distance(partials.point);
        if residual > tolerance {
            return None;
        }
        return Some(SurfaceMeasure {
            residual,
            normal: partials.du.cross(partials.dv).unit(),
        })
        .filter(|measure| measure.residual.is_finite());
    }
    if let SurfaceGeometry::Transformed { basis, transform } = surface {
        if transform.is_proper_rigid() {
            let mut measure = surface_measure(
                basis,
                transform.try_inverse_affine()?.apply_point(point),
                fit_tolerance,
            )?;
            measure.normal = measure
                .normal
                .and_then(|normal| transform.apply_vector(normal).unit());
            return Some(measure);
        }
    }
    Some(SurfaceMeasure {
        residual: analytic_surface_residual(surface, point)?,
        normal: analytic_surface_normal(surface, point),
    })
}

#[cfg(test)]
mod tests;
