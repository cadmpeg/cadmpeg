// SPDX-License-Identifier: Apache-2.0
//! Traversal direction of a Type 144 boundary about its support normal.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::Sense;

use super::BoundaryItem;

/// Points sampled on each boundary curve to find the boundary's winding.
const BOUNDARY_WINDING_SAMPLES: u32 = 64;

/// A parameter-space image closes when, on each axis, its ends lie within
/// this fraction of the support's period on a periodic axis, or of the
/// image's extent on another axis. A boundary that crosses the seam of a
/// periodic support leaves a gap of one period, a hundred times this bound.
/// A parameter curve approximated within the model tolerance leaves a gap
/// far below it.
const PARAMETER_RING_CLOSURE_FRACTION: f64 = 1.0e-2;

/// A boundary whose twice signed area is at most this fraction of the
/// square of its extent encloses no area at the sampling resolution and has
/// no winding.
const BOUNDARY_AREA_DEGENERATE_FRACTION: f64 = 1.0e-9;

/// The traversal of a Type 144 boundary against the neutral loop direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoundaryWinding {
    /// The source order keeps the face on the neutral side.
    Kept,
    /// The source order runs backward.
    Reversed,
    /// The boundary has no winding; the reason names why.
    Unresolved(&'static str),
}

/// Twice the signed area of a closed sample polygon with the diagonal of its
/// bounding box.
#[derive(Debug, Clone, Copy)]
struct SignedArea {
    twice: f64,
    extent: f64,
}

impl SignedArea {
    /// Whether the area is too small against the extent to give a sign.
    fn degenerate(self) -> bool {
        !(self.twice.is_finite() && self.extent.is_finite())
            || self.twice.abs() <= BOUNDARY_AREA_DEGENERATE_FRACTION * self.extent * self.extent
    }
}

fn sample_parameters([start, end]: [f64; 2]) -> impl Iterator<Item = f64> {
    (0..=BOUNDARY_WINDING_SAMPLES).map(move |step| {
        start + (end - start) * f64::from(step) / f64::from(BOUNDARY_WINDING_SAMPLES)
    })
}

/// Twice the signed area of the closed polygon through `points`.
pub(super) fn polygon_signed_area_twice(points: &[[f64; 2]]) -> f64 {
    let Some(origin) = points.first().copied() else {
        return 0.0;
    };
    let mut area = 0.0;
    for (index, point) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        let (a, b) = (
            [point[0] - origin[0], point[1] - origin[1]],
            [next[0] - origin[0], next[1] - origin[1]],
        );
        area += a[0] * b[1] - a[1] * b[0];
    }
    area
}

/// The parameter period of each axis of `support`, from its canonical
/// parameterization: the azimuth of a cylinder, cone, sphere, or torus, the
/// minor azimuth of a torus, and the knot domain of a periodic NURBS axis.
fn support_parameter_periods(support: &SolvedSurfaceGeometry) -> [Option<f64>; 2] {
    const TAU: f64 = std::f64::consts::TAU;
    match support {
        SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_) => [Some(TAU), None],
        SolvedSurfaceGeometry::Torus(_) => [Some(TAU), Some(TAU)],
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            let period = |periodic: bool, degree: u32, knots: &[f64], count: usize| {
                periodic
                    .then(|| cadmpeg_ir::eval::nurbs_pcurve_parameter_domain(degree, knots, count))
                    .flatten()
                    .map(|domain| domain.upper() - domain.lower())
            };
            [
                period(
                    nurbs.u_periodic(),
                    nurbs.u_degree(),
                    nurbs.u_knots().as_slice(),
                    nurbs.u_count(),
                ),
                period(
                    nurbs.v_periodic(),
                    nurbs.v_degree(),
                    nurbs.v_knots().as_slice(),
                    nurbs.v_count(),
                ),
            ]
        }
        SolvedSurfaceGeometry::Transformed(placed) => support_parameter_periods(placed.basis()),
        SolvedSurfaceGeometry::Plane(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => [None, None],
    }
}

/// The signed area of a closed parameter-space sample polygon, or `None`
/// when its ends do not meet under [`PARAMETER_RING_CLOSURE_FRACTION`].
fn closed_parameter_area(points: &[[f64; 2]], periods: [Option<f64>; 2]) -> Option<SignedArea> {
    let (first, last) = (*points.first()?, *points.last()?);
    let mut low = first;
    let mut high = first;
    for point in points {
        for axis in 0..2 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    let extent = (high[0] - low[0]).hypot(high[1] - low[1]);
    if !extent.is_finite() {
        return None;
    }
    for axis in 0..2 {
        let reference = periods[axis].unwrap_or(extent);
        let gap = (last[axis] - first[axis]).abs();
        if !(gap.is_finite() && gap <= PARAMETER_RING_CLOSURE_FRACTION * reference) {
            return None;
        }
    }
    Some(SignedArea {
        twice: polygon_signed_area_twice(points),
        extent,
    })
}

/// The signed parameter-space area of a boundary traversed in source order:
/// positive when it runs counterclockwise in `(u, v)`. It is absent when an
/// item has no parameter curves, when a curve does not evaluate, or when the
/// image does not close.
fn parameter_boundary_signed_area(
    items: &[BoundaryItem],
    periods: [Option<f64>; 2],
    ctx: &DecodeContext<'_>,
) -> Result<Option<SignedArea>, CodecError> {
    let mut points = Vec::new();
    for item in items {
        if item.pcurves.is_empty() {
            return Ok(None);
        }
        let mut item_points = Vec::new();
        for (geometry, range) in &item.pcurves {
            for parameter in sample_parameters(*range) {
                let Some(point) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::pcurve_uv(ctx, geometry, parameter),
                )?)?
                else {
                    return Ok(None);
                };
                item_points.push([point.u, point.v]);
            }
        }
        if item.segment.sense == Sense::Reversed {
            item_points.reverse();
        }
        points.extend(item_points);
    }
    Ok(closed_parameter_area(&points, periods))
}

