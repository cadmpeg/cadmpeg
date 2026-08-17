// SPDX-License-Identifier: Apache-2.0
//! `DisplayLists` descriptor tables.

use crate::container::{ContainerScan, Section};
use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
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
pub(crate) struct PersistentSurfaceReference {
    pub(crate) feature_source_id: u32,
    pub(crate) local_surface_id: u32,
}

impl DisplayFace {
    /// Return the source ID only when all duplicated references agree.
    pub(crate) fn feature_source_id(&self) -> Option<u32> {
        let mut sources = self
            .surface_references
            .iter()
            .map(|reference| reference.feature_source_id);
        let source = sources.next()?;
        sources
            .all(|candidate| candidate == source)
            .then_some(source)
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
    let classes = class_intervals(payload);
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
        let limit = classes
            .iter()
            .find(|class| class.name == "uoTempFaceTessData_c" && class.class_offset + 6 == marker)
            .map_or_else(
                || {
                    markers
                        .get(marker_index + 1)
                        .copied()
                        .unwrap_or(payload.len())
                },
                |class| class.content.end,
            );
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
        let units = raw
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();
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
        references.push(PersistentSurfaceReference {
            feature_source_id,
            local_surface_id,
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

/// Bind a face-tessellation table when its vertices select one analytic face.
///
/// Display coordinates are stored as f32, while the B-rep carriers are f64.
/// The relative tolerance below covers that quantization. Complete analytic
/// trims can distinguish faces on a shared analytic carrier.
pub(crate) fn assign_unique_analytic_owners(
    model: &mut cadmpeg_ir::document::Model,
) -> Vec<String> {
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
            Some((
                &face.id,
                body,
                *surfaces.get(&face.surface)?,
                face.tolerance.unwrap_or(0.0),
                inverse,
                analytic_trim(
                    face,
                    *surfaces.get(&face.surface)?,
                    &loops,
                    &coedges,
                    &edges,
                    &vertices,
                    &points,
                    &curves,
                ),
            ))
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
            coordinate_scale * f64::from(f32::EPSILON) * 8.0 + EPS_DISPLAY_QUANTIZATION;
        let mut owners = candidates
            .iter()
            .filter(|(_, _, surface, tolerance, inverse, _)| {
                let tolerance = tolerance.max(quantization_tolerance);
                mesh.vertices.iter().all(|point| {
                    analytic_surface_residual(surface, inverse.apply_point(*point))
                        .is_some_and(|residual| residual <= tolerance)
                })
            })
            .collect::<Vec<_>>();
        if owners.len() > 1 {
            owners.retain(|(_, _, _, tolerance, inverse, trim)| {
                trim.as_ref().is_none_or(|trim| {
                    trim.contains_mesh(mesh, *inverse, tolerance.max(quantization_tolerance))
                })
            });
        }
        let [owner] = owners.as_slice() else {
            continue;
        };
        let (face, body, ..) = owner;
        mesh.faces.push((*face).clone());
        mesh.body = Some((*body).clone());
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
        }
    }
}

#[derive(Debug, Clone)]
struct PlanarTrim {
    frame: PlaneFrame,
    outer: Vec<Point2>,
    holes: Vec<CircularHole>,
}

#[derive(Debug, Clone, Copy)]
struct CylindricalTrim {
    origin: Point3,
    axis: Vector3,
    min_axial: f64,
    max_axial: f64,
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
            axial.is_finite()
                && axial >= self.min_axial - tolerance
                && axial <= self.max_axial + tolerance
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
        let projected = mesh
            .vertices
            .iter()
            .map(|point| self.frame.project(inverse_body.apply_point(*point)))
            .collect::<Vec<_>>();
        if projected.iter().any(|point| {
            !convex_polygon_contains(&self.outer, *point, tolerance)
                || self
                    .holes
                    .iter()
                    .any(|hole| point_distance(*point, hole.center) < hole.radius - tolerance)
        }) {
            return false;
        }
        mesh.triangles.iter().all(|triangle| {
            let [Some(a), Some(b), Some(c)] =
                triangle.map(|index| projected.get(index as usize).copied())
            else {
                return false;
            };
            self.holes
                .iter()
                .all(|hole| !triangle_crosses_hole([a, b, c], *hole, tolerance))
        })
    }
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
    let mut polygons = Vec::new();
    let mut holes = Vec::new();
    for loop_id in &face.loops {
        let loop_ = *loops.get(loop_id)?;
        if loop_.face != face.id || loop_.coedges.is_empty() || !loop_.vertex_uses.is_empty() {
            return None;
        }
        if loop_.coedges.len() == 1 {
            let coedge = *coedges.get(&loop_.coedges[0])?;
            let edge = *edges.get(&coedge.edge)?;
            if coedge.owner_loop != loop_.id
                || coedge.next != coedge.id
                || coedge.previous != coedge.id
                || edge.start != edge.end
            {
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
            if !radius.is_finite()
                || *radius <= tolerance
                || axis.dot(frame.normal).abs() < 1.0 - EPS_AXIS_ALIGNMENT
                || analytic_surface_residual(surface, *center)? > tolerance
                || analytic_surface_residual(surface, boundary_point)? > tolerance
                || (boundary_point.distance(*center) - radius).abs() > tolerance
            {
                return None;
            }
            holes.push(CircularHole {
                center: frame.project(*center),
                radius: *radius,
            });
            continue;
        }

        let mut polygon = Vec::with_capacity(loop_.coedges.len());
        let mut first_start = None;
        let mut previous_end = None;
        for (index, coedge_id) in loop_.coedges.iter().enumerate() {
            let coedge = *coedges.get(coedge_id)?;
            let edge = *edges.get(&coedge.edge)?;
            if coedge.owner_loop != loop_.id
                || coedge.next != loop_.coedges[(index + 1) % loop_.coedges.len()]
                || coedge.previous
                    != loop_.coedges[(index + loop_.coedges.len() - 1) % loop_.coedges.len()]
                || !matches!(
                    curves.get(edge.curve.as_ref()?)?,
                    CurveGeometry::Line { .. }
                )
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
            polygon.push(frame.project(start));
            first_start.get_or_insert(start);
            previous_end = Some(end);
        }
        if previous_end?.distance(first_start?) > tolerance {
            return None;
        }
        polygons.push(polygon);
    }
    let [outer] = polygons.as_slice() else {
        return None;
    };
    if !is_strictly_convex(outer, tolerance)
        || holes
            .iter()
            .any(|hole| !circle_inside_polygon(outer, *hole, tolerance))
        || holes.iter().enumerate().any(|(index, left)| {
            holes[index + 1..].iter().any(|right| {
                point_distance(left.center, right.center) < left.radius + right.radius - tolerance
            })
        })
    {
        return None;
    }
    Some(PlanarTrim {
        frame,
        outer: outer.clone(),
        holes,
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
        radius,
        ..
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
    if loop_.face != face.id || loop_.coedges.is_empty() || !loop_.vertex_uses.is_empty() {
        return None;
    }
    let tolerance = face.tolerance.unwrap_or(0.0).max(EPS_DISPLAY_QUANTIZATION);
    let mut axial_bounds = None::<(f64, f64)>;
    for (index, coedge_id) in loop_.coedges.iter().enumerate() {
        let coedge = *coedges.get(coedge_id)?;
        if coedge.owner_loop != loop_.id
            || coedge.next != loop_.coedges[(index + 1) % loop_.coedges.len()]
            || coedge.previous
                != loop_.coedges[(index + loop_.coedges.len() - 1) % loop_.coedges.len()]
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
    (max_axial - min_axial > tolerance).then_some(CylindricalTrim {
        origin: *origin,
        axis,
        min_axial,
        max_axial,
    })
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
        .map(AnalyticTrim::Planar),
        SurfaceGeometry::Cylinder { .. } => cylindrical_trim(
            face, surface, loops, coedges, edges, vertices, points, curves,
        )
        .map(AnalyticTrim::Cylindrical),
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

fn is_strictly_convex(polygon: &[Point2], tolerance: f64) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut sign = 0.0_f64;
    for index in 0..polygon.len() {
        let cross = signed_area_twice(
            polygon[index],
            polygon[(index + 1) % polygon.len()],
            polygon[(index + 2) % polygon.len()],
        );
        if cross.abs() <= tolerance * tolerance {
            return false;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
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
    convex_polygon_contains(polygon, hole.center, tolerance)
        && (0..polygon.len()).all(|index| {
            point_segment_distance(
                hole.center,
                polygon[index],
                polygon[(index + 1) % polygon.len()],
            ) >= hole.radius - tolerance
        })
}

fn triangle_crosses_hole(triangle: [Point2; 3], hole: CircularHole, tolerance: f64) -> bool {
    if convex_polygon_contains(&triangle, hole.center, tolerance) {
        return true;
    }
    (0..3).any(|index| {
        let start = triangle[index];
        let end = triangle[(index + 1) % 3];
        point_segment_distance(hole.center, start, end) < hole.radius - tolerance
            && ![start, end]
                .iter()
                .all(|point| (point_distance(*point, hole.center) - hole.radius).abs() <= tolerance)
    })
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

#[cfg(test)]
mod tests;
