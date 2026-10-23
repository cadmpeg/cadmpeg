// SPDX-License-Identifier: Apache-2.0
//! Neutral carriers for native surfaces, edge curves, and pcurves.
//!
//! The neutral model stores lengths in millimetres and uses its own analytic
//! parameterisations. A native analytic surface maps to a neutral carrier
//! together with the affine map `M` that carries a native `(u, v)` to the
//! neutral `(u', v')` of the same point; a native pcurve on that surface maps
//! through `M` exactly, because every pcurve type is closed under affine
//! maps. The surface-type sections below derive `M` from §8.4 to §8.8.

use std::f64::consts::{PI, TAU};

use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, DegenerateCurve, EllipseCurve, LineCurve,
    PlaneSurface, SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::nurbs::{
    NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
};
use cadmpeg_ir::geometry::pcurve::{HarmonicPcurve, LinePcurve, PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use super::eval::{cross, dot, norm, scale, sub};
use super::model::{BsplineCurve, BsplineSurface, CurveGeometry, SurfaceGeometry, Vec2, Vec3};

/// Neutral length unit per metre (§8.21).
pub(crate) const MILLIMETRES_PER_METRE: f64 = 1000.0;
/// Relative tolerance for the perpendicular and equal-length conditions of
/// an analytic frame.
const EPS_FRAME_RELATIVE: f64 = 1.0e-5;

/// Why a native record has no neutral carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CarrierRefusal {
    /// The record type has no neutral form here.
    Untyped,
    /// The vectors violate the frame conditions of the record type.
    Frame(&'static str),
    /// The neutral constructor refused the values.
    Admission(&'static str),
}

/// Affine map `x' = L·x + o` between two parameter domains.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UvMap {
    /// Linear part, row-major.
    pub(crate) linear: [[f64; 2]; 2],
    /// Translation.
    pub(crate) offset: Vec2,
}

impl UvMap {
    /// The identity map.
    pub(crate) const IDENTITY: Self = Self {
        linear: [[1.0, 0.0], [0.0, 1.0]],
        offset: [0.0, 0.0],
    };

    const fn diagonal(u: f64, v: f64, offset: Vec2) -> Self {
        Self {
            linear: [[u, 0.0], [0.0, v]],
            offset,
        }
    }

    /// Image of a vector.
    pub(crate) fn vector(&self, x: Vec2) -> Vec2 {
        [
            self.linear[0][0] * x[0] + self.linear[0][1] * x[1],
            self.linear[1][0] * x[0] + self.linear[1][1] * x[1],
        ]
    }

    /// Image of a point.
    pub(crate) fn point(&self, x: Vec2) -> Vec2 {
        let [u, v] = self.vector(x);
        [u + self.offset[0], v + self.offset[1]]
    }
}

/// A neutral surface carrier and the map from the native domain into its
/// domain.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceChart {
    /// Neutral surface carrier, in millimetres.
    pub(crate) geometry: SolvedSurfaceGeometry,
    /// Native `(u, v)` to neutral `(u', v')`.
    pub(crate) map: UvMap,
    /// Neutral periods of `u'` and `v'`.
    pub(crate) periods: [Option<f64>; 2],
}

fn point3(value: Vec3) -> Point3 {
    let [x, y, z] = scale(value, MILLIMETRES_PER_METRE);
    Point3::new(x, y, z)
}

fn vector3(value: Vec3) -> Vector3 {
    Vector3::new(value[0], value[1], value[2])
}

fn unit(value: Vec3) -> Result<Vec3, CarrierRefusal> {
    let length = norm(value);
    if length > 0.0 && length.is_finite() {
        Ok(scale(value, length.recip()))
    } else {
        Err(CarrierRefusal::Frame("a frame vector has zero length"))
    }
}

/// `value` without its component along the unit vector `axis`, normalised.
fn unit_across(value: Vec3, axis: Vec3) -> Result<Vec3, CarrierRefusal> {
    unit(sub(value, scale(axis, dot(value, axis))))
}

/// Whether `a` and `b` are perpendicular within the frame tolerance.
fn perpendicular(a: Vec3, b: Vec3) -> bool {
    dot(a, b).abs() <= EPS_FRAME_RELATIVE * norm(a) * norm(b)
}

/// Whether `a` and `b` have equal lengths within the frame tolerance.
fn equal_length(a: Vec3, b: Vec3) -> bool {
    (norm(a) - norm(b)).abs() <= EPS_FRAME_RELATIVE * norm(a).max(norm(b))
}

