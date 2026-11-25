// SPDX-License-Identifier: Apache-2.0
//! Exact generator joins and complete result-face construction checks.

use crate::feature::history_views::{ResultStates, StateModel};
use crate::feature::profile::selection::{PathCurve, PathSpan, SweepCurve};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::sweeps::SolidSweepOperation;
use cadmpeg_ir::geometry::analytic::{ConeSurface, PlaneSurface, TorusSurface};
use cadmpeg_ir::geometry::lofts::LoftMemberForm;
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{BodyKind, Face};
use cadmpeg_ir::CadIr;
use std::collections::{HashMap, HashSet};

const EPS_RESULT_POINT_MM: f64 = 1.0e-5;
const EPS_SWEEP_RADIUS_MM: f64 = 1.0e-5;
/// Largest distance between a side-face axis or center and the joined path
/// curve.
const EPS_SWEEP_PATH_MM: f64 = 1.0e-5;
/// Largest sine of the angle between a cylinder axis and its path line.
const EPS_SWEEP_AXIS: f64 = 1.0e-6;

/// Largest deviation of a unit-axis cone ratio from one.
const EPS_SWEEP_RATIO: f64 = 1.0e-6;

struct NamedFace<'a> {
    face: &'a Face,
    family: &'a str,
    values: Vec<i64>,
}

fn owned_faces(ir: &CadIr, generator: u32) -> Option<Vec<NamedFace<'_>>> {
    owned_faces_of(ir, generator, true)
}

/// The faces that carry a naming attribute of the generator, all in one solid
/// body. With `whole_body`, they are every face of that body.
fn owned_faces_of(ir: &CadIr, generator: u32, whole_body: bool) -> Option<Vec<NamedFace<'_>>> {
    let mut names = HashMap::new();
    for attribute in &ir.model.attributes {
        let AttributeTarget::Face(face) = &attribute.target.entity else {
            continue;
        };
        if attribute.name == LEGACY_SWEEP_CLASS {
            // A malformed record that does not name this generator first
            // leaves its face unowned, so one bad record elsewhere does not
            // stop the check. A record that names the generator first but
            // lacks the owned form refuses the sweep.
            match legacy_name_tail(&attribute.values, generator) {
                LegacyOwner::Generator(tail) => {
                    if names
                        .insert(face.as_str(), ("INV_NMX_SWEEPGENERATED_TAG", tail))
                        .is_some()
                    {
                        return None;
                    }
                }
                LegacyOwner::Other => {}
                LegacyOwner::Refused => return None,
            }
            continue;
        }
        let [_, _, _, _, _, Some(AttributeValue::String(family)), Some(AttributeValue::Integer(1)), Some(AttributeValue::Integer(44)), Some(AttributeValue::Integer(1)), Some(AttributeValue::Integer(source)), Some(AttributeValue::Integer(-1)), rest @ ..] =
            attribute.values.as_slice()
        else {
            continue;
        };
        if attribute.name != "ATTRIB_CUSTOM-attrib"
            || *source != i64::from(generator)
            || !matches!(
                family.as_str(),
                "INV_NMX_GEN_TAG" | "INV_NMX_SWEEPGENERATED_TAG" | "INV_NMX_ORDERED_GEN_TAG"
            )
        {
            continue;
        }
        let values = rest
            .iter()
            .map(|value| match value {
                Some(AttributeValue::Integer(value)) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        if names
            .insert(face.as_str(), (family.as_str(), values))
            .is_some()
        {
            return None;
        }
    }
    let mut selected = Vec::new();
    let mut body = None;
    for face in &ir.geometry.faces {
        let Some((family, values)) = names.remove(face.id.as_str()) else {
            continue;
        };
        let shell = ir
            .geometry
            .shells
            .iter()
            .find(|shell| shell.id == face.shell)?;
        let region = ir
            .geometry
            .regions
            .iter()
            .find(|region| region.id == shell.region)?;
        if body.is_some_and(|body| body != &region.body) {
            return None;
        }
        body = Some(&region.body);
        selected.push(NamedFace {
            face,
            family,
            values,
        });
    }
    let body = body?;
    if !names.is_empty()
        || ir
            .geometry
            .bodies
            .iter()
            .find(|candidate| &candidate.id == body)?
            .kind
            != BodyKind::Solid
    {
        return None;
    }
    let shells = ir
        .geometry
        .shells
        .iter()
        .filter(|shell| {
            ir.geometry
                .regions
                .iter()
                .any(|region| region.id == shell.region && &region.body == body)
        })
        .map(|shell| &shell.id)
        .collect::<HashSet<_>>();
    if whole_body
        && ir
            .geometry
            .faces
            .iter()
            .filter(|face| shells.contains(&face.shell))
            .count()
            != selected.len()
    {
        return None;
    }
    Some(selected)
}

pub(in crate::feature::loft_sweep) fn sweep(
    ir: &CadIr,
    generator: u32,
    radius: f64,
    profile: i64,
    path: &[(i64, Option<PathCurve>)],
    endpoints: [Point3; 2],
) -> Option<()> {
    let geometry = &ir.geometry;
    let faces = owned_faces(ir, generator)?;
    let joined = |span: i64| path.iter().find(|(name, _)| *name == span)?.1;
    let mut caps = HashMap::new();
    let mut spans = HashSet::new();
    for named in &faces {
        if named.family != "INV_NMX_SWEEPGENERATED_TAG" {
            return None;
        }
        let [section, span, role, tail @ ..] = named.values.as_slice() else {
            return None;
        };
        if tail != [0; 7] {
            return None;
        }
        let surface = geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == named.face.surface)?;
        let SurfaceGeometry::Solved(geometry) = &surface.geometry else {
            return None;
        };
        match (section, span, role, geometry) {
            (0, 0, 4 | 12, SolvedSurfaceGeometry::Plane(_)) => {
                if caps
                    .insert(*role, cap_center(&ir.geometry, named.face, radius)?)
                    .is_some()
                {
                    return None;
                }
            }
            (section, span, 20, SolvedSurfaceGeometry::Cylinder(cylinder))
                if *section == profile
                    && (cylinder.radius().get() - radius).abs() <= EPS_SWEEP_RADIUS_MM
                    && joined(*span).is_none_or(|curve| along_line(cylinder, curve)) =>
            {
                if !spans.insert(*span) {
                    return None;
                }
            }
            (section, span, 20, SolvedSurfaceGeometry::Torus(torus))
                if *section == profile
                    && (torus.minor_radius().get().abs() - radius).abs() <= EPS_SWEEP_RADIUS_MM
                    && joined(*span).is_none_or(|curve| about_arc(torus, curve)) =>
            {
                if !spans.insert(*span) {
                    return None;
                }
            }
            _ => return None,
        }
    }
    if caps.len() != 2
        || spans.len() != path.len()
        || !path.iter().all(|(name, _)| spans.contains(name))
    {
        return None;
    }
    if !((same_point(endpoints[0], caps[&4]) && same_point(endpoints[1], caps[&12]))
        || (same_point(endpoints[1], caps[&4]) && same_point(endpoints[0], caps[&12])))
    {
        return None;
    }
    Some(())
}

