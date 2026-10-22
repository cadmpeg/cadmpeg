// SPDX-License-Identifier: Apache-2.0
//! Region classification: which part of the carrier's domain a face covers.
//!
//! On an unbounded planar domain the loops decide the region without the
//! face's orientation: the outer loop is the one loop that winds around a
//! point of every other loop, and the rest are holes that do not contain
//! each other. Each loop is then oriented by its role, outer
//! counterclockwise and holes clockwise, so the region lies on the left of
//! every loop. Cylinders and cones follow the periodic rules of
//! [`periodic`], NURBS carriers the finite-domain rules of [`finite`].

mod closed;
mod finite;
mod membership;
mod periodic;

pub(crate) use finite::FiniteContext;
pub(crate) use membership::Indicator;

use std::cmp::Ordering;

use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::topology::{Face, FaceLoops};

use crate::boundary::{LoopKind, LoopResolution, ResolvedFace, ResolvedLoop};
use crate::canonical::surface::Chart;
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::geom::Aabb;
use crate::reason::UnavailableReason;
use crate::report::{Classification, LoopRole, RegionRule, Winding};
use crate::tolerance::{LOOP_DEGENERATE_RELATIVE_AREA, NESTING_SAMPLE_RELATIVE_DEVIATION};

/// The parameter-plane polyline of a resolved ring loop and its own Green
/// integral.
pub(crate) struct LoopShape {
    pub(crate) points: Vec<[f64; 2]>,
    /// The loop's model-space box diagonal.
    pub(crate) diagonal: f64,
    /// `∮ (G(v) − G(b)) du` from the loop's own lowest `v`, in mm².
    pub(crate) integral: LoopIntegral,
}

/// A loop's own Green integral and its error estimate.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopIntegral {
    pub(crate) value: f64,
    pub(crate) error: f64,
    /// The base `b` of the integral.
    pub(crate) base: f64,
}

/// The polyline of a ring loop, refined against the loop's own size.
pub(crate) fn loop_shape(
    eval: &Eval<'_, '_>,
    chart: &Chart,
    resolved: &ResolvedLoop<'_>,
) -> Measure<LoopShape> {
    let mut coarse: Vec<[f64; 2]> = Vec::new();
    for piece in &resolved.pieces {
        coarse.extend(piece.polyline(eval, chart, f64::INFINITY)?);
    }
    let (low, high) = bounds(&coarse);
    let size = (high[0] - low[0]).hypot(high[1] - low[1]);
    let deviation = NESTING_SAMPLE_RELATIVE_DEVIATION * size;
    let mut points: Vec<[f64; 2]> = Vec::new();
    for piece in &resolved.pieces {
        for point in piece.polyline(eval, chart, deviation)? {
            if points.last() != Some(&point) {
                points.push(point);
            }
        }
    }
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    let mut aabb: Option<Aabb> = None;
    for point in &points {
        let surface = chart.partials(eval, point[0], point[1])?.point;
        match aabb.as_mut() {
            Some(aabb) => aabb.include(surface),
            None => aabb = Some(Aabb::of_point(surface)),
        }
    }
    let diagonal = aabb.map_or(0.0, |aabb| aabb.diagonal());
    let base = points
        .iter()
        .map(|point| point[1])
        .fold(f64::INFINITY, f64::min);
    let integral = crate::area::loop_integral(eval, chart, resolved, base, diagonal)?;
    Ok(LoopShape {
        points,
        diagonal,
        integral,
    })
}

fn bounds(points: &[[f64; 2]]) -> ([f64; 2], [f64; 2]) {
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for point in points {
        for axis in 0..2 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    (low, high)
}

/// The winding number of a closed polyline around `point`.
pub(crate) fn winding_number(polyline: &[[f64; 2]], point: [f64; 2]) -> i64 {
    let p = Point2::new(point[0], point[1]);
    let mut winding = 0_i64;
    for (index, a) in polyline.iter().enumerate() {
        let b = polyline[(index + 1) % polyline.len()];
        let (a, b) = (Point2::new(a[0], a[1]), Point2::new(b[0], b[1]));
        let side = cadmpeg_ir::math::planar::orientation(a, b, p);
        if a.v <= p.v {
            if b.v > p.v && side == Some(Ordering::Greater) {
                winding += 1;
            }
        } else if b.v <= p.v && side == Some(Ordering::Less) {
            winding -= 1;
        }
    }
    winding
}

/// The traversal direction of a loop.
pub(crate) fn winding(resolved: &ResolvedLoop<'_>, shape: Option<&LoopShape>) -> Winding {
    match resolved.kind {
        LoopKind::Vertex(_) => Winding::Point,
        LoopKind::Ring => {
            let shift = resolved.net_shift;
            if shift[0] > 0 || (shift[0] == 0 && shift[1] > 0) {
                Winding::NonContractibleForward
            } else if shift != [0, 0] {
                Winding::NonContractibleBackward
            } else if shape.is_some_and(|shape| shape.integral.value < 0.0) {
                Winding::Clockwise
            } else {
                Winding::Counterclockwise
            }
        }
    }
}

/// One loop's role in the region and the orientation that puts the region
/// on its left.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopRegion {
    pub(crate) role: LoopRole,
    /// `+1` when the traversal already has the role's orientation, `−1`
    /// when it runs against it, `0` for a loop that bounds no area.
    pub(crate) factor: f64,
}