/// Whether `a` equals `b` within the frame tolerance relative to `size`.
fn coincident(a: Vec3, b: Vec3, size: f64) -> bool {
    norm(sub(a, b)) <= EPS_FRAME_RELATIVE * size
}

fn require(condition: bool, refusal: &'static str) -> Result<(), CarrierRefusal> {
    if condition {
        Ok(())
    } else {
        Err(CarrierRefusal::Frame(refusal))
    }
}

fn sign(value: f64) -> f64 {
    if value < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// The neutral chart of an analytic surface (§8.4 to §8.8).
pub(crate) fn analytic_chart(surface: &SurfaceGeometry) -> Result<SurfaceChart, CarrierRefusal> {
    match surface {
        SurfaceGeometry::Plane {
            origin,
            u_axis,
            v_axis,
            normal,
        } => plane_chart(*origin, *u_axis, *v_axis, *normal),
        SurfaceGeometry::Cylinder { origin, x, y, axis } => {
            require(
                perpendicular(*x, *y) && perpendicular(*x, *axis) && perpendicular(*y, *axis),
                "cylinder vectors are not perpendicular",
            )?;
            require(equal_length(*x, *y), "cylinder X and Y differ in length")?;
            let axis_unit = unit(*axis)?;
            let reference = unit_across(*x, axis_unit)?;
            let turn = sign(dot(*y, cross(axis_unit, reference)));
            let geometry = CylinderSurface::try_new(
                point3(*origin),
                vector3(axis_unit),
                vector3(reference),
                norm(*x) * MILLIMETRES_PER_METRE,
            )
            .map_err(CarrierRefusal::Admission)?;
            Ok(SurfaceChart {
                geometry: SolvedSurfaceGeometry::Cylinder(geometry),
                map: UvMap::diagonal(turn, norm(*axis) * MILLIMETRES_PER_METRE, [0.0, 0.0]),
                periods: [Some(TAU), None],
            })
        }
        SurfaceGeometry::Cone {
            origin,
            x,
            y,
            axis,
            a,
            b,
        } => cone_chart(*origin, *x, *y, *axis, [*a, *b]),
        SurfaceGeometry::Sphere { center, x, y, z } => {
            require(
                perpendicular(*x, *y) && perpendicular(*x, *z) && perpendicular(*y, *z),
                "sphere vectors are not perpendicular",
            )?;
            require(
                equal_length(*x, *y) && equal_length(*x, *z),
                "sphere vectors differ in length",
            )?;
            let axis = unit(*z)?;
            let reference = unit_across(*x, axis)?;
            let turn = sign(dot(*y, cross(axis, reference)));
            let geometry = SphereSurface::try_new(
                point3(*center),
                vector3(axis),
                vector3(reference),
                norm(*x) * MILLIMETRES_PER_METRE,
            )
            .map_err(CarrierRefusal::Admission)?;
            Ok(SurfaceChart {
                geometry: SolvedSurfaceGeometry::Sphere(geometry),
                map: UvMap::diagonal(turn, 1.0, [0.0, 0.0]),
                periods: [Some(TAU), None],
            })
        }
        SurfaceGeometry::Torus(torus) => torus_chart(torus),
        SurfaceGeometry::Bspline(_) | SurfaceGeometry::Undefined { .. } => {
            Err(CarrierRefusal::Untyped)
        }
    }
}

/// `O + u·U + v·V` maps to the neutral plane through `O` with normal `N`
/// and u-axis along `U`; `M` is the projection of `U` and `V` onto that frame.
fn plane_chart(
    origin: Vec3,
    u_axis: Vec3,
    v_axis: Vec3,
    normal: Vec3,
) -> Result<SurfaceChart, CarrierRefusal> {
    require(
        perpendicular(u_axis, normal) && perpendicular(v_axis, normal),
        "plane U or V is not perpendicular to N",
    )?;
    let normal = unit(normal)?;
    let reference = unit_across(u_axis, normal)?;
    let across = cross(normal, reference);
    let geometry = PlaneSurface::try_new(point3(origin), vector3(normal), vector3(reference))
        .map_err(CarrierRefusal::Admission)?;
    let mm = MILLIMETRES_PER_METRE;
    Ok(SurfaceChart {
        geometry: SolvedSurfaceGeometry::Plane(geometry),
        map: UvMap {
            linear: [
                [mm * dot(u_axis, reference), mm * dot(v_axis, reference)],
                [mm * dot(u_axis, across), mm * dot(v_axis, across)],
            ],
            offset: [0.0, 0.0],
        },
        periods: [None, None],
    })
}

/// `O + v·A + (a + b·v)·(cos(u)·X + sin(u)·Y)`. The neutral axial coordinate
/// is `v·|A|`, the neutral radius at the origin is `|a·X|`, and the radial
/// slope is `±b·|X| / |A|`. A negative `a` turns the reference direction to
/// `−X`, which keeps the native angle.
fn cone_chart(
    origin: Vec3,
    x: Vec3,
    y: Vec3,
    axis: Vec3,
    [a, b]: [f64; 2],
) -> Result<SurfaceChart, CarrierRefusal> {
    require(
        perpendicular(x, y) && perpendicular(x, axis) && perpendicular(y, axis),
        "cone vectors are not perpendicular",
    )?;
    require(equal_length(x, y), "cone X and Y differ in length")?;
    let axis_length = norm(axis);
    let axis_unit = unit(axis)?;
    let x_unit = unit_across(x, axis_unit)?;
    let turn = sign(dot(y, cross(axis_unit, x_unit)));
    let side = sign(a);
    let radius = norm(x);
    let slope = side * b * radius / axis_length;
    let geometry = ConeSurface::try_new(
        point3(origin),
        vector3(axis_unit),
        vector3(scale(x_unit, side)),
        a.abs() * radius * MILLIMETRES_PER_METRE,
        1.0,
        slope.atan(),
    )
    .map_err(CarrierRefusal::Admission)?;
    Ok(SurfaceChart {
        geometry: SolvedSurfaceGeometry::Cone(geometry),
        map: UvMap::diagonal(turn, axis_length * MILLIMETRES_PER_METRE, [0.0, 0.0]),
        periods: [Some(TAU), None],
    })
}

/// The torus of §8.8 is a neutral torus when `m_c = ε·r·M̂_c`,
/// `m_s = −ε·s·(â × M̂_c)`, and `m_a = η·r·â`, with `s` the turn of `M_s`
/// about the axis. Then `u' = s·u` and `(cos v', sin v') = (ε·cos v,
/// η·sin v)`.
fn torus_chart(torus: &super::model::Torus) -> Result<SurfaceChart, CarrierRefusal> {
    let major = norm(torus.major_cos);
    let minor = norm(torus.minor_cos);
    require(
        perpendicular(torus.major_cos, torus.major_sin)
            && perpendicular(torus.major_cos, torus.axis)
            && equal_length(torus.major_cos, torus.major_sin),
        "torus major vectors are not a circle frame",
    )?;
    let axis = unit(torus.axis)?;
    let reference = unit_across(torus.major_cos, axis)?;
    let across = cross(axis, reference);
    let turn = sign(dot(torus.major_sin, across));
    let side = sign(dot(torus.minor_cos, reference));
    let lift = sign(dot(torus.minor_axial, axis));
    require(
        coincident(torus.minor_cos, scale(reference, side * minor), minor)
            && coincident(torus.minor_axial, scale(axis, lift * minor), minor)
            && coincident(torus.minor_sin, scale(across, -side * turn), 1.0),
        "torus minor vectors are not a tube frame",
    )?;
    let geometry = TorusSurface::try_new(
        point3(torus.origin),
        vector3(axis),
        vector3(reference),
        major * MILLIMETRES_PER_METRE,
        minor * MILLIMETRES_PER_METRE,
    )
    .map_err(CarrierRefusal::Admission)?;
    let v_offset = if side < 0.0 { PI } else { 0.0 };
    Ok(SurfaceChart {
        geometry: SolvedSurfaceGeometry::Torus(geometry),
        map: UvMap::diagonal(turn, side * lift, [0.0, v_offset]),
        periods: [Some(TAU), Some(TAU)],
    })
}

/// The neutral NURBS carrier of a B-spline surface; its domain is the
/// native domain.
pub(crate) fn bspline_chart(surface: &BsplineSurface) -> Result<SurfaceChart, CarrierRefusal> {
    let mut rows = Vec::new();
    let mut weights = Vec::new();
    for row in surface.poles.chunks(surface.v_count.max(1)) {
        let mut points = Vec::new();
        let mut row_weights = Vec::new();
        for [wx, wy, wz, w] in row {
            points.push(point3([wx / w, wy / w, wz / w]));
            row_weights.push(*w);
        }
        rows.push(points);
        weights.push(row_weights);
    }
    let grid = NurbsPoleGrid::from_lanes(rows, surface.rational.then_some(weights))
        .map_err(|_| CarrierRefusal::Admission("B-spline surface weights"))?;
    let degree = |order: usize| {
        order
            .checked_sub(1)
            .and_then(|degree| u32::try_from(degree).ok())
            .ok_or(CarrierRefusal::Admission("B-spline surface order"))
    };
    let geometry = NurbsSurface::new(
        NurbsSurfaceAxis::new(
            degree(surface.u_order)?,
            surface.u_knots.clone(),
            surface.periodic[0],
        ),
        NurbsSurfaceAxis::new(
            degree(surface.v_order)?,
            surface.v_knots.clone(),
            surface.periodic[1],
        ),
        grid,
        false,
    )
    .map_err(|_| CarrierRefusal::Admission("B-spline surface structure"))?;
    Ok(SurfaceChart {
        geometry: SolvedSurfaceGeometry::Nurbs(geometry),
        map: UvMap::IDENTITY,
        periods: [None, None],
    })
}

/// A neutral 3D curve carrier and the affine map `t' = scale·t + shift` from
/// the native curve parameter to the carrier parameter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurveChart {
    /// Neutral curve carrier, in millimetres.
    pub(crate) geometry: SolvedCurveGeometry,
    /// `scale` and `shift` of the parameter map.
    pub(crate) parameter: [f64; 2],
}