/// A rigid motion of space: `point -> rotation * point + shift`.
#[derive(Clone, Copy)]
struct Pose {
    rotation: [[f64; 3]; 3],
    shift: Vector3,
}

impl Pose {
    const IDENTITY: Self = Self {
        rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        shift: Vector3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    };

    fn direction(&self, vector: Vector3) -> Vector3 {
        let [x, y, z] = self.rotation;
        Vector3::new(
            x[0] * vector.x + x[1] * vector.y + x[2] * vector.z,
            y[0] * vector.x + y[1] * vector.y + y[2] * vector.z,
            z[0] * vector.x + z[1] * vector.y + z[2] * vector.z,
        )
    }

    fn point(&self, point: Point3) -> Point3 {
        let moved = self.direction(Vector3::new(point.x, point.y, point.z));
        Point3::new(
            moved.x + self.shift.x,
            moved.y + self.shift.y,
            moved.z + self.shift.z,
        )
    }

    /// This motion followed by the translation `by`.
    fn translated(self, by: Vector3) -> Self {
        Self {
            rotation: self.rotation,
            shift: Vector3::new(
                self.shift.x + by.x,
                self.shift.y + by.y,
                self.shift.z + by.z,
            ),
        }
    }

    /// This motion followed by the turn of `angle` about the line through
    /// `center` along the unit vector `axis`.
    fn turned(self, center: Point3, axis: Vector3, angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        let (x, y, z) = (axis.x, axis.y, axis.z);
        let turn = [
            [
                cos + x * x * (1.0 - cos),
                x * y * (1.0 - cos) - z * sin,
                x * z * (1.0 - cos) + y * sin,
            ],
            [
                y * x * (1.0 - cos) + z * sin,
                cos + y * y * (1.0 - cos),
                y * z * (1.0 - cos) - x * sin,
            ],
            [
                z * x * (1.0 - cos) - y * sin,
                z * y * (1.0 - cos) + x * sin,
                cos + z * z * (1.0 - cos),
            ],
        ];
        let by = Pose {
            rotation: turn,
            shift: Vector3::new(0.0, 0.0, 0.0),
        };
        let pivot = by.direction(Vector3::new(center.x, center.y, center.z));
        let mut rotation = [[0.0; 3]; 3];
        for (row, out) in rotation.iter_mut().enumerate() {
            for (column, slot) in out.iter_mut().enumerate() {
                *slot = (0..3)
                    .map(|k| turn[row][k] * self.rotation[k][column])
                    .sum();
            }
        }
        let carried = by.direction(self.shift);
        Self {
            rotation,
            shift: Vector3::new(
                carried.x + center.x - pivot.x,
                carried.y + center.y - pivot.y,
                carried.z + center.z - pivot.z,
            ),
        }
    }

    fn curve(&self, curve: SweepCurve) -> SweepCurve {
        match curve {
            SweepCurve::Line { start, end } => SweepCurve::Line {
                start: self.point(start),
                end: self.point(end),
            },
            SweepCurve::Arc {
                center,
                radius,
                normal,
            } => SweepCurve::Arc {
                center: self.point(center),
                radius,
                normal: self.direction(normal),
            },
        }
    }
}

/// The signed angle from `from` to `to` about the unit vector `axis`.
fn signed_angle(axis: Vector3, from: Vector3, to: Vector3) -> f64 {
    axis.dot(from.cross(to)).atan2(from.dot(to))
}

