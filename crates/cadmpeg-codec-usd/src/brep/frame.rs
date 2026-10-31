// SPDX-License-Identifier: Apache-2.0
//! The frame that a connector's origin rule derives from one face of a
//! B-rep (spec §18.2).
//!
//! Frames are row-vector matrices, as `xformOp:transform`: rows `x`, `y`,
//! `z`, and the origin. The face boundary is sampled along each edge curve,
//! so the area centroid and the axial extent are exact for straight edges
//! and within the chord slack for curved ones.

use super::{Brep, Catalog, CurveKind, SurfaceKind};

/// A rigid frame: rows `x`, `y`, `z`, and the origin.
pub(crate) type RowFrame = [[f64; 3]; 4];

/// The origin rules (spec §18.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OriginRule {
    FaceFrame,
    Axis { high: bool },
    Centre,
}

impl OriginRule {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::FaceFrame => "faceFrame",
            Self::Axis { .. } => "axis",
            Self::Centre => "centre",
        }
    }
}

/// Samples along each edge curve in the boundary of a face.
const SAMPLES: usize = 64;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(a, a).sqrt();
    (length.is_finite() && length > 0.0).then(|| a.map(|value| value / length))
}

/// `proj(r, n)`: the unit projection of `r` perpendicular to `n`, or of the
/// cyclic partner of `n` when `r` is parallel to `n`.
pub(crate) fn projected_up(up: [f64; 3], normal: [f64; 3]) -> Option<[f64; 3]> {
    let up = unit(up)?;
    let along = dot(up, normal);
    let rest = [0, 1, 2].map(|k| up[k] - along * normal[k]);
    if dot(rest, rest).sqrt() > f64::EPSILON.sqrt() {
        return unit(rest);
    }
    let largest = (0..3)
        .reduce(|best, k| {
            if normal[k].abs() > normal[best].abs() {
                k
            } else {
                best
            }
        })
        .unwrap_or(0);
    let mut partner = [0.0; 3];
    partner[(largest + 2) % 3] = normal[largest].signum();
    let along = dot(partner, normal);
    unit([0, 1, 2].map(|k| partner[k] - along * normal[k]))
}

/// The frame with axis `z`, `y = proj(up, z)`, `x = y × z`, at `origin`.
fn framed(z: [f64; 3], up: [f64; 3], origin: [f64; 3]) -> Option<RowFrame> {
    let y = projected_up(up, z)?;
    Some([cross(y, z), y, z, origin])
}

/// The point at parameter `t` of an analytic or NURBS curve record.
fn curve_point(brep: &Brep, curve: (Catalog, usize), t: f64) -> Option<[f64; 3]> {
    match curve {
        (Catalog::Analytic, index) => {
            let record = brep.analytic_curves.get(index)?;
            let frame = &record.frame;
            let y = cross(frame.axis, frame.ref_direction);
            let t = match record.kind {
                CurveKind::Circle | CurveKind::Ellipse => t * brep.angle_scale,
                _ => t,
            };
            let param = |at: usize| record.params.get(at).copied().unwrap_or_default();
            let (along_x, along_y, along_z) = match record.kind {
                CurveKind::Line => (0.0, 0.0, t),
                CurveKind::Circle => (param(0) * t.cos(), param(0) * t.sin(), 0.0),
                CurveKind::Ellipse => (param(0) * t.cos(), param(1) * t.sin(), 0.0),
                CurveKind::Parabola => (param(0) * t * t, 2.0 * param(0) * t, 0.0),
                CurveKind::Hyperbola => (param(0) * t.cosh(), param(1) * t.sinh(), 0.0),
            };
            Some([0, 1, 2].map(|k| {
                frame.position[k]
                    + along_x * frame.ref_direction[k]
                    + along_y * y[k]
                    + along_z * frame.axis[k]
            }))
        }
        (Catalog::Nurbs, index) => {
            let record = brep.nurbs_curves.get(index)?;
            nurbs_point(record, t)
        }
    }
}

/// De Boor evaluation of a NURBS curve record.
fn nurbs_point(record: &super::NurbsCurveRecord<3>, parameter: f64) -> Option<[f64; 3]> {
    let degree = record.order.checked_sub(1)?;
    let count = record.points.len();
    let knots = &record.knots;
    let parameter = parameter.clamp(*knots.get(degree)?, *knots.get(count)?);
    let span = (degree..count).rev().find(|span| {
        knots[*span] <= parameter && (parameter < knots[span + 1] || span + 1 == count)
    })?;
    let mut work: Vec<[f64; 4]> = (span - degree..=span)
        .map(|index| {
            let weight = record.weights.get(index).copied().unwrap_or(1.0);
            let point = record.points[index];
            [
                point[0] * weight,
                point[1] * weight,
                point[2] * weight,
                weight,
            ]
        })
        .collect();
    for level in 1..=degree {
        for j in (level..=degree).rev() {
            let index = span - degree + j;
            let denominator = knots[index + degree + 1 - level] - knots[index];
            let alpha = if denominator == 0.0 {
                0.0
            } else {
                (parameter - knots[index]) / denominator
            };
            let previous = work[j - 1];
            for (value, before) in work[j].iter_mut().zip(previous) {
                *value = alpha * *value + (1.0 - alpha) * before;
            }
        }
    }
    let [x, y, z, weight] = work[degree];
    (weight != 0.0).then(|| [x / weight, y / weight, z / weight])
}