impl CurveChart {
    /// Carrier parameter of native parameter `t`.
    pub(crate) fn map(&self, t: f64) -> f64 {
        self.parameter[0] * t + self.parameter[1]
    }

    /// Carrier interval of the native interval `[low, high]`. A circle or
    /// ellipse interval moves by whole turns so that it starts in
    /// `[0, 2π)`.
    pub(crate) fn interval(&self, [low, high]: Vec2) -> Vec2 {
        let [start, end] = [self.map(low), self.map(high)];
        match self.geometry {
            SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
                let turns = (start / TAU).floor() * TAU;
                [start - turns, end - turns]
            }
            _ => [start, end],
        }
    }
}

/// The neutral carrier of an edge curve (§8.15).
pub(crate) fn edge_curve(curve: &CurveGeometry<3>) -> Result<CurveChart, CarrierRefusal> {
    let chart = |geometry, parameter| CurveChart {
        geometry,
        parameter,
    };
    match curve {
        CurveGeometry::Line { origin, direction } => {
            let length = norm(*direction);
            let line = LineCurve::try_new(point3(*origin), vector3(unit(*direction)?))
                .map_err(CarrierRefusal::Admission)?;
            Ok(chart(
                SolvedCurveGeometry::Line(line),
                [length * MILLIMETRES_PER_METRE, 0.0],
            ))
        }
        CurveGeometry::Circle(conic) => {
            require(
                perpendicular(conic.x, conic.y) && equal_length(conic.x, conic.y),
                "circle X and Y are not a circle frame",
            )?;
            let normal = unit(cross(conic.x, conic.y))?;
            let circle = CircleCurve::try_new(
                point3(conic.center),
                vector3(normal),
                vector3(unit_across(conic.x, normal)?),
                norm(conic.x) * MILLIMETRES_PER_METRE,
            )
            .map_err(CarrierRefusal::Admission)?;
            Ok(chart(SolvedCurveGeometry::Circle(circle), [1.0, 0.0]))
        }
        CurveGeometry::Ellipse(conic) => {
            require(
                perpendicular(conic.x, conic.y),
                "ellipse X and Y are not perpendicular",
            )?;
            let normal = unit(cross(conic.x, conic.y))?;
            let (major, minor, shift) = if norm(conic.x) >= norm(conic.y) {
                (conic.x, conic.y, 0.0)
            } else {
                (conic.y, conic.x, -PI / 2.0)
            };
            let ellipse = EllipseCurve::try_new(
                point3(conic.center),
                vector3(normal),
                vector3(unit_across(major, normal)?),
                norm(major) * MILLIMETRES_PER_METRE,
                norm(minor) * MILLIMETRES_PER_METRE,
            )
            .map_err(CarrierRefusal::Admission)?;
            Ok(chart(SolvedCurveGeometry::Ellipse(ellipse), [1.0, shift]))
        }
        CurveGeometry::Point(point) => {
            let degenerate =
                DegenerateCurve::try_new(point3(*point)).map_err(CarrierRefusal::Admission)?;
            Ok(chart(
                SolvedCurveGeometry::Degenerate(degenerate),
                [1.0, 0.0],
            ))
        }
        CurveGeometry::Bspline(spline) => {
            let nurbs = nurbs_curve(spline)?;
            Ok(chart(SolvedCurveGeometry::Nurbs(nurbs), [1.0, 0.0]))
        }
        CurveGeometry::Polyline(points) => {
            let poles = points.iter().map(|point| point3(*point)).collect();
            let nurbs = NurbsCurve::new(
                1,
                polyline_knots(points.len())?,
                NurbsPoles3::from_lanes(poles, None)
                    .map_err(|_| CarrierRefusal::Admission("polyline points"))?,
                false,
            )
            .map_err(|_| CarrierRefusal::Admission("polyline structure"))?;
            Ok(chart(SolvedCurveGeometry::Nurbs(nurbs), [1.0, 0.0]))
        }
        CurveGeometry::Composite(_) | CurveGeometry::Undefined { .. } => {
            Err(CarrierRefusal::Untyped)
        }
    }
}