/// The path spans run from one end of the path, each from its start end, and
/// the motion that carries the section from the start of each span and from
/// the end of the path.
fn section_motion(path: &[PathSpan], from_last: bool) -> Option<(Vec<(PathSpan, Pose)>, Pose)> {
    let ordered: Vec<PathSpan> = if from_last {
        path.iter()
            .rev()
            .map(|span| PathSpan {
                from: span.to,
                to: span.from,
                ..*span
            })
            .collect()
    } else {
        path.to_vec()
    };
    let mut pose = Pose::IDENTITY;
    let mut motions = Vec::new();
    for span in ordered {
        motions.push((span, pose));
        pose = match span.curve {
            SweepCurve::Line { .. } => pose.translated(span.to.vector_from(span.from)),
            SweepCurve::Arc { center, normal, .. } => {
                let axis = normal.unit()?;
                let half = signed_angle(
                    axis,
                    span.from.vector_from(center),
                    span.middle?.vector_from(center),
                );
                pose.turned(center, axis, 2.0 * half)
            }
        };
    }
    Some((motions, pose))
}

/// Check every generator-owned face of a solid sweep of a closed line and
/// arc section along a tangent-continuous line and arc path.
///
/// The path starts at an end that lies in the plane of the section, and the
/// result holds for that start. The section turns with the path, so its
/// position at the start of each span is its position at the path start moved
/// by the spans before. Each side
/// face (role 20) names its section curve and path span, and its surface is
/// the one that the moved section curve generates along the span. A new body
/// owns one side face for every pair and both caps, and the caps hold the
/// section at the two path ends; a cut or join can own any subset of faces,
/// each of which matches and each cap of which holds the section at a path
/// end. A cut or join splits and merges faces, so it can own several faces
/// for one pair or cap role, and its bodies keep their other faces. A cap holds the section when its plane is the plane of the moved
/// section.
///
/// `section` lists every edge of the section wire; an edge cut by other
/// curves repeats the name of its curve.
pub(in crate::feature::loft_sweep) fn sweep_partition(
    ir: &CadIr,
    generator: u32,
    op: SolidSweepOperation,
    section: &[(i64, SweepCurve)],
    plane: (Point3, Vector3),
    path: &[PathSpan],
) -> Option<()> {
    if !matches!(
        op,
        SolidSweepOperation::NewBody | SolidSweepOperation::Cut | SolidSweepOperation::Join
    ) {
        return None;
    }
    let normal = plane.1.unit()?;
    let in_plane =
        |point: Point3| point.vector_from(plane.0).dot(normal).abs() <= EPS_SWEEP_PATH_MM;
    // Both ends lie in the plane when the path leaves it and comes back; the
    // faces then say which end is the start.
    [
        (in_plane(path.first()?.from), false),
        (in_plane(path.last()?.to), true),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .find_map(|(_, from_last)| {
        let (motions, end) = section_motion(path, from_last)?;
        partition_from(
            ir,
            generator,
            op,
            section,
            normal,
            path.len(),
            &motions,
            end,
        )
    })
}

/// The faces of the sweep against the section moved along `motions`, which
/// end with the motion `end`.
fn partition_from(
    ir: &CadIr,
    generator: u32,
    op: SolidSweepOperation,
    section: &[(i64, SweepCurve)],
    section_normal: Vector3,
    spans: usize,
    motions: &[(PathSpan, Pose)],
    end: Pose,
) -> Option<()> {
    let new_body = op == SolidSweepOperation::NewBody;
    // The section lists every edge; an edge cut by other curves repeats its
    // name.
    let names = section
        .iter()
        .map(|(name, _)| name)
        .collect::<HashSet<_>>()
        .len();
    // A cut or join leaves faces of the bodies it works on in the result.
    let faces = owned_faces_of(ir, generator, new_body)?;
    let mut caps: Vec<(i64, &PlaneSurface)> = Vec::new();
    let mut pairs = HashSet::new();
    let mut repeated = false;
    for named in &faces {
        if named.family != "INV_NMX_SWEEPGENERATED_TAG" {
            return None;
        }
        let [name, span, role, tail @ ..] = named.values.as_slice() else {
            return None;
        };
        if tail != [0; 7] {
            return None;
        }
        let surface = ir
            .geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == named.face.surface)?;
        let SurfaceGeometry::Solved(geometry) = &surface.geometry else {
            return None;
        };
        match (name, span, role) {
            (0, 0, 4 | 12) => {
                let SolvedSurfaceGeometry::Plane(plane) = geometry else {
                    return None;
                };
                repeated |= caps.iter().any(|(seen, _)| seen == role);
                caps.push((*role, plane));
            }
            (name, span, 20) => {
                let curve = section.iter().find(|(key, _)| key == name)?.1;
                let (joined, pose) = motions.iter().find(|(joined, _)| joined.name == *span)?;
                if !surface_of_pair(pose.curve(curve), joined.curve, geometry) {
                    return None;
                }
                repeated |= !pairs.insert((*name, *span));
            }
            _ => return None,
        }
    }
    if pairs.is_empty() {
        return None;
    }
    // A cap holds the section when its plane is the plane of the moved
    // section: parallel to the moved section normal and through every edge.
    let holds = |pose: &Pose, plane: &PlaneSurface| {
        let moved = section
            .iter()
            .map(|(name, curve)| (*name, pose.curve(*curve)))
            .collect::<Vec<_>>();
        is_parallel(
            *plane.frame().axis().as_raw(),
            pose.direction(section_normal),
        ) && section_on_plane(&moved, plane)
    };
    let at_start = |plane: &PlaneSurface| holds(&Pose::IDENTITY, plane);
    let at_end = |plane: &PlaneSurface| holds(&end, plane);
    if !caps
        .iter()
        .all(|(_, plane)| at_start(plane) || at_end(plane))
    {
        return None;
    }
    // A cut or join splits and merges faces, so it can own several faces
    // for one pair or cap role. A new body owns each exactly once.
    if new_body
        && (repeated
            || caps.len() != 2
            || pairs.len() != names * spans
            || !(at_start(caps[0].1) && at_end(caps[1].1)
                || at_end(caps[0].1) && at_start(caps[1].1)))
    {
        return None;
    }
    Some(())
}

