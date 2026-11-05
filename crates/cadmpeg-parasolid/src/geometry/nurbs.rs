// SPDX-License-Identifier: Apache-2.0
//! Neutral geometry of Parasolid B-spline carriers.
//!
//! [`b_curve`] and [`b_surface`] map the fields of a `NURBS_CURVE` or
//! `NURBS_SURF` node, with the control values, knot multiplicities, and
//! distinct knots of the nodes it names, to IR NURBS geometry. Control points
//! convert from metres to millimetres; knots and parameter-space poles keep
//! their stored values. The `sense` of the wrapping `B_SURFACE` stays with
//! the host.

use cadmpeg_ir::geometry::nurbs::{
    NurbsCurve, NurbsError, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
};
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::Magnification;

const MILLIMETRES_PER_METRE: f64 = Magnification::MILLIMETERS_PER_METER.get();

/// The knot vector of one parameter direction: its declared distinct-knot
/// count and the multiplicity and distinct-knot values of the nodes it names.
#[derive(Debug, Clone, Copy)]
pub struct Knots<'a> {
    /// The declared distinct-knot count.
    pub count: usize,
    /// The knot multiplicities.
    pub multiplicities: &'a [u16],
    /// The distinct knots.
    pub values: &'a [f64],
}

/// The fields of a `NURBS_CURVE` node and the arrays it names.
#[derive(Debug, Clone, Copy)]
pub struct CurveFields<'a> {
    /// The degree.
    pub degree: u32,
    /// The number of control points.
    pub vertex_count: usize,
    /// The values per control point.
    pub vertex_dim: usize,
    /// Whether the control points carry weights.
    pub rational: bool,
    /// Whether the curve is periodic.
    pub periodic: bool,
    /// The control values, homogeneous when rational, in metres for a
    /// model-space curve.
    pub vertices: &'a [f64],
    /// The knot vector.
    pub knots: Knots<'a>,
}

/// The fields of a `NURBS_SURF` node and the arrays it names.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceFields<'a> {
    /// The `u` and `v` degrees.
    pub degrees: [u32; 2],
    /// The `u` and `v` control-point counts.
    pub vertex_counts: [usize; 2],
    /// The values per control point.
    pub vertex_dim: usize,
    /// Whether the control points carry weights.
    pub rational: bool,
    /// Whether the surface is periodic in `u` and in `v`.
    pub periodic: [bool; 2],
    /// The control values in `u`-major order, homogeneous when rational, in
    /// metres.
    pub vertices: &'a [f64],
    /// The `u` and `v` knot vectors.
    pub knots: [Knots<'a>; 2],
}

/// A B-curve: a model-space curve, or a parameter-space curve of dimension 2.
#[derive(Debug, Clone, PartialEq)]
pub enum BCurve {
    /// A curve in model space, in millimetres.
    Spatial(NurbsCurve),
    /// A curve in the parameter space of a support surface.
    Planar(PcurveNurbs),
}

/// Why a B-spline carrier has no neutral geometry.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// The fields contradict each other: a dimension that disagrees with the
    /// rational flag, a control or knot count that disagrees with its array,
    /// a zero multiplicity, or a weight that is not positive.
    Fields,
    /// The IR refuses the assembled lanes.
    Lanes(NurbsError),
}

/// The neutral geometry of a `NURBS_CURVE`: a model-space curve when its
/// control points are `x y z` or `x·w y·w z·w w`, a parameter-space curve
/// when they are `u v` or `u·w v·w w`. A vertex dimension of 4 is
/// homogeneous whatever the rational flag states.
pub fn b_curve(fields: &CurveFields<'_>) -> Result<BCurve, Refusal> {
    let expected = knot_total(fields.vertex_count, fields.degree)?;
    let knots = expand(&fields.knots, expected)?;
    match (fields.vertex_dim, fields.rational) {
        (3, false) | (4, _) => {
            let rational = fields.vertex_dim == 4;
            let (points, weights) = spatial_poles(fields.vertices, fields.vertex_count, rational)?;
            NurbsCurve::from_lanes(fields.degree, knots, points, weights, fields.periodic)
                .map(BCurve::Spatial)
                .map_err(Refusal::Lanes)
        }
        (2, false) | (3, true) => {
            let (points, weights) =
                planar_poles(fields.vertices, fields.vertex_count, fields.rational)?;
            PcurveNurbs::from_lanes(fields.degree, knots, points, weights, fields.periodic)
                .map(BCurve::Planar)
                .map_err(Refusal::Lanes)
        }
        _ => Err(Refusal::Fields),
    }
}

