// SPDX-License-Identifier: Apache-2.0
//! Traversal direction of a Type 144 boundary about its support normal.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, SurfaceParameterAxis};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::Sense;

use super::BoundaryItem;

/// Points sampled on each boundary curve to find the boundary's winding.
const BOUNDARY_WINDING_SAMPLES: u32 = 64;

/// A parameter-space image closes when, on each axis, its ends lie within
/// this fraction of the support's period on a periodic axis, or of the
/// image's extent on another axis. A boundary that turns around a periodic
/// axis leaves a gap of one period within this fraction of it.
/// A parameter curve approximated within the model tolerance leaves a gap
/// far below it.
const PARAMETER_RING_CLOSURE_FRACTION: f64 = 1.0e-2;

/// A boundary whose twice signed area is at most this fraction of the
/// square of its extent encloses no area at the sampling resolution and has
/// no winding.
const BOUNDARY_AREA_DEGENERATE_FRACTION: f64 = 1.0e-9;

/// A step between consecutive samples of an image that is longer than this
/// fraction of a turn on the turning axis is a jump across the seam. A
/// sampled parameter curve that runs once around steps far below it.
const TURN_JUMP_FRACTION: f64 = 0.5;

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
    match support_basis(support) {
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            nurbs_axis_lengths(nurbs, [nurbs.u_periodic(), nurbs.v_periodic()])
        }
        basis => analytic_periods(basis),
    }
}

/// The one axis on which `support` closes, with the parameter length of one
/// turn around it, or none where the support closes on no axis or on both.
/// A periodic axis closes over its period. A NURBS axis closes over its knot
/// domain where it is periodic or its two end isocurves agree within
/// `resolution`.
fn support_turn(
    support: &SolvedSurfaceGeometry,
    resolution: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(usize, f64)>, CodecError> {
    let lengths = match support_basis(support) {
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            let domains = nurbs_axis_domains(nurbs);
            let mut closed = [nurbs.u_periodic(), nurbs.v_periodic()];
            for (axis, fixed_axis) in [SurfaceParameterAxis::U, SurfaceParameterAxis::V]
                .into_iter()
                .enumerate()
            {
                if let (false, Some(fixed), Some(varying)) =
                    (closed[axis], domains[axis], domains[1 - axis])
                {
                    closed[axis] = super::super::surfaces::surface_boundary_is_closed(
                        ctx, nurbs, fixed_axis, fixed, varying, resolution,
                    )? == Some(true);
                }
            }
            nurbs_axis_lengths(nurbs, closed)
        }
        basis => analytic_periods(basis),
    };
    Ok(match lengths {
        [Some(length), None] => Some((0, length)),
        [None, Some(length)] => Some((1, length)),
        _ => None,
    })
}

/// `support` without its placements.
fn support_basis(support: &SolvedSurfaceGeometry) -> &SolvedSurfaceGeometry {
    match support {
        SolvedSurfaceGeometry::Transformed(placed) => support_basis(placed.basis()),
        basis => basis,
    }
}

