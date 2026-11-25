// SPDX-License-Identifier: Apache-2.0
//! Lower bounds, in the definition's frame, on how far an edge's curve lies
//! from a face's surface or from a point, and the lengths of line edges (§10:
//! in-IR arithmetic).
//!
//! The carriers read are planes,
//! cylinders, cones, spheres, tori, and NURBS surfaces whose weights share
//! one sign; line edges between two vertices, circles, and NURBS curves whose
//! weights share one sign; each placed by rigid maps, its body's and those of
//! each `Transformed` carrier it nests in. Each bound holds for the whole
//! carrier, so it holds for the face or edge on it. A NURBS curve or surface
//! whose weights share one sign lies in the convex hull of its poles, and so
//! in their box. An elliptic cone lies between the circular cones of its
//! minor and major sections.

use crate::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use crate::math::portable::Portable;
use crate::math::{Point3, Vector3};
use crate::scalar::NonZeroReal;
use crate::topology::EdgeCarrier;
use crate::transform::Transform;

use super::super::frames::{held_by, point, vector, Placement};
use super::super::matching::StateTopology;
use super::position;

/// `outer` after a carrier's own rigid placement `inner`.
fn within(outer: Placement, inner: &Transform) -> Option<Placement> {
    if !inner.is_proper_rigid() {
        return None;
    }
    Some(Some(match outer {
        None => *inner,
        Some(outer) => outer.compose(*inner).ok()?,
    }))
}

/// Whether weights share one sign, so that the carrier lies in the convex
/// hull of its poles. Unstated weights are all one.
fn convex(weights: Option<Vec<f64>>) -> bool {
    weights.is_none_or(|weights| {
        weights.iter().all(|&weight| weight > 0.0) || weights.iter().all(|&weight| weight < 0.0)
    })
}

/// An axis-aligned box.
#[derive(Clone, Copy)]
struct Boxed {
    min: Point3,
    max: Point3,
}

impl Boxed {
    fn of(points: &[Point3]) -> Option<Self> {
        let (first, rest) = points.split_first()?;
        Some(rest.iter().fold(
            Self {
                min: *first,
                max: *first,
            },
            |boxed, point| Self {
                min: Point3::new(
                    boxed.min.x.min(point.x),
                    boxed.min.y.min(point.y),
                    boxed.min.z.min(point.z),
                ),
                max: Point3::new(
                    boxed.max.x.max(point.x),
                    boxed.max.y.max(point.y),
                    boxed.max.z.max(point.z),
                ),
            },
        ))
    }

    /// The distance between two boxes.
    fn distance(self, other: Self) -> f64 {
        let gap = |low: f64, high: f64, other_low: f64, other_high: f64| {
            (other_low - high).max(low - other_high).max(0.0)
        };
        Vector3::new(
            gap(self.min.x, self.max.x, other.min.x, other.max.x),
            gap(self.min.y, self.max.y, other.min.y, other.max.y),
            gap(self.min.z, self.max.z, other.min.z, other.max.z),
        )
        .norm()
    }

    fn point_distance(self, point: Point3) -> f64 {
        self.distance(Self {
            min: point,
            max: point,
        })
    }
}

/// The distance from `point` to the circle of `center`, unit `axis`, and
/// `radius`.
fn circle_distance(center: Point3, axis: Vector3, radius: f64, point: Point3) -> f64 {
    let offset = point.vector_from(center);
    let height = offset.dot(axis);
    let across = offset.cross(axis).norm() - radius;
    height.portable_hypot(across)
}