/// The neutral geometry of a `NURBS_SURF`, with its normal reversed when the
/// host's `B_SURFACE` sense reverses it.
pub fn b_surface(
    fields: &SurfaceFields<'_>,
    normal_reversed: bool,
) -> Result<NurbsSurface, Refusal> {
    matches!((fields.vertex_dim, fields.rational), (3, false) | (4, true))
        .then_some(())
        .ok_or(Refusal::Fields)?;
    let [u_count, v_count] = fields.vertex_counts;
    let count = u_count.checked_mul(v_count).ok_or(Refusal::Fields)?;
    let u_knots = expand(&fields.knots[0], knot_total(u_count, fields.degrees[0])?)?;
    let v_knots = expand(&fields.knots[1], knot_total(v_count, fields.degrees[1])?)?;
    let (points, weights) = spatial_poles(fields.vertices, count, fields.rational)?;
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(fields.degrees[0], u_knots, fields.periodic[0]),
        NurbsSurfaceAxis::new(fields.degrees[1], v_knots, fields.periodic[1]),
        NurbsSurfaceLanes::new(
            rows(&points, v_count),
            weights.map(|weights| rows(&weights, v_count)),
        ),
        normal_reversed,
    )
    .map_err(Refusal::Lanes)
}

/// `values` in rows of `width`.
fn rows<T: Clone>(values: &[T], width: usize) -> Vec<Vec<T>> {
    values.chunks(width.max(1)).map(<[T]>::to_vec).collect()
}

/// The full knot count of `vertex_count` control points of `degree`.
fn knot_total(vertex_count: usize, degree: u32) -> Result<usize, Refusal> {
    vertex_count
        .checked_add(usize::try_from(degree).map_err(|_| Refusal::Fields)?)
        .and_then(|total| total.checked_add(1))
        .ok_or(Refusal::Fields)
}

/// The full knot vector of `knots`: each of the declared distinct knots
/// repeated by its positive multiplicity. Entries past the declared count
/// carry zero multiplicity.
fn expand(knots: &Knots<'_>, expected: usize) -> Result<Vec<f64>, Refusal> {
    let multiplicities = knots
        .multiplicities
        .get(..knots.count)
        .ok_or(Refusal::Fields)?;
    let values = knots.values.get(..knots.count).ok_or(Refusal::Fields)?;
    (knots.count > 0
        && multiplicities.iter().all(|multiplicity| *multiplicity > 0)
        && knots.multiplicities[knots.count..]
            .iter()
            .all(|multiplicity| *multiplicity == 0))
    .then_some(())
    .ok_or(Refusal::Fields)?;
    let mut expanded = Vec::new();
    for (value, multiplicity) in values.iter().zip(multiplicities) {
        let multiplicity = usize::from(*multiplicity);
        if expanded.len() + multiplicity > expected {
            return Err(Refusal::Fields);
        }
        expanded.extend(std::iter::repeat_n(*value, multiplicity));
    }
    (expanded.len() == expected)
        .then_some(expanded)
        .ok_or(Refusal::Fields)
}

/// The positive weight of a homogeneous control point.
fn weight(pole: &[f64], rational: bool) -> Result<f64, Refusal> {
    let weight = if rational {
        pole.last().copied().ok_or(Refusal::Fields)?
    } else {
        1.0
    };
    (weight > 0.0).then_some(weight).ok_or(Refusal::Fields)
}

type Lanes<P> = (Vec<P>, Option<Vec<f64>>);

fn spatial_poles(vertices: &[f64], count: usize, rational: bool) -> Result<Lanes<Point3>, Refusal> {
    let dim = if rational { 4 } else { 3 };
    (Some(vertices.len()) == count.checked_mul(dim))
        .then_some(())
        .ok_or(Refusal::Fields)?;
    let mut points = Vec::with_capacity(count);
    let mut weights = rational.then(|| Vec::with_capacity(count));
    for pole in vertices.chunks_exact(dim) {
        let weight = weight(pole, rational)?;
        points.push(Point3::new(
            pole[0] / weight * MILLIMETRES_PER_METRE,
            pole[1] / weight * MILLIMETRES_PER_METRE,
            pole[2] / weight * MILLIMETRES_PER_METRE,
        ));
        if let Some(weights) = &mut weights {
            weights.push(weight);
        }
    }
    Ok((points, weights))
}

fn planar_poles(vertices: &[f64], count: usize, rational: bool) -> Result<Lanes<Point2>, Refusal> {
    let dim = if rational { 3 } else { 2 };
    (Some(vertices.len()) == count.checked_mul(dim))
        .then_some(())
        .ok_or(Refusal::Fields)?;
    let mut points = Vec::with_capacity(count);
    let mut weights = rational.then(|| Vec::with_capacity(count));
    for pole in vertices.chunks_exact(dim) {
        let weight = weight(pole, rational)?;
        points.push(Point2::new(pole[0] / weight, pole[1] / weight));
        if let Some(weights) = &mut weights {
            weights.push(weight);
        }
    }
    Ok((points, weights))
}

#[cfg(test)]
mod tests;