/// The period of each periodic axis of an analytic `support`.
fn analytic_periods(support: &SolvedSurfaceGeometry) -> [Option<f64>; 2] {
    const TAU: f64 = std::f64::consts::TAU;
    match support {
        SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_) => [Some(TAU), None],
        SolvedSurfaceGeometry::Torus(_) => [Some(TAU), Some(TAU)],
        SolvedSurfaceGeometry::Plane(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Nurbs(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Unknown { .. } => [None, None],
    }
}

/// The knot domain of each axis of `nurbs`.
fn nurbs_axis_domains(nurbs: &NurbsSurface) -> [Option<[f64; 2]>; 2] {
    let domain = |degree: u32, knots: &[f64], count: usize| {
        cadmpeg_ir::eval::nurbs_pcurve_parameter_domain(degree, knots, count)
            .map(|domain| [domain.lower(), domain.upper()])
    };
    [
        domain(
            nurbs.u_degree(),
            nurbs.u_knots().as_slice(),
            nurbs.u_count(),
        ),
        domain(
            nurbs.v_degree(),
            nurbs.v_knots().as_slice(),
            nurbs.v_count(),
        ),
    ]
}

/// The length of the knot domain of each axis of `nurbs` that `selected`
/// names.
fn nurbs_axis_lengths(nurbs: &NurbsSurface, selected: [bool; 2]) -> [Option<f64>; 2] {
    let domains = nurbs_axis_domains(nurbs);
    [0, 1].map(|axis| {
        domains[axis]
            .filter(|_| selected[axis])
            .map(|[lower, upper]| upper - lower)
    })
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

/// The parameter-space image of a boundary traversed in source order. It is
/// absent when an item has no parameter curves or when a curve does not
/// evaluate.
fn parameter_image(
    items: &[BoundaryItem],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<[f64; 2]>>, CodecError> {
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
        if item.pcurves_run_backward() {
            item_points.reverse();
        }
        points.extend(item_points);
    }
    Ok(Some(points))
}

/// A boundary whose parameter-space image turns once around the one closed
/// axis of its support and closes on the other axis.
#[derive(Debug, Clone, Copy)]
struct Turn {
    axis: usize,
    /// Whether the image advances along `axis`.
    forward: bool,
    /// The mean coordinate of the image on the other axis, weighted by arc
    /// length along the image.
    level: f64,
}

/// The turn of an image around `axis`, whose turn is `length` long. The
/// ends of the image lie one turn apart on `axis` within
/// [`PARAMETER_RING_CLOSURE_FRACTION`] of the turn, and meet on the other
/// axis within that fraction of the image extent. No step between
/// consecutive samples is longer on `axis` than [`TURN_JUMP_FRACTION`] of
/// the turn.
fn turn_of(points: &[[f64; 2]], axis: usize, length: f64) -> Option<Turn> {
    let (first, last) = (*points.first()?, *points.last()?);
    let other = 1 - axis;
    let mut low = first;
    let mut high = first;
    for point in points {
        for axis in 0..2 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    let extent = (high[0] - low[0]).hypot(high[1] - low[1]);
    let gap = last[axis] - first[axis];
    let turns = (gap / length).round();
    let turned = turns.abs() == 1.0
        && (gap - turns * length).abs() <= PARAMETER_RING_CLOSURE_FRACTION * length
        && (last[other] - first[other]).abs() <= PARAMETER_RING_CLOSURE_FRACTION * extent
        && points
            .windows(2)
            .all(|pair| (pair[1][axis] - pair[0][axis]).abs() <= TURN_JUMP_FRACTION * length);
    if !turned {
        return None;
    }
    let (moment, arc) = points.windows(2).fold((0.0, 0.0), |(moment, arc), pair| {
        let step = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
        (
            step.mul_add(pair[0][other].midpoint(pair[1][other]), moment),
            arc + step,
        )
    });
    let level = moment / arc;
    level.is_finite().then_some(Turn {
        axis,
        forward: turns > 0.0,
        level,
    })
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

/// The winding of each Type 144 boundary of one face against the neutral
/// loop direction, from its items and whether it is the outer boundary.
/// Type 144 and Type 142 state no traversal direction, and the face keeps the
/// support normal, so each boundary must keep the face on its left about that
/// normal: the outer boundary runs counterclockwise and every inner boundary
/// clockwise. On a support that closes on exactly one axis within
/// `resolution`, exactly two boundaries that turn once around that axis
/// bound the strip between their levels on the other axis.
pub(super) fn trimmed_boundary_windings<'a, 'b: 'a>(
    boundaries: impl IntoIterator<Item = (&'a [BoundaryItem<'b>], bool)>,
    index: &ModelIndex<'_>,
    support: &SurfaceGeometry,
    resolution: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<BoundaryWinding>, CodecError> {
    let solved = support.solved();
    let periods = solved.map_or([None, None], support_parameter_periods);
    let turning_axis = match solved {
        Some(solved) => support_turn(solved, resolution, ctx)?,
        None => None,
    };
    let mut windings = Vec::new();
    let mut turns = Vec::new();
    for (items, outer) in boundaries {
        let image = parameter_image(items, ctx)?;
        let closed = image
            .as_deref()
            .and_then(|points| closed_parameter_area(points, periods));
        let area = match (closed, solved) {
            (Some(area), Some(solved)) => Some(SignedArea {
                twice: support_oriented_area(area.twice, solved),
                ..area
            }),
            (Some(area), None) => Some(area),
            (None, Some(SolvedSurfaceGeometry::Plane(plane))) => planar_model_boundary_signed_area(
                items,
                index,
                *plane.frame().axis().as_raw(),
                ctx,
            )?,
            (None, _) => None,
        };
        if let (None, Some(points), Some((axis, length))) = (area, image.as_deref(), turning_axis) {
            if let Some(turn) = turn_of(points, axis, length) {
                turns.push((windings.len(), turn));
            }
        }
        windings.push(winding_from_area(area, outer));
    }
    let normal_reversed = solved.is_some_and(|solved| support_oriented_area(1.0, solved) < 0.0);
    for (position, winding) in strip_windings(&turns, normal_reversed) {
        windings[position] = winding;
    }
    Ok(windings)
}

/// The winding of each of two boundaries that turn around one axis at
/// different levels, at their positions in `turns`, which keeps the strip
/// between them on the left of each about the support normal. The left of an
/// advance along `u` lies toward greater `v`, and the left of an advance along
/// `v` toward smaller `u`; a support normal opposite `Pu × Pv` exchanges the
/// two sides.
fn strip_windings(turns: &[(usize, Turn)], normal_reversed: bool) -> Vec<(usize, BoundaryWinding)> {
    let [(first, a), (second, b)] = turns else {
        return Vec::new();
    };
    let [lower, upper] = match a.level.partial_cmp(&b.level) {
        Some(std::cmp::Ordering::Less) => [(*first, *a), (*second, *b)],
        Some(std::cmp::Ordering::Greater) => [(*second, *b), (*first, *a)],
        _ => return Vec::new(),
    };
    [(lower, true), (upper, false)]
        .into_iter()
        .map(|((position, turn), strip_above)| {
            let left_above = (turn.forward == (turn.axis == 0)) != normal_reversed;
            let winding = if left_above == strip_above {
                BoundaryWinding::Kept
            } else {
                BoundaryWinding::Reversed
            };
            (position, winding)
        })
        .collect()
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