/// The least and the greatest length of `start + t (end - start)` over
/// `t` in `[0, 1]`; the length is convex in `t`.
fn span(start: Vector3, end: Vector3) -> (f64, f64) {
    let along = end - start;
    let length = along.dot(along);
    let t = if length > 0.0 {
        (-start.dot(along) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (
        (start + along.scale(t)).norm(),
        start.norm().max(end.norm()),
    )
}

/// How far a set whose distances from a core lie in `[least, greatest]` is
/// from the points at `radius` from that core.
fn off_radius((least, greatest): (f64, f64), radius: f64) -> f64 {
    (least - radius).max(radius - greatest)
}

/// Where a face's surface lies.
enum Sheet {
    Plane {
        origin: Point3,
        normal: Vector3,
    },
    /// The points at `radius` from the axis line.
    Cylinder {
        origin: Point3,
        axis: Vector3,
        radius: f64,
    },
    /// The points at `scale` times `|radius + slope * h|` from the axis at
    /// height `h`, for some `scale` in `[inner, outer]`.
    Cone {
        origin: Point3,
        axis: Vector3,
        radius: f64,
        slope: f64,
        inner: f64,
        outer: f64,
    },
    /// The points at `radius` from the center.
    Sphere {
        center: Point3,
        radius: f64,
    },
    /// The points within `minor` of the core circle, all of them at `minor`
    /// where the torus is a ring.
    Torus {
        center: Point3,
        axis: Vector3,
        major: f64,
        minor: f64,
    },
    /// The box of a NURBS surface's poles.
    Hull(Boxed),
}

impl Sheet {
    fn of(topology: &StateTopology<'_>, face: &str) -> Option<Self> {
        let mut placement = held_by(topology.face_body(face))?;
        let surface = topology.faces.get(face)?.surface.as_str();
        let mut geometry = topology.surfaces.get(surface)?.geometry.solved()?;
        while let SolvedSurfaceGeometry::Transformed(placed) = geometry {
            placement = within(placement, placed.transform())?;
            geometry = placed.basis();
        }
        let point = |at| point(placement, at);
        let vector = |along| vector(placement, along);
        Some(match geometry {
            SolvedSurfaceGeometry::Plane(plane) => Self::Plane {
                origin: point(plane.origin().get())?,
                normal: vector(*plane.frame().axis().as_raw())?,
            },
            SolvedSurfaceGeometry::Cylinder(cylinder) => Self::Cylinder {
                origin: point(cylinder.origin().get())?,
                axis: vector(*cylinder.frame().axis().as_raw())?,
                radius: cylinder.radius().get(),
            },
            SolvedSurfaceGeometry::Cone(cone) => {
                let ratio = cone.ratio().get();
                Self::Cone {
                    origin: point(cone.origin().get())?,
                    axis: vector(*cone.frame().axis().as_raw())?,
                    radius: cone.radius().get(),
                    slope: cone.half_angle().get().portable_tan(),
                    inner: ratio.min(1.0),
                    outer: ratio.max(1.0),
                }
            }
            SolvedSurfaceGeometry::Sphere(sphere) => Self::Sphere {
                center: point(sphere.center().get())?,
                radius: sphere.radius().get().abs(),
            },
            SolvedSurfaceGeometry::Torus(torus) => Self::Torus {
                center: point(torus.center().get())?,
                axis: vector(*torus.frame().axis().as_raw())?,
                major: torus.major_radius().get(),
                minor: torus.minor_radius().get().abs(),
            },
            SolvedSurfaceGeometry::Nurbs(nurbs) => {
                let weights = nurbs
                    .pole_weights()
                    .map(|weights| weights.into_iter().map(NonZeroReal::get).collect());
                if !convex(weights) {
                    return None;
                }
                let poles = nurbs
                    .poles()
                    .into_iter()
                    .map(|pole| point(pole.get()))
                    .collect::<Option<Vec<_>>>()?;
                Self::Hull(Boxed::of(&poles)?)
            }
            _ => return None,
        })
    }

    /// A lower bound on the distance from `point` to the surface.
    fn distance(&self, point: Point3) -> f64 {
        match *self {
            Self::Plane { origin, normal } => point.vector_from(origin).dot(normal).abs(),
            Self::Cylinder {
                origin,
                axis,
                radius,
            } => (point.vector_from(origin).cross(axis).norm() - radius).abs(),
            Self::Cone {
                origin,
                axis,
                radius,
                slope,
                inner,
                outer,
            } => {
                let offset = point.vector_from(origin);
                let across = offset.cross(axis).norm();
                let section = (radius + slope * offset.dot(axis)).abs();
                // The distance, in the point's meridian plane, to the two
                // generator lines of the circular cone of `scale`.
                let to = |scale: f64| {
                    (scale * section - across).abs() / (scale * slope).portable_hypot(1.0)
                };
                if across > outer * section {
                    to(outer)
                } else if across < inner * section {
                    to(inner)
                } else {
                    0.0
                }
            }
            Self::Sphere { center, radius } => (point.distance(center) - radius).abs(),
            Self::Torus {
                center,
                axis,
                major,
                minor,
            } => {
                let core = circle_distance(center, axis, major, point);
                if major > minor {
                    (core - minor).abs()
                } else {
                    (core - minor).max(0.0)
                }
            }
            Self::Hull(boxed) => boxed.point_distance(point),
        }
    }

    /// Whether `reach` lies farther than `tolerance` from the surface.
    /// `None` where no bound reads it.
    fn apart(&self, reach: &Reach, tolerance: f64) -> Option<bool> {
        if let Self::Plane { origin, normal } = *self {
            let height = |point: Point3| point.vector_from(origin).dot(normal);
            let one_side = |heights: &[f64]| {
                (heights.iter().all(|&height| height > 0.0)
                    || heights.iter().all(|&height| height < 0.0))
                    && heights.iter().all(|height| height.abs() > tolerance)
            };
            return match reach {
                Reach::Segment(start, end) => Some(one_side(&[height(*start), height(*end)])),
                Reach::Circle {
                    center,
                    axis,
                    radius,
                } => Some(height(*center).abs() - radius * normal.cross(*axis).norm() > tolerance),
                Reach::Hull(poles) => Some(one_side(
                    &poles.iter().copied().map(height).collect::<Vec<_>>(),
                )),
                Reach::Line { .. } => None,
            };
        }
        if let Self::Hull(boxed) = self {
            return Some(boxed.distance(reach.boxed()?) > tolerance);
        }
        if let Reach::Segment(start, end) = *reach {
            match *self {
                Self::Cylinder {
                    origin,
                    axis,
                    radius,
                } => {
                    let across = |point: Point3| {
                        let offset = point.vector_from(origin);
                        offset - axis.scale(offset.dot(axis))
                    };
                    let reach = span(across(start), across(end));
                    return Some(off_radius(reach, radius) > tolerance);
                }
                Self::Sphere { center, radius } => {
                    let reach = span(start.vector_from(center), end.vector_from(center));
                    return Some(off_radius(reach, radius) > tolerance);
                }
                _ => {}
            }
        }
        let (center, radius) = reach.ball()?;
        Some(self.distance(center) - radius > tolerance)
    }
}

/// Where an edge's curve lies.
enum Reach {
    /// A whole line carrier.
    Line { origin: Point3, direction: Vector3 },
    /// A line edge between its two ends.
    Segment(Point3, Point3),
    Circle {
        center: Point3,
        axis: Vector3,
        radius: f64,
    },
    /// The poles of a NURBS curve, whose hull holds it.
    Hull(Vec<Point3>),
}

impl Reach {
    fn of(topology: &StateTopology<'_>, edge: &str) -> Option<Self> {
        let bodies = topology.edge_bodies(edge);
        let body = held_by(
            bodies
                .iter()
                .filter_map(|body| topology.bodies.get(body).copied()),
        )?;
        let record = topology.edges.get(edge)?;
        let (EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _)) = &record.carrier else {
            return None;
        };
        let mut placement = body;
        let mut geometry = topology.curves.get(curve.as_str())?.geometry.solved()?;
        while let SolvedCurveGeometry::Transformed(placed) = geometry {
            placement = within(placement, placed.transform())?;
            geometry = placed.basis();
        }
        let carried = |at| point(placement, at);
        Some(match geometry {
            SolvedCurveGeometry::Line(line) => match &record.ends {
                // The ends are vertices of the body, placed by it alone.
                Some(ends) => Self::Segment(
                    point(body, position(topology, ends.start.as_str())?)?,
                    point(body, position(topology, ends.end.as_str())?)?,
                ),
                None => Self::Line {
                    origin: carried(line.origin().get())?,
                    direction: vector(placement, *line.direction().as_raw())?,
                },
            },
            SolvedCurveGeometry::Circle(circle) => Self::Circle {
                center: carried(circle.center().get())?,
                axis: vector(placement, *circle.frame().axis().as_raw())?,
                radius: circle.radius().get(),
            },
            SolvedCurveGeometry::Nurbs(nurbs) => {
                let weights = nurbs
                    .weights()
                    .map(|weights| weights.into_iter().map(NonZeroReal::get).collect());
                if !convex(weights) {
                    return None;
                }
                Self::Hull(
                    nurbs
                        .control_points()
                        .into_iter()
                        .map(|pole| carried(pole.get()))
                        .collect::<Option<Vec<_>>>()?,
                )
            }
            _ => return None,
        })
    }

    /// A ball that holds the reach: its center and radius.
    fn ball(&self) -> Option<(Point3, f64)> {
        match self {
            Self::Line { .. } => None,
            Self::Segment(start, end) => Some((
                start.translated(end.vector_from(*start), 0.5),
                start.distance(*end) / 2.0,
            )),
            Self::Circle { center, radius, .. } => Some((*center, *radius)),
            Self::Hull(poles) => {
                let boxed = Boxed::of(poles)?;
                let center = boxed.min.translated(boxed.max.vector_from(boxed.min), 0.5);
                Some((center, boxed.min.distance(boxed.max) / 2.0))
            }
        }
    }

    /// A box that holds the reach.
    fn boxed(&self) -> Option<Boxed> {
        match self {
            Self::Line { .. } => None,
            Self::Segment(start, end) => Boxed::of(&[*start, *end]),
            Self::Circle {
                center,
                axis,
                radius,
            } => {
                // Along each model axis the circle spans its radius times the
                // sine of that axis's angle to the circle's axis.
                let span = |along: f64| radius * (1.0 - along * along).max(0.0).sqrt();
                let half = Vector3::new(span(axis.x), span(axis.y), span(axis.z));
                Some(Boxed {
                    min: center.translated(half, -1.0),
                    max: center.translated(half, 1.0),
                })
            }
            Self::Hull(poles) => Boxed::of(poles),
        }
    }

    /// A lower bound on the distance from `point` to the curve, through the
    /// whole carrier of a line.
    fn distance(&self, point: Point3) -> Option<f64> {
        match self {
            Self::Line { origin, direction } => {
                Some(point.vector_from(*origin).cross(*direction).norm())
            }
            Self::Segment(start, end) => {
                let direction = end.vector_from(*start).unit()?;
                Some(point.vector_from(*start).cross(direction).norm())
            }
            Self::Circle {
                center,
                axis,
                radius,
            } => Some(circle_distance(*center, *axis, *radius, point)),
            Self::Hull(poles) => Some(Boxed::of(poles)?.point_distance(point)),
        }
    }
}

/// Whether `edge` lies farther than `tolerance` from the surface of `face`.
/// `None` where either carrier gives no bound.
pub(super) fn apart(
    topology: &StateTopology<'_>,
    face: &str,
    edge: &str,
    tolerance: f64,
) -> Option<bool> {
    Sheet::of(topology, face)?.apart(&Reach::of(topology, edge)?, tolerance)
}

/// A lower bound on the distance from `point` to the curve of `edge`,
/// through the whole carrier of a line. `None` where the carrier gives no
/// bound.
pub(super) fn curve_distance(
    topology: &StateTopology<'_>,
    edge: &str,
    point: Point3,
) -> Option<f64> {
    Reach::of(topology, edge)?.distance(point)
}

/// The position of `vertex` in the definition's frame, where the bodies
/// that hold it give it one rigid placement.
pub(super) fn placed_position(topology: &StateTopology<'_>, vertex: &str) -> Option<Point3> {
    let bodies = topology.vertex_bodies(vertex);
    let placement = held_by(
        bodies
            .iter()
            .filter_map(|body| topology.bodies.get(body).copied()),
    )?;
    point(placement, position(topology, vertex)?)
}

/// A lower bound on the distance from `point` to the surface of `face`.
/// `None` where no bound reads the surface.
pub(super) fn surface_distance(
    topology: &StateTopology<'_>,
    face: &str,
    point: Point3,
) -> Option<f64> {
    Some(Sheet::of(topology, face)?.distance(point))
}

/// The length of a line edge between two vertices.
pub(super) fn straight_length(topology: &StateTopology<'_>, edge: &str) -> Option<f64> {
    match Reach::of(topology, edge)? {
        Reach::Segment(start, end) => Some(start.distance(end)),
        Reach::Line { .. } | Reach::Circle { .. } | Reach::Hull(_) => None,
    }
}