fn distance_to_line(point: Point3, origin: Point3, direction: Vector3) -> f64 {
    point.vector_from(origin).cross(direction).norm() / direction.norm()
}

fn is_parallel(left: Vector3, right: Vector3) -> bool {
    left.cross(right).norm() <= EPS_SWEEP_AXIS * left.norm() * right.norm()
}

fn is_perpendicular(left: Vector3, right: Vector3) -> bool {
    left.dot(right).abs() <= EPS_SWEEP_AXIS * left.norm() * right.norm()
}

fn on_plane(plane: &PlaneSurface, point: Point3) -> bool {
    let normal = *plane.frame().axis().as_raw();
    point.vector_from(plane.origin().get()).dot(normal).abs() <= EPS_SWEEP_PATH_MM
}

/// Whether the plane holds every edge of the section: both ends of a line,
/// and an arc whose center the plane holds and whose own plane is the plane
/// (so its end points lie in it as well).
fn section_on_plane(section: &[(i64, SweepCurve)], plane: &PlaneSurface) -> bool {
    section.iter().all(|(_, curve)| match *curve {
        SweepCurve::Line { start, end } => on_plane(plane, start) && on_plane(plane, end),
        SweepCurve::Arc { center, normal, .. } => {
            on_plane(plane, center) && is_parallel(*plane.frame().axis().as_raw(), normal)
        }
    })
}

/// Whether `surface` is the surface that the section curve generates along
/// the path curve.
fn surface_of_pair(section: SweepCurve, path: SweepCurve, surface: &SolvedSurfaceGeometry) -> bool {
    match (section, path, surface) {
        (
            SweepCurve::Line { start, end },
            SweepCurve::Line {
                start: from,
                end: to,
            },
            SolvedSurfaceGeometry::Plane(plane),
        ) => {
            let along = end.vector_from(start);
            let onward = to.vector_from(from);
            let normal = *plane.frame().axis().as_raw();
            !is_parallel(along, onward)
                && is_perpendicular(normal, along)
                && is_perpendicular(normal, onward)
                && on_plane(plane, start)
                && on_plane(plane, end)
        }
        (SweepCurve::Line { start, end }, SweepCurve::Arc { center, normal, .. }, surface) => {
            let along = end.vector_from(start);
            match surface {
                SolvedSurfaceGeometry::Plane(plane) => {
                    is_parallel(*plane.frame().axis().as_raw(), normal)
                        && is_perpendicular(normal, along)
                        && on_plane(plane, start)
                        && on_plane(plane, end)
                }
                SolvedSurfaceGeometry::Cylinder(cylinder) => {
                    let radius = distance_to_line(start, center, normal);
                    is_parallel(*cylinder.frame().axis().as_raw(), normal)
                        && is_parallel(along, normal)
                        && distance_to_line(cylinder.origin().get(), center, normal)
                            <= EPS_SWEEP_PATH_MM
                        && (cylinder.radius().get() - radius).abs() <= EPS_SWEEP_RADIUS_MM
                }
                SolvedSurfaceGeometry::Cone(cone) => {
                    // A line that skews the axis turns into a hyperboloid.
                    coplanar_with_axis(start, end, center, normal)
                        && on_cone_about(cone, center, normal, [start, end])
                }
                _ => false,
            }
        }
        (
            SweepCurve::Arc {
                center,
                radius,
                normal: plane_normal,
            },
            SweepCurve::Line { start, end },
            SolvedSurfaceGeometry::Cylinder(cylinder),
        ) => {
            let onward = end.vector_from(start);
            is_parallel(*cylinder.frame().axis().as_raw(), onward)
                && is_parallel(plane_normal, onward)
                && distance_to_line(center, cylinder.origin().get(), onward) <= EPS_SWEEP_PATH_MM
                && (cylinder.radius().get() - radius).abs() <= EPS_SWEEP_RADIUS_MM
        }
        (
            SweepCurve::Arc {
                center,
                radius,
                normal: plane_normal,
            },
            SweepCurve::Arc {
                center: pivot,
                normal: axis,
                ..
            },
            SolvedSurfaceGeometry::Torus(torus),
        ) => torus_about(torus, center, radius, plane_normal, pivot, axis),
        _ => false,
    }
}

/// Whether the line through `start` and `end` and the axis through `pivot`
/// along `axis` lie in one plane.
fn coplanar_with_axis(start: Point3, end: Point3, pivot: Point3, axis: Vector3) -> bool {
    let along = end.vector_from(start);
    axis.dot(start.vector_from(pivot).cross(along)).abs()
        <= EPS_SWEEP_PATH_MM * axis.norm() * along.norm()
}