/// The signed model-space area of a boundary on a plane about the plane
/// normal: positive when it runs counterclockwise about that normal.
fn planar_model_boundary_signed_area(
    items: &[BoundaryItem],
    index: &ModelIndex<'_>,
    normal: Vector3,
    ctx: &DecodeContext<'_>,
) -> Result<Option<SignedArea>, CodecError> {
    let mut points = Vec::new();
    for item in items {
        let Some(curve) = index.curves(item.model_curve.as_str(), cadmpeg_ir::index::StandardIndex)
        else {
            return Ok(None);
        };
        let Some(range) = item.source_edge.param_range() else {
            return Ok(None);
        };
        let mut item_points = Vec::new();
        for parameter in sample_parameters(range.get()) {
            let Some(point) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::curve_point(ctx, &curve.geometry, parameter),
            )?)?
            else {
                return Ok(None);
            };
            item_points.push(point.get());
        }
        if item.segment.sense == Sense::Reversed {
            item_points.reverse();
        }
        points.extend(item_points);
    }
    let Some(origin) = points.first().copied() else {
        return Ok(None);
    };
    let (mut low, mut high) = (origin, origin);
    let mut twice = 0.0;
    for (position, point) in points.iter().enumerate() {
        let next = points[(position + 1) % points.len()];
        twice += point
            .vector_from(origin)
            .cross(next.vector_from(origin))
            .dot(normal);
        low = cadmpeg_ir::math::Point3::new(
            low.x.min(point.x),
            low.y.min(point.y),
            low.z.min(point.z),
        );
        high = cadmpeg_ir::math::Point3::new(
            high.x.max(point.x),
            high.y.max(point.y),
            high.z.max(point.z),
        );
    }
    Ok(Some(SignedArea {
        twice,
        extent: high.distance(low),
    }))
}

/// `twice_area` about the oriented normal of `support`: the sign is inverted
/// on a NURBS support whose oriented normal is opposite `Pu × Pv`.
fn support_oriented_area(twice_area: f64, support: &SolvedSurfaceGeometry) -> f64 {
    match support {
        SolvedSurfaceGeometry::Nurbs(nurbs) if nurbs.normal_reversed() => -twice_area,
        _ => twice_area,
    }
}

/// The winding of a Type 144 boundary against the neutral loop direction.
/// Type 144 and Type 142 state no traversal direction, and the face keeps the
/// support normal, so the outer boundary must keep the face on its left about
/// that normal and every inner boundary must keep the face on its right.
pub(super) fn trimmed_boundary_winding(
    items: &[BoundaryItem],
    outer: bool,
    index: &ModelIndex<'_>,
    support: &SurfaceGeometry,
    ctx: &DecodeContext<'_>,
) -> Result<BoundaryWinding, CodecError> {
    let solved = support.solved();
    let periods = solved.map_or([None, None], support_parameter_periods);
    let area = match parameter_boundary_signed_area(items, periods, ctx)? {
        Some(area) => Some(SignedArea {
            twice: solved.map_or(area.twice, |solved| {
                support_oriented_area(area.twice, solved)
            }),
            ..area
        }),
        None => match solved {
            Some(SolvedSurfaceGeometry::Plane(plane)) => planar_model_boundary_signed_area(
                items,
                index,
                *plane.frame().axis().as_raw(),
                ctx,
            )?,
            _ => None,
        },
    };
    Ok(winding_from_area(area, outer))
}

/// The winding a signed area gives an outer or inner boundary.
fn winding_from_area(area: Option<SignedArea>, outer: bool) -> BoundaryWinding {
    let Some(area) = area else {
        return BoundaryWinding::Unresolved(
            "it has no closed parameter-space image and no planar model-space image",
        );
    };
    if area.degenerate() {
        return BoundaryWinding::Unresolved("it encloses no area at the sampling resolution");
    }
    let backward = if outer {
        area.twice < 0.0
    } else {
        area.twice > 0.0
    };
    if backward {
        BoundaryWinding::Reversed
    } else {
        BoundaryWinding::Kept
    }
}

#[cfg(test)]
mod tests;