/// Degree-1 knots `0, 0, 1, …, n − 1, n − 1` that parameterise a polyline
/// of `count` points by point index.
fn polyline_knots(count: usize) -> Result<Vec<f64>, CarrierRefusal> {
    let last = count
        .checked_sub(1)
        .filter(|last| *last > 0)
        .ok_or(CarrierRefusal::Frame(
            "a polyline has fewer than two points",
        ))?;
    let mut knots = Vec::new();
    knots.push(0.0);
    for index in 0..=last {
        knots.push(
            cadmpeg_core::convert::f64_from_index(index)
                .ok_or(CarrierRefusal::Admission("polyline knot"))?,
        );
    }
    knots.push(*knots.last().unwrap_or(&0.0));
    Ok(knots)
}

fn nurbs_curve(spline: &BsplineCurve<3>) -> Result<NurbsCurve, CarrierRefusal> {
    let mut points = Vec::new();
    for (weighted, weight) in spline.weighted.iter().zip(&spline.weights) {
        points.push(point3(scale(*weighted, weight.recip())));
    }
    let poles = NurbsPoles3::from_lanes(points, spline.rational.then(|| spline.weights.clone()))
        .map_err(|_| CarrierRefusal::Admission("B-spline curve weights"))?;
    let degree = spline
        .order
        .checked_sub(1)
        .and_then(|degree| u32::try_from(degree).ok())
        .ok_or(CarrierRefusal::Admission("B-spline curve order"))?;
    NurbsCurve::new(degree, spline.knots.clone(), poles, false)
        .map_err(|_| CarrierRefusal::Admission("B-spline curve structure"))
}