/// Whether the circular cone is coaxial with the axis through `pivot` and
/// holds both ends of a section line.
fn on_cone_about(cone: &ConeSurface, pivot: Point3, axis: Vector3, ends: [Point3; 2]) -> bool {
    let cone_axis = *cone.frame().axis().as_raw();
    if !is_parallel(cone_axis, axis)
        || distance_to_line(cone.origin().get(), pivot, axis) > EPS_SWEEP_PATH_MM
        || (cone.ratio().get() - 1.0).abs() > EPS_SWEEP_RATIO
    {
        return false;
    }
    let slope = cone.half_angle().get().tan();
    ends.iter().all(|end| {
        let height = end.vector_from(cone.origin().get()).dot(cone_axis);
        let radial = distance_to_line(*end, pivot, axis);
        (radial - (cone.radius().get() + height * slope)).abs() <= EPS_SWEEP_PATH_MM
    })
}

/// Whether the torus is the one that a section circle, whose plane holds the
/// axis, generates about the axis through `pivot`.
fn torus_about(
    torus: &TorusSurface,
    center: Point3,
    radius: f64,
    plane_normal: Vector3,
    pivot: Point3,
    axis: Vector3,
) -> bool {
    let unit = axis.scale(1.0 / axis.norm());
    let height = center.vector_from(pivot).dot(unit);
    let foot = pivot.translated(unit, height);
    is_parallel(*torus.frame().axis().as_raw(), axis)
        && is_perpendicular(plane_normal, axis)
        && same_point_within(torus.center().get(), foot, EPS_SWEEP_PATH_MM)
        && (torus.minor_radius().get().abs() - radius).abs() <= EPS_SWEEP_RADIUS_MM
        && (torus.major_radius().get() - distance_to_line(center, pivot, axis)).abs()
            <= EPS_SWEEP_RADIUS_MM
}

/// Whether the cylinder axis is the line of a line-joined span.
fn along_line(
    cylinder: &cadmpeg_ir::geometry::analytic::CylinderSurface,
    curve: PathCurve,
) -> bool {
    let PathCurve::Line { start, end } = curve else {
        return false;
    };
    let axis = *cylinder.frame().axis().as_raw();
    let line = end.vector_from(start);
    let length = line.norm();
    let off_axis = |point: Point3| {
        let offset = point.vector_from(cylinder.origin().get());
        offset.cross(axis).norm()
    };
    length > 0.0
        && line.cross(axis).norm() <= EPS_SWEEP_AXIS * length
        && off_axis(start) <= EPS_SWEEP_PATH_MM
        && off_axis(end) <= EPS_SWEEP_PATH_MM
}

/// Whether the torus center and major radius are those of an arc-joined span.
fn about_arc(torus: &cadmpeg_ir::geometry::analytic::TorusSurface, curve: PathCurve) -> bool {
    let PathCurve::Arc { center, radius } = curve else {
        return false;
    };
    same_point_within(torus.center().get(), center, EPS_SWEEP_PATH_MM)
        && (torus.major_radius().get() - radius).abs() <= EPS_SWEEP_RADIUS_MM
}

fn same_point(left: Point3, right: Point3) -> bool {
    same_point_within(left, right, EPS_RESULT_POINT_MM)
}

fn same_point_within(left: Point3, right: Point3, tolerance: f64) -> bool {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
        <= tolerance
}

fn cap_center(geometry: &Topology, face: &Face, radius: f64) -> Option<Point3> {
    let face_loops = face.loops.boundary()?;
    if face_loops.len() != 1 {
        return None;
    }
    let id = face_loops.iter().next()?;
    let ring = geometry
        .loops
        .iter()
        .find(|ring| &ring.id == id && ring.face == face.id)?;
    if ring.coedges().is_empty() {
        return None;
    }
    let mut center = None;
    for id in ring.coedges() {
        let coedge = geometry
            .coedges
            .iter()
            .find(|coedge| &coedge.id == id && coedge.owner_loop == ring.id)?;
        if coedge.use_curve.is_some() {
            return None;
        }
        let edge = geometry.edges.iter().find(|edge| edge.id == coedge.edge)?;
        let curve = geometry
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.carrier.curve())?;
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = &curve.geometry else {
            return None;
        };
        if (circle.radius().get() - radius).abs() > EPS_SWEEP_RADIUS_MM {
            return None;
        }
        let point = circle.center().get();
        if center.is_some_and(|center| !same_point(center, point)) {
            return None;
        }
        center = Some(point);
    }
    center
}

pub(in crate::feature::loft_sweep) fn loft(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    curve_member: impl Fn(usize, i64) -> bool,
) -> Option<()> {
    let geometry = &ir.geometry;
    let faces = owned_faces(ir, generator)?;
    let mut caps = HashSet::new();
    let mut sides = HashSet::new();
    for named in faces {
        let surface = geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == named.face.surface)?;
        match (named.family, named.values.as_slice()) {
            ("INV_NMX_GEN_TAG", [0, section, 260])
                if matches!(
                    &surface.geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
                ) =>
            {
                if !sections.iter().any(|source| i64::from(*source) == *section)
                    || !caps.insert(*section)
                {
                    return None;
                }
            }
            ("INV_NMX_ORDERED_GEN_TAG", values)
                if side_tag(values).is_some_and(|tag| {
                    tag.first_section == i64::from(sections[0])
                        && tag.last_section == i64::from(sections[1])
                        && curve_member(0, tag.first_curve)
                        && curve_member(1, tag.last_curve)
                }) =>
            {
                let member = side_tag(values)?.member;
                side_surface_is_loft(geometry, &surface.geometry)?;
                if !sides.insert(usize::try_from(member).ok()?) {
                    return None;
                }
            }
            _ => return None,
        }
    }
    (sections[0] != sections[1]
        && caps.len() == 2
        && !sides.is_empty()
        && (1..=sides.len()).all(|member| sides.contains(&member)))
    .then_some(())
}