/// The boundary points of each loop of a face, in traversal order.
fn loop_points(brep: &Brep, face: usize) -> Option<Vec<Vec<[f64; 3]>>> {
    let mut loops = Vec::new();
    for loop_ in &brep.faces.get(face)?.loops {
        let mut points = Vec::new();
        for oriented in &brep.loops.get(*loop_)?.oriented_edges {
            let use_ = brep.oriented_edges.get(*oriented)?;
            let edge = brep.edges.get(use_.edge)?;
            let forward = edge.same_sense == use_.orientation;
            match edge.curve {
                Some(curve) => {
                    let [low, high] = edge.range;
                    for sample in 0..SAMPLES {
                        let fraction = cadmpeg_core::convert::f64_from_index(sample)?
                            / cadmpeg_core::convert::f64_from_index(SAMPLES)?;
                        let fraction = if forward { fraction } else { 1.0 - fraction };
                        points.push(curve_point(brep, curve, low + (high - low) * fraction)?);
                    }
                }
                None => {
                    let [first, second] = edge.vertices;
                    let start = if use_.orientation { first } else { second };
                    points.push(*brep.vertices.get(start)?);
                }
            }
        }
        loops.push(points);
    }
    Some(loops)
}

/// The analytic surface record of a face.
fn analytic_surface(brep: &Brep, face: usize) -> Option<&super::AnalyticSurface> {
    match brep.faces.get(face)?.surface {
        (Catalog::Analytic, index) => brep.analytic_surfaces.get(index),
        (Catalog::Nurbs, _) => None,
    }
}

/// The surface class of a face for the origin rules: its analytic type when
/// it is a plane, cylinder, cone, sphere, or torus.
pub(crate) fn surface_class(brep: &Brep, face: usize) -> Option<SurfaceKind> {
    analytic_surface(brep, face)
        .map(|surface| surface.kind)
        .filter(|kind| !matches!(kind, SurfaceKind::Extrusion | SurfaceKind::Revolution))
}

/// The intrinsic parameters of a surface class (spec §18.3): radii, and the
/// cone half angle in radians.
pub(crate) fn intrinsic_params(brep: &Brep, face: usize) -> Option<Vec<f64>> {
    let surface = analytic_surface(brep, face)?;
    Some(match surface.kind {
        SurfaceKind::Plane => Vec::new(),
        SurfaceKind::Cylinder | SurfaceKind::Sphere => vec![*surface.params.first()?],
        SurfaceKind::Cone => vec![*surface.params.get(1)? * brep.angle_scale],
        SurfaceKind::Torus => surface.params.clone(),
        SurfaceKind::Extrusion | SurfaceKind::Revolution => return None,
    })
}

/// A point on the face: its first boundary point.
pub(crate) fn face_point(brep: &Brep, face: usize) -> Option<[f64; 3]> {
    loop_points(brep, face)?.into_iter().flatten().next()
}

/// The frame `G` that `rule` derives from face `face`, or `None` when the
/// rule does not admit the face's class or the frame is not computable.
pub(crate) fn derived(
    brep: &Brep,
    face: usize,
    rule: OriginRule,
    up: [f64; 3],
    direction: [f64; 3],
) -> Option<RowFrame> {
    let surface = analytic_surface(brep, face)?;
    let position = surface.frame.position;
    let axis = unit(surface.frame.axis)?;
    match (rule, surface.kind) {
        (OriginRule::FaceFrame, SurfaceKind::Plane) => {
            let normal = if brep.faces.get(face)?.same_sense {
                axis
            } else {
                axis.map(|value| -value)
            };
            // Signed areas of the loop polygons about the outward normal:
            // holes wind clockwise and subtract.
            let mut area = 0.0;
            let mut moment = [0.0; 3];
            for points in loop_points(brep, face)? {
                let Some(first) = points.first().copied() else {
                    continue;
                };
                for pair in points
                    .windows(2)
                    .chain(std::iter::once([*points.last()?, first].as_slice()))
                {
                    let (a, b) = (pair[0], pair[1]);
                    let triangle = dot(
                        cross(
                            [0, 1, 2].map(|k| a[k] - first[k]),
                            [0, 1, 2].map(|k| b[k] - first[k]),
                        ),
                        normal,
                    ) / 2.0;
                    area += triangle;
                    for k in 0..3 {
                        moment[k] += triangle * (first[k] + a[k] + b[k]) / 3.0;
                    }
                }
            }
            if area.abs() <= f64::EPSILON {
                return None;
            }
            framed(normal, up, moment.map(|value| value / area))
        }
        (
            OriginRule::Axis { high },
            SurfaceKind::Cylinder | SurfaceKind::Cone | SurfaceKind::Torus,
        ) => {
            let direction = unit(direction)?;
            let alignment = dot(direction, axis);
            if alignment.abs() <= f64::EPSILON.sqrt() {
                return None;
            }
            let z = if alignment > 0.0 {
                axis
            } else {
                axis.map(|value| -value)
            };
            let heights = loop_points(brep, face)?
                .into_iter()
                .flatten()
                .map(|point| dot([0, 1, 2].map(|k| point[k] - position[k]), z));
            let height = if high {
                heights.reduce(f64::max)?
            } else {
                heights.reduce(f64::min)?
            };
            framed(z, up, [0, 1, 2].map(|k| position[k] + height * z[k]))
        }
        (OriginRule::Centre, SurfaceKind::Sphere) => framed(unit(direction)?, up, position),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