/// The region of a face.
pub(crate) struct Region {
    pub(crate) rule: RegionRule,
    /// Per loop of the face, in face order.
    pub(crate) loops: Vec<LoopRegion>,
    /// The base `v₀` of the inner area integral.
    pub(crate) base: f64,
    pub(crate) classification: Classification,
    /// The cone apex, when it is a member of the region.
    pub(crate) apex: Option<Point3>,
    /// The factor, 0 or 1, of the whole domain's area in the region's area:
    /// 1 when the region reaches the domain's natural edges.
    pub(crate) exterior: f64,
    /// What a region on a sphere or torus adds to its loops.
    pub(crate) closed: Option<ClosedRegion>,
}

/// What a region on a closed domain adds to its loops. The domain is
/// `u ∈ [0, 2π)` by `v ∈ [base, base + span]`; Green's theorem on it adds
/// the parts of the upper edge in the region to the loops' integrals.
pub(crate) struct ClosedRegion {
    /// The `u`-intervals of the upper edge `v = base + span` in the region.
    pub(crate) top: Vec<[f64; 2]>,
    /// The cut of a doubly periodic domain.
    pub(crate) wrap: Option<Wrap>,
    /// Which points of the domain are members.
    pub(crate) membership: Membership,
}

/// A doubly periodic domain cut at `v = base`: every piece's `v` is taken
/// in `[base, base + period)`.
pub(crate) struct Wrap {
    pub(crate) period: f64,
    /// Per loop and piece, the piece parameters where the piece crosses
    /// the cut.
    pub(crate) breaks: Vec<Vec<Vec<f64>>>,
}

/// Which points of a closed domain belong to a region.
pub(crate) enum Membership {
    /// Every point.
    Whole,
    /// The points where the indicator of the oriented loops is 1.
    Loops(Indicator),
}

impl Membership {
    /// Whether the parameter point `uv`, off the boundary, is a member.
    pub(crate) fn member(&self, uv: [f64; 2]) -> bool {
        match self {
            Self::Whole => true,
            Self::Loops(indicator) => indicator.at(uv) == 1,
        }
    }

    /// Whether the region reaches the lower (`toward = −1`) or upper
    /// (`toward = 1`) end of a domain whose `v` is not periodic, beyond
    /// every boundary point.
    pub(crate) fn end(&self, toward: f64) -> bool {
        match self {
            Self::Whole => true,
            Self::Loops(indicator) => indicator.beyond(toward) == 1,
        }
    }
}

/// What the region rules read beyond the loops' polylines.
pub(crate) struct RegionInput<'a, 'ir> {
    pub(crate) eval: Eval<'a, 'ir>,
    pub(crate) context: FiniteContext,
    /// Per loop and piece, whether the piece lies on a seam of the face.
    pub(crate) seams: &'a [Vec<bool>],
}

/// The ring loops of a face, or the first loop failure.
fn ring_loops<'a, 'ir>(
    resolved: &'a ResolvedFace<'ir>,
) -> Measure<Vec<(usize, &'a ResolvedLoop<'ir>)>> {
    let mut rings = Vec::new();
    for (index, resolution) in resolved.loops.iter().enumerate() {
        let resolved_loop = resolution.outcome.as_ref().map_err(Clone::clone)?;
        if matches!(resolved_loop.kind, LoopKind::Ring) {
            rings.push((index, resolved_loop));
        }
    }
    Ok(rings)
}

fn invalid(message: &str) -> MeasureFailure {
    MeasureFailure::unavailable(
        UnavailableReason::BoundaryNestingInvalid,
        None,
        message.to_owned(),
    )
}