/// Whether a side-face surface is an unconstrained procedural loft with a
/// NURBS solution.
fn side_surface_is_loft(geometry: &Topology, surface: &SurfaceGeometry) -> Option<()> {
    let construction = surface.procedural_construction()?;
    let procedural = geometry
        .procedural_surfaces
        .iter()
        .find(|procedural| &procedural.id == construction)?;
    let ProceduralSurfaceDefinition::Loft(payload) = procedural.definition() else {
        return None;
    };
    (matches!(surface.solved()?, SolvedSurfaceGeometry::Nurbs(_)) && unconstrained_loft(payload))
        .then_some(())
}

/// Loft result proof from the final model alone, for the validator. The
/// final model holds the result faces unless a later feature consumed or
/// split a cap, or copied faces with a pattern or a mirror. The history state
/// after the loft holds them then (spec section 13.2), and the validator
/// cannot decode the state. `history_bound` says that the loft has a bound
/// history-state record of the loft kind. The loft stays proved then when no
/// generator-owned face of the final model contradicts it.
///
/// A loft proved this way is screened for contradicting faces, not proved
/// again: the state is not persisted with its faces, so the validator has
/// nothing to prove the result against. The projector proved it when it
/// decoded the state.
pub(in crate::feature::loft_sweep) fn loft_final_proof(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
    history_bound: bool,
) -> Option<()> {
    if loft_result(ir, generator, sections, &section_curves).is_some() {
        return Some(());
    }
    (history_bound && loft_owned_consistent(ir, generator, sections, &section_curves)).then_some(())
}

