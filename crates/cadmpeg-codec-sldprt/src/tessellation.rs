// SPDX-License-Identifier: Apache-2.0
//! `DisplayLists` descriptor tables.

use crate::container::{ContainerScan, Section};
use cadmpeg_core::le::u32_at as u32_le;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::tessellation::TessellationChannel;
use cadmpeg_ir::topology::Sense;
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
        .map(|bytes| usize::try_from(u32::from_le_bytes(bytes.try_into().ok()?)).ok())
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
            .all(|bytes| f32::from_le_bytes(bytes.try_into().expect("four-byte chunk")).is_finite())
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

pub fn section_meshes(section: Section<'_>) -> Vec<Mesh> {
    const MARKER: &[u8] = b"uoTempFaceTessData_c";
    let payload = section.payload();
    let Some(marker) = payload.windows(MARKER.len()).position(|w| w == MARKER) else {
        return Vec::new();
    };
    let end = marker + MARKER.len();
    let (Some(triangle_count), Some(strip_count)) =
        (u32_le(payload, end), u32_le(payload, end + 4))
    else {
        return Vec::new();
    };
    let meshes = parse_table_sequence(payload, end + descriptor_table_offset(payload, end));
    meshes
        .filter(|meshes| {
            meshes.first().is_some_and(|mesh| {
                usize::try_from(triangle_count).ok() == Some(mesh.triangles.len())
                    && usize::try_from(strip_count).ok() == Some(mesh.strip_lengths.len())
            })
        })
        .unwrap_or_default()
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

/// Bind a face-tessellation table when its vertices select one analytic face.
///
/// Display coordinates are stored as f32, while the B-rep carriers are f64.
/// The relative tolerance below covers that quantization. Complete planar
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
                planar_trim(
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
        let quantization_tolerance = coordinate_scale * f64::from(f32::EPSILON) * 8.0 + 1.0e-9;
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
struct PlanarTrim {
    frame: PlaneFrame,
    outer: Vec<Point2>,
    holes: Vec<CircularHole>,
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
    let tolerance = face.tolerance.unwrap_or(0.0).max(1.0e-9);
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
                || axis.dot(frame.normal).abs() < 1.0 - 1.0e-9
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
    let dot =
        |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
    let norm = |value: Vector3| dot(value, value).sqrt();
    match surface {
        SurfaceGeometry::Plane { origin, normal, .. } => {
            Some(dot(subtract(point, *origin), *normal).abs() / norm(*normal))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => {
            let delta = subtract(point, *origin);
            let axis_length = norm(*axis);
            let axial = dot(delta, *axis) / axis_length;
            let radial = Vector3::new(
                delta.x - axis.x * axial / axis_length,
                delta.y - axis.y * axial / axis_length,
                delta.z - axis.z * axial / axis_length,
            );
            Some((norm(radial) - radius).abs())
        }
        SurfaceGeometry::Sphere { center, radius, .. } => {
            Some((norm(subtract(point, *center)) - radius).abs())
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let delta = subtract(point, *center);
            let axis_length = norm(*axis);
            let axial = dot(delta, *axis) / axis_length;
            let radial = Vector3::new(
                delta.x - axis.x * axial / axis_length,
                delta.y - axis.y * axial / axis_length,
                delta.z - axis.z * axial / axis_length,
            );
            Some(
                (((norm(radial) - major_radius).powi(2) + axial.powi(2)).sqrt() - minor_radius)
                    .abs(),
            )
        }
        SurfaceGeometry::Transformed { basis, transform } if transform.is_proper_rigid() => {
            analytic_surface_residual(basis, transform.try_inverse_affine()?.apply_point(point))
        }
        SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
    .filter(|residual| residual.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::geometry::{Curve, Surface};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::tessellation::Tessellation;
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Shell, Vertex,
    };

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
        out.extend(descriptor(4, 8, 4, &[0; 16]));
        out.extend(descriptor(4, 8, 1, &4_u32.to_le_bytes()));
        out.extend(descriptor(1, 8, 4, &[0; 4]));
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
        payload.extend(1_u32.to_le_bytes());
        payload.extend(1_u32.to_le_bytes());
        payload.extend(table());
        assert_eq!(descriptor_table_offset(&payload, 0), 8);
        assert!(parse_table_sequence(&payload, 8).is_some());
    }

    #[test]
    fn extended_face_tessellation_header_places_table_at_plus_40() {
        let mut payload = Vec::new();
        for word in [1_u32, 1, 1, 0, 0, 0x0020_1296, 0, 0, 0, 0] {
            payload.extend(word.to_le_bytes());
        }
        payload.extend(table());
        assert_eq!(descriptor_table_offset(&payload, 0), 40);
        assert!(parse_table_sequence(&payload, 40).is_some());
    }

    #[test]
    fn incomplete_extended_header_does_not_shift_the_table() {
        let mut payload = Vec::new();
        for word in [1_u32, 1, 1, 0, 0, 0, 0, 0, 0, 0] {
            payload.extend(word.to_le_bytes());
        }
        payload.extend(table());
        assert_eq!(descriptor_table_offset(&payload, 0), 8);
    }

    #[test]
    fn inconsistent_auxiliary_count_invalidates_the_table() {
        let mut payload = table();
        let list_b_count = 20 + 52 + 52 + 12;
        payload[list_b_count..list_b_count + 4].copy_from_slice(&3_u32.to_le_bytes());
        assert!(parse_table(&payload, 0).is_none());
    }

    #[test]
    fn analytic_surface_residuals_measure_normal_distance() {
        let plane = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 2.0),
            normal: Vector3::new(0.0, 0.0, 2.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let cylinder = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 2.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 3.0,
        };
        let sphere = SurfaceGeometry::Sphere {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        };
        let torus = SurfaceGeometry::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 5.0,
            minor_radius: 2.0,
        };

        for (surface, point, displaced) in [
            (
                &plane,
                Point3::new(3.0, 4.0, 2.0),
                Point3::new(3.0, 4.0, 2.5),
            ),
            (
                &cylinder,
                Point3::new(3.0, 0.0, 7.0),
                Point3::new(3.5, 0.0, 7.0),
            ),
            (
                &sphere,
                Point3::new(5.0, 2.0, 3.0),
                Point3::new(5.5, 2.0, 3.0),
            ),
            (
                &torus,
                Point3::new(7.0, 0.0, 0.0),
                Point3::new(7.5, 0.0, 0.0),
            ),
        ] {
            assert_eq!(analytic_surface_residual(surface, point), Some(0.0));
            assert!(analytic_surface_residual(surface, displaced)
                .is_some_and(|residual| residual > 0.0));
        }
    }

    fn add_square_face(model: &mut cadmpeg_ir::document::Model, name: &str, x: f64) -> FaceId {
        let face_id = FaceId(format!("face-{name}"));
        let loop_id = LoopId(format!("loop-{name}"));
        let surface_id = SurfaceId(format!("surface-{name}"));
        model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        let corners = [
            Point3::new(x, -1.0, 0.0),
            Point3::new(x + 2.0, -1.0, 0.0),
            Point3::new(x + 2.0, 1.0, 0.0),
            Point3::new(x, 1.0, 0.0),
        ];
        let coedge_ids = (0..4)
            .map(|index| CoedgeId(format!("coedge-{name}-{index}")))
            .collect::<Vec<_>>();
        for (index, corner) in corners.iter().copied().enumerate() {
            let point_id = PointId(format!("point-{name}-{index}"));
            let vertex_id = VertexId(format!("vertex-{name}-{index}"));
            model.points.push(Point {
                id: point_id.clone(),
                position: corner,
                source_object: None,
            });
            model.vertices.push(Vertex {
                id: vertex_id,
                point: point_id,
                tolerance: None,
            });
        }
        for (index, origin) in corners.iter().copied().enumerate() {
            let next = (index + 1) % 4;
            let curve_id = CurveId(format!("curve-{name}-{index}"));
            let edge_id = EdgeId(format!("edge-{name}-{index}"));
            let direction = corners[next].vector_from(origin).unit().unwrap();
            model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Line { origin, direction },
                source_object: None,
            });
            model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: VertexId(format!("vertex-{name}-{index}")),
                end: VertexId(format!("vertex-{name}-{next}")),
                param_range: None,
                tolerance: None,
            });
            model.coedges.push(Coedge {
                id: coedge_ids[index].clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                next: coedge_ids[next].clone(),
                previous: coedge_ids[(index + 3) % 4].clone(),
                radial_next: coedge_ids[index].clone(),
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
            });
        }
        model.loops.push(Loop {
            id: loop_id.clone(),
            face: face_id.clone(),
            boundary_role: LoopBoundaryRole::Outer,
            coedges: coedge_ids,
            vertex_uses: Vec::new(),
        });
        model.faces.push(Face {
            id: face_id.clone(),
            shell: ShellId("shell".into()),
            surface: surface_id,
            sense: Sense::Forward,
            loops: vec![loop_id],
            name: None,
            color: None,
            tolerance: None,
        });
        face_id
    }

    #[test]
    fn bounded_planar_trim_selects_between_coincident_supports() {
        let mut model = cadmpeg_ir::document::Model {
            bodies: vec![Body {
                id: BodyId("body".into()),
                kind: BodyKind::Solid,
                regions: vec![RegionId("region".into())],
                transform: None,
                name: None,
                color: None,
                visible: None,
            }],
            regions: vec![Region {
                id: RegionId("region".into()),
                body: BodyId("body".into()),
                shells: vec![ShellId("shell".into())],
            }],
            shells: vec![Shell {
                id: ShellId("shell".into()),
                region: RegionId("region".into()),
                faces: Vec::new(),
                wire_edges: Vec::new(),
                free_vertices: Vec::new(),
            }],
            ..Default::default()
        };
        let first = add_square_face(&mut model, "first", -4.0);
        let second = add_square_face(&mut model, "second", 2.0);
        model.shells[0].faces = vec![first.clone(), second.clone()];
        model.tessellations.push(Tessellation {
            id: "mesh".into(),
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices: vec![
                Point3::new(2.25, -0.75, 0.0),
                Point3::new(3.75, -0.75, 0.0),
                Point3::new(3.0, 0.75, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
            feature_edges: Vec::new(),
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: Vec::new(),
        });

        assert_eq!(assign_unique_analytic_owners(&mut model), vec!["mesh"]);
        assert_eq!(model.tessellations[0].faces, vec![second]);
        assert_eq!(model.tessellations[0].body, Some(BodyId("body".into())));

        model
            .faces
            .iter_mut()
            .find(|face| face.id == first)
            .unwrap()
            .loops
            .clear();
        model.tessellations[0].body = None;
        model.tessellations[0].faces.clear();
        assert!(assign_unique_analytic_owners(&mut model).is_empty());
        assert!(model.tessellations[0].faces.is_empty());
    }

    #[test]
    fn circular_hole_excludes_crossing_triangles_but_allows_boundary_chords() {
        let trim = PlanarTrim {
            frame: PlaneFrame {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
                v_axis: Vector3::new(0.0, 1.0, 0.0),
            },
            outer: vec![
                Point2::new(-3.0, -3.0),
                Point2::new(3.0, -3.0),
                Point2::new(3.0, 3.0),
                Point2::new(-3.0, 3.0),
            ],
            holes: vec![CircularHole {
                center: Point2::new(0.0, 0.0),
                radius: 1.0,
            }],
        };
        let mesh = |vertices, triangle| Tessellation {
            id: "mesh".into(),
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices,
            triangles: vec![triangle],
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            feature_edges: Vec::new(),
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: Vec::new(),
        };
        let boundary_chord = mesh(
            vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(2.0, 2.0, 0.0),
            ],
            [0, 1, 2],
        );
        let crossing = mesh(
            vec![
                Point3::new(-2.0, 0.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(0.0, 2.0, 0.0),
            ],
            [0, 1, 2],
        );

        assert!(trim.contains_mesh(
            &boundary_chord,
            cadmpeg_ir::transform::Transform::identity(),
            1.0e-9
        ));
        assert!(!trim.contains_mesh(
            &crossing,
            cadmpeg_ir::transform::Transform::identity(),
            1.0e-9
        ));
    }
}