/// Classify the region of `face` from its resolved loops. `v_tolerance` is
/// the allowance in units of `v`.
pub(crate) fn classify(
    face: &Face,
    chart: &Chart<'_>,
    resolved: &ResolvedFace<'_>,
    shapes: &[Option<LoopShape>],
    v_tolerance: f64,
    input: &RegionInput<'_, '_>,
) -> Measure<Region> {
    let context = input.context;
    if context.implicit_outer
        && matches!(chart, Chart::Plane(_) | Chart::Cylinder(_) | Chart::Cone(_))
    {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::BoundaryUnboundedFace,
            Some(face.id.as_str()),
            "the face's outer boundary is the natural boundary of an unbounded support",
        ));
    }
    match chart {
        Chart::Plane(_) => planar(face, resolved, shapes),
        Chart::Cylinder(_) | Chart::Cone(_) => {
            periodic::classify(face, chart, resolved, shapes, v_tolerance)
        }
        Chart::Nurbs(_) => finite::classify(face, chart, resolved, shapes, context),
        Chart::Sphere(_) | Chart::Torus(_) => {
            closed::classify(face, chart, resolved, shapes, input)
        }
    }
}

/// Nesting on an unbounded planar domain.
fn planar(
    face: &Face,
    resolved: &ResolvedFace<'_>,
    shapes: &[Option<LoopShape>],
) -> Measure<Region> {
    let rings = ring_loops(resolved)?;
    if rings.is_empty() {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::BoundaryUnboundedFace,
            Some(face.id.as_str()),
            "a face on an unbounded plane has no boundary loop",
        ));
    }
    let mut members = Vec::new();
    for (index, resolved_loop) in &rings {
        let Some(Some(shape)) = shapes.get(*index) else {
            return Err(invalid("a loop has no parameter-plane polyline"));
        };
        if resolved_loop.net_shift != [0, 0] {
            return Err(invalid("a loop on a plane wraps a period"));
        }
        if degenerate(shape) {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryLoopDegenerate,
                Some(resolved.loops[*index].face_loop.id.as_str()),
                "a ring loop encloses no area relative to its own size",
            ));
        }
        members.push((*index, shape));
    }
    let outers: Vec<usize> = members
        .iter()
        .filter(|(index, shape)| {
            members
                .iter()
                .filter(|(other, _)| other != index)
                .all(|(_, other)| {
                    other
                        .points
                        .first()
                        .is_some_and(|point| winding_number(&shape.points, *point) != 0)
                })
        })
        .map(|(index, _)| *index)
        .collect();
    let [outer] = outers.as_slice() else {
        return Err(invalid("no single loop encloses every other loop"));
    };
    for (a, shape_a) in &members {
        for (b, shape_b) in &members {
            if a == b || a == outer || b == outer {
                continue;
            }
            if shape_b
                .points
                .first()
                .is_some_and(|point| winding_number(&shape_a.points, *point) != 0)
            {
                return Err(invalid("one hole contains another"));
            }
        }
    }
    let mut loops = vec![
        LoopRegion {
            role: LoopRole::Vertex,
            factor: 0.0,
        };
        resolved.loops.len()
    ];
    let mut base = f64::INFINITY;
    for (index, shape) in &members {
        let counterclockwise = shape.integral.value > 0.0;
        let (role, wanted) = if index == outer {
            (LoopRole::Outer, true)
        } else {
            (LoopRole::Inner, false)
        };
        loops[*index] = LoopRegion {
            role,
            factor: if counterclockwise == wanted {
                1.0
            } else {
                -1.0
            },
        };
        for point in &shape.points {
            base = base.min(point[1]);
        }
    }
    Ok(Region {
        rule: RegionRule::Nesting,
        loops,
        base,
        classification: classification(face, resolved, *outer),
        apex: None,
        exterior: 0.0,
        closed: None,
    })
}

/// Whether a loop's enclosed area is negligible against its own size.
fn degenerate(shape: &LoopShape) -> bool {
    shape.points.len() < 3
        || shape.integral.value.abs()
            <= LOOP_DEGENERATE_RELATIVE_AREA * shape.diagonal * shape.diagonal
}

/// The computed outer loop against the stored classification.
fn classification(face: &Face, resolved: &ResolvedFace<'_>, outer: usize) -> Classification {
    match &face.loops {
        FaceLoops::Unspecified { .. } => Classification::Unspecified,
        FaceLoops::Classified { outer: stored, .. } => {
            let computed: Option<&LoopResolution<'_>> = resolved.loops.get(outer);
            if computed.is_some_and(|computed| computed.face_loop.id == *stored) {
                Classification::Consistent
            } else {
                Classification::Conflict
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::winding_number;

    #[test]
    fn winding_numbers_count_turns_around_a_point() {
        let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert_eq!(winding_number(&square, [0.5, 0.5]), 1);
        assert_eq!(winding_number(&square, [1.5, 0.5]), 0);
        let reversed: Vec<[f64; 2]> = square.iter().rev().copied().collect();
        assert_eq!(winding_number(&reversed, [0.5, 0.5]), -1);
    }
}