/// Whether no generator-owned face of `ir` contradicts the loft. A later
/// feature removes a cap or a side face, splits it, or copies it with a
/// pattern or a mirror. Removal leaves gaps in the member indices and a copy
/// repeats a name, so neither contradicts the loft, and a face that is gone
/// proves nothing. A named face contradicts the loft when no such operation
/// produces it: a cap of a section that the loft does not use, a side face
/// whose first or last section or curve differs from the loft, a side member
/// index below one, or a name of another family. An empty owned set is
/// consistent.
///
/// The screen reads names and tails only, not surfaces. The projector proves
/// the surface of each cap and side face on the state after the loft, and the
/// final model can hold a face with another surface when a later feature
/// re-surfaced it, which section 13.2 does not forbid. A surface of the final
/// model therefore does not contradict a loft that the state proved. A legacy
/// naming attribute that the projector refuses (a form other than the
/// count-prefixed pairs of integers, or a source count below one) refuses
/// the screen as well, whichever generator it names.
pub(in crate::feature::loft_sweep) fn loft_owned_consistent(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> bool {
    if sections[0] == sections[1] {
        return false;
    }
    let first_curves = section_curves(0);
    let last_curves = section_curves(1);
    let uses = |section: i64| sections.iter().any(|source| i64::from(*source) == section);
    let faces = ir
        .geometry
        .faces
        .iter()
        .map(|face| face.id.as_str())
        .collect::<HashSet<_>>();
    for attribute in &ir.model.attributes {
        let AttributeTarget::Face(id) = &attribute.target.entity else {
            continue;
        };
        let name = attribute.name.as_str();
        let integers = |values: &[Option<AttributeValue>]| {
            values
                .iter()
                .map(|value| match value {
                    Some(AttributeValue::Integer(value)) => Some(*value),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
        };
        let consistent = if name == "ATTRIB_CUSTOM-attrib" {
            let [_, _, _, _, _, Some(AttributeValue::String(family)), Some(AttributeValue::Integer(1)), Some(AttributeValue::Integer(44)), Some(AttributeValue::Integer(1)), Some(AttributeValue::Integer(source)), Some(AttributeValue::Integer(-1)), rest @ ..] =
                attribute.values.as_slice()
            else {
                continue;
            };
            if *source != i64::from(generator)
                || !matches!(
                    family.as_str(),
                    "INV_NMX_GEN_TAG" | "INV_NMX_SWEEPGENERATED_TAG" | "INV_NMX_ORDERED_GEN_TAG"
                )
            {
                continue;
            }
            if !faces.contains(id.as_str()) {
                return false;
            }
            match (family.as_str(), integers(rest)) {
                ("INV_NMX_GEN_TAG", Some(values)) => {
                    matches!(values[..], [0, section, 260] if uses(section))
                }
                ("INV_NMX_ORDERED_GEN_TAG", Some(values)) => side_tag(&values).is_some_and(|tag| {
                    tag.first_section == i64::from(sections[0])
                        && tag.last_section == i64::from(sections[1])
                        && first_curves.contains(&tag.first_curve)
                        && last_curves.contains(&tag.last_curve)
                        && tag.member >= 1
                }),
                _ => false,
            }
        } else if name == LEGACY_CAP_CLASS || name == LEGACY_SIDE_CLASS {
            // `legacy_owned_faces` refuses the whole loft for a legacy
            // attribute of another form, so the screen does too.
            let [_, _, _, _, _, Some(AttributeValue::Integer(_)), Some(AttributeValue::Integer(count)), rest @ ..] =
                attribute.values.as_slice()
            else {
                return false;
            };
            let Some(integers) = integers(rest) else {
                return false;
            };
            let Some(pairs) = usize::try_from(*count)
                .ok()
                .and_then(|count| count.checked_mul(2))
                .filter(|pairs| *count >= 1 && *pairs <= integers.len())
            else {
                return false;
            };
            let (names, tail) = integers.split_at(pairs);
            if !names
                .chunks_exact(2)
                .any(|pair| pair[0] == i64::from(generator))
            {
                continue;
            }
            if !faces.contains(id.as_str()) {
                return false;
            }
            *count == 1
                && names[1] == -1
                && if name == LEGACY_CAP_CLASS {
                    matches!(tail[..], [0, section, 260] if uses(section))
                } else {
                    matches!(
                        tail[..],
                        [2, first_curve, first_section, 258, 0, 0, last_curve, last_section, 258, 0, 0]
                            if first_section == i64::from(sections[0])
                                && last_section == i64::from(sections[1])
                                && first_curves.contains(&first_curve)
                                && last_curves.contains(&last_curve)
                    )
                }
        } else {
            continue;
        };
        if !consistent {
            return false;
        }
    }
    true
}

/// Loft result check for either attribute carrier. `section_curves` lists the
/// associative curve names of a section boundary.
pub(in crate::feature::loft_sweep) fn loft_result(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> Option<()> {
    loft(ir, generator, sections, |section, name| {
        section_curves(section).contains(&name)
    })
    .or_else(|| legacy_loft(ir, generator, sections, &section_curves))
}

/// Why a loft result is not proved.
#[derive(Debug)]
pub(in crate::feature::loft_sweep) enum Unproved {
    /// Neither the final model nor the state after the loft holds a complete
    /// generator-owned result.
    Result,
    /// The history state of the loft does not decode.
    StateFailed,
    /// The views of history states passed their work cap.
    Bounded,
    /// The decode of a state failed.
    Codec(CodecError),
}

/// Loft result proof. The final model holds the result faces unless a later
/// feature removed or copied them. The history state that first names the
/// loft holds them in that case (spec section 13.2).
pub(in crate::feature::loft_sweep) fn loft_proof(
    ctx: &DecodeContext<'_>,
    final_model: Option<&CadIr>,
    states: Option<&dyn ResultStates>,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> Result<(), Unproved> {
    if final_model
        .is_some_and(|model| loft_result(model, generator, sections, &section_curves).is_some())
    {
        return Ok(());
    }
    let Some(states) = states else {
        return Err(Unproved::Result);
    };
    match &*states.after(ctx, generator).map_err(Unproved::Codec)? {
        StateModel::Model(model) => {
            loft_result(model, generator, sections, &section_curves).ok_or(Unproved::Result)
        }
        StateModel::Unavailable => Err(Unproved::Result),
        StateModel::Failed => Err(Unproved::StateFailed),
        StateModel::Bounded => Err(Unproved::Bounded),
    }
}

const LEGACY_SWEEP_CLASS: &str =
    "NMx_Sweep_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";
const LEGACY_CAP_CLASS: &str =
    "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";
const LEGACY_SIDE_CLASS: &str =
    "NMx_Loft_Surface_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";

/// The owner of a legacy naming attribute.
enum LegacyOwner {
    /// The first source name is the generator with name value -1; the tail
    /// belongs to it.
    Generator(Vec<i64>),
    /// Another feature's name comes first, or the record has no readable
    /// first source name.
    Other,
    /// The first source name is the generator, but the record lacks the
    /// owned form.
    Refused,
}

/// The owner of a legacy naming attribute.
fn legacy_name_tail(values: &[Option<AttributeValue>], generator: u32) -> LegacyOwner {
    let [_, _, _, _, _, Some(AttributeValue::Integer(_)), Some(AttributeValue::Integer(count)), rest @ ..] =
        values
    else {
        return LegacyOwner::Other;
    };
    let Some(Some(AttributeValue::Integer(first))) = rest.first() else {
        return LegacyOwner::Other;
    };
    if *count < 1 || *first != i64::from(generator) {
        return LegacyOwner::Other;
    }
    let owned = || {
        let pairs = usize::try_from(*count).ok()?.checked_mul(2)?;
        let integers = rest
            .iter()
            .map(|value| match value {
                Some(AttributeValue::Integer(value)) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        if pairs > integers.len() {
            return None;
        }
        let (names, tail) = integers.split_at(pairs);
        (names[1] == -1).then(|| tail.to_vec())
    };
    owned().map_or(LegacyOwner::Refused, LegacyOwner::Generator)
}

/// A face that carries a generator-owned naming attribute of the legacy class chains.
struct LegacyFace<'a> {
    face: &'a Face,
    cap: bool,
    tail: Vec<i64>,
}

/// Faces named by `generator` in a legacy carrier, in model order. The faces
/// belong to one solid body. Other faces of the body can belong to later features.
fn legacy_owned_faces(ir: &CadIr, generator: u32) -> Option<Vec<LegacyFace<'_>>> {
    let mut names = HashMap::new();
    for attribute in &ir.model.attributes {
        let AttributeTarget::Face(face) = &attribute.target.entity else {
            continue;
        };
        let cap = match attribute.name.as_str() {
            LEGACY_CAP_CLASS => true,
            LEGACY_SIDE_CLASS => false,
            _ => continue,
        };
        let [_, _, _, _, _, Some(AttributeValue::Integer(_)), Some(AttributeValue::Integer(count)), rest @ ..] =
            attribute.values.as_slice()
        else {
            return None;
        };
        let pairs = usize::try_from(*count).ok()?.checked_mul(2)?;
        let integers = rest
            .iter()
            .map(|value| match value {
                Some(AttributeValue::Integer(value)) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        if *count < 1 || pairs > integers.len() {
            return None;
        }
        let (names_part, tail) = integers.split_at(pairs);
        let named = names_part
            .chunks_exact(2)
            .any(|pair| pair[0] == i64::from(generator));
        if !named {
            continue;
        }
        if *count != 1 || names_part[1] != -1 {
            return None;
        }
        if names.insert(face.as_str(), (cap, tail.to_vec())).is_some() {
            return None;
        }
    }
    let mut selected = Vec::new();
    let mut body = None;
    for face in &ir.geometry.faces {
        let Some((cap, tail)) = names.remove(face.id.as_str()) else {
            continue;
        };
        let shell = ir
            .geometry
            .shells
            .iter()
            .find(|shell| shell.id == face.shell)?;
        let region = ir
            .geometry
            .regions
            .iter()
            .find(|region| region.id == shell.region)?;
        if body.is_some_and(|body| body != &region.body) {
            return None;
        }
        body = Some(&region.body);
        selected.push(LegacyFace { face, cap, tail });
    }
    let body = body?;
    (names.is_empty()
        && ir
            .geometry
            .bodies
            .iter()
            .find(|candidate| &candidate.id == body)?
            .kind
            == BodyKind::Solid)
        .then_some(selected)
}

/// Loft result check for a legacy carrier. The generator names both caps and
/// one planar side face for each curve of both section boundaries. Later
/// features can add faces to the body.
pub(in crate::feature::loft_sweep) fn legacy_loft(
    ir: &CadIr,
    generator: u32,
    sections: [u32; 2],
    section_curves: impl Fn(usize) -> Vec<i64>,
) -> Option<()> {
    let geometry = &ir.geometry;
    if sections[0] == sections[1] {
        return None;
    }
    let mut caps = HashSet::new();
    let mut first_curves = HashSet::new();
    let mut last_curves = HashSet::new();
    for named in legacy_owned_faces(ir, generator)? {
        let surface = geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == named.face.surface)?;
        if !matches!(
            &surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
        ) {
            return None;
        }
        if named.cap {
            let [0, section, 260] = named.tail[..] else {
                return None;
            };
            if !sections.iter().any(|source| i64::from(*source) == section) || !caps.insert(section)
            {
                return None;
            }
        } else {
            let [2, first_curve, first_section, 258, 0, 0, last_curve, last_section, 258, 0, 0] =
                named.tail[..]
            else {
                return None;
            };
            if first_section != i64::from(sections[0])
                || last_section != i64::from(sections[1])
                || !first_curves.insert(first_curve)
                || !last_curves.insert(last_curve)
            {
                return None;
            }
        }
    }
    let complete = |found: &HashSet<i64>, section: usize| {
        let expected = section_curves(section);
        !expected.is_empty()
            && found.len() == expected.len()
            && expected.iter().all(|name| found.contains(name))
    };
    (caps.len() == 2 && complete(&first_curves, 0) && complete(&last_curves, 1)).then_some(())
}

/// The section groups and member index of a loft side-face tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SideTag {
    first_curve: i64,
    first_section: i64,
    last_curve: i64,
    last_section: i64,
    member: i64,
}

/// Reads a loft side-face tag. The tag holds two groups
/// `curve, section, 258, tail_0, tail_1` in loft-section order. One group has
/// tail `0, 0`, and the other group has tail `4, member`.
fn side_tag(values: &[i64]) -> Option<SideTag> {
    let [2, first_curve, first_section, 258, a_0, a_1, last_curve, last_section, 258, b_0, b_1] =
        *values
    else {
        return None;
    };
    let (([0, 0], [4, member]) | ([4, member], [0, 0])) = ([a_0, a_1], [b_0, b_1]) else {
        return None;
    };
    Some(SideTag {
        first_curve,
        first_section,
        last_curve,
        last_section,
        member,
    })
}

fn unconstrained_loft(
    payload: &cadmpeg_ir::geometry::surface_payloads::LoftSurfacePayload,
) -> bool {
    if payload.closures() != &[0, 0] || *payload.mode() != 0 || !payload.bridge().is_empty() {
        return false;
    }
    let [primary, secondary] = payload.sections();
    if primary.entries.len() != 2 || !secondary.entries.is_empty() {
        return false;
    }
    for entry in &primary.entries {
        if entry.path.path.is_some()
            || !entry.path.auxiliaries.is_empty()
            || entry.profile.len() != 1
        {
            return false;
        }
        if !matches!(
            &entry.profile[0].form,
            LoftMemberForm::Support {
                type_code: 1,
                surface: None,
                support_bounds: [None, None, None, None],
                pcurve: None,
                first_flag: false,
                direction: None,
                ..
            }
        ) {
            return false;
        }
    }
    true
}

#[cfg(test)]
pub(in crate::feature::loft_sweep) mod tests;