/// The neutral pcurve of a native pcurve whose domain maps into the
/// neutral surface domain through `map`. The pcurve parameter is unchanged.
pub(crate) fn pcurve(
    curve: &CurveGeometry<2>,
    map: &UvMap,
) -> Result<PcurveGeometry, CarrierRefusal> {
    let point2 = |value: Vec2| {
        let [u, v] = map.point(value);
        Point2::new(u, v)
    };
    let vector2 = |value: Vec2| {
        let [u, v] = map.vector(value);
        Point2::new(u, v)
    };
    match curve {
        CurveGeometry::Line { origin, direction } => {
            LinePcurve::try_new(point2(*origin), vector2(*direction))
                .map(PcurveGeometry::Line)
                .map_err(CarrierRefusal::Admission)
        }
        CurveGeometry::Ellipse(conic) | CurveGeometry::Circle(conic) => {
            HarmonicPcurve::try_new(point2(conic.center), vector2(conic.x), vector2(conic.y))
                .map(PcurveGeometry::Harmonic)
                .map_err(CarrierRefusal::Admission)
        }
        CurveGeometry::Bspline(spline) => {
            let points = spline
                .weighted
                .iter()
                .zip(&spline.weights)
                .map(|(weighted, weight)| point2(scale(*weighted, weight.recip())))
                .collect::<Vec<_>>();
            let degree = spline
                .order
                .checked_sub(1)
                .and_then(|degree| u32::try_from(degree).ok())
                .ok_or(CarrierRefusal::Admission("B-spline pcurve order"))?;
            PcurveNurbs::from_lanes(
                degree,
                spline.knots.clone(),
                points,
                spline.rational.then(|| spline.weights.clone()),
                false,
            )
            .map(|nurbs| PcurveGeometry::Nurbs { nurbs })
            .map_err(|_| CarrierRefusal::Admission("B-spline pcurve structure"))
        }
        CurveGeometry::Polyline(points) => PcurveNurbs::from_lanes(
            1,
            polyline_knots(points.len())?,
            points.iter().map(|point| point2(*point)).collect(),
            None,
            false,
        )
        .map(|nurbs| PcurveGeometry::Nurbs { nurbs })
        .map_err(|_| CarrierRefusal::Admission("polyline pcurve structure")),
        CurveGeometry::Point(_) | CurveGeometry::Composite(_) | CurveGeometry::Undefined { .. } => {
            Err(CarrierRefusal::Untyped)
        }
    }
}

#[cfg(test)]
mod tests;
