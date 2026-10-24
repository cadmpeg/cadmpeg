// SPDX-License-Identifier: Apache-2.0
//! Regions on a NURBS carrier's finite parameter domain.
//!
//! - No ring loop: the face covers the whole domain (`whole_surface`).
//! - A curve-bounded construction with an implicit outer boundary: the
//!   domain is the outer boundary and every loop is a hole
//!   (`domain_exterior`).
//! - Classified loops: the classified outer loop excludes a domain-supplied
//!   exterior, so the planar rule decides the region (`nesting`).
//! - Unspecified loops: the region lies on the left of every loop, after
//!   each loop is reversed when the declared normal is reversed
//!   (`declared_orientation`). Its indicator is the sum of the oriented
//!   loops' winding numbers plus a constant `c`, which is 1 when the region
//!   reaches the domain's natural edges and 0 otherwise; a point just left
//!   of a loop is a member and fixes `c`.

use cadmpeg_ir::math::Point2;
use cadmpeg_ir::topology::{Face, FaceLoops};

use super::{
    degenerate, invalid, planar, ring_loops, winding_number, LoopRegion, LoopShape, Region,
};
use crate::boundary::ResolvedFace;
use crate::canonical::surface::Chart;
use crate::failure::{Measure, MeasureFailure};
use crate::reason::UnavailableReason;
use crate::report::{Classification, LoopRole, NormalReading, RegionRule};
use crate::tolerance::MEMBER_OFFSET_RELATIVE;

/// The region of a face on a NURBS carrier.
pub(super) fn classify(
    face: &Face,
    chart: &Chart<'_>,
    resolved: &ResolvedFace<'_>,
    shapes: &[Option<LoopShape>],
    context: FiniteContext,
) -> Measure<Region> {
    let Some(domain) = chart.domain() else {
        return Err(invalid("the carrier has no finite domain"));
    };
    let rings = ring_loops(resolved)?;
    let base = domain[1][0];
    let vertex_loops = vec![
        LoopRegion {
            role: LoopRole::Vertex,
            factor: 0.0,
        };
        resolved.loops.len()
    ];
    if rings.is_empty() {
        return Ok(Region {
            rule: RegionRule::WholeSurface,
            loops: vertex_loops,
            base,
            classification: Classification::Unspecified,
            apex: None,
            exterior: 1.0,
            closed: None,
        });
    }
    if chart.periods().iter().any(Option::is_some) {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::RegionDomainUnsupported,
            Some(face.id.as_str()),
            "regions bounded by loops on a periodic NURBS domain are not classified in this build",
        ));
    }
    let mut members: Vec<(usize, &LoopShape)> = Vec::new();
    for (index, resolved_loop) in &rings {
        let Some(Some(shape)) = shapes.get(*index) else {
            return Err(invalid("a loop has no parameter-plane polyline"));
        };
        if resolved_loop.net_shift != [0, 0] {
            return Err(invalid("a loop wraps a finite domain"));
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
    if context.implicit_outer {
        let mut loops = vertex_loops;
        for (position, (index, shape)) in members.iter().enumerate() {
            for (_, other_shape) in members.iter().skip(position + 1) {
                let contains = |a: &LoopShape, b: &LoopShape| {
                    b.points
                        .first()
                        .is_some_and(|point| winding_number(&a.points, *point) != 0)
                };
                if contains(shape, other_shape) || contains(other_shape, shape) {
                    return Err(invalid("one hole contains another"));
                }
            }
            loops[*index] = LoopRegion {
                role: LoopRole::Inner,
                factor: if shape.integral.value > 0.0 {
                    -1.0
                } else {
                    1.0
                },
            };
        }
        return Ok(Region {
            rule: RegionRule::DomainExterior,
            loops,
            base,
            classification: Classification::Unspecified,
            apex: None,
            exterior: 1.0,
            closed: None,
        });
    }
    if matches!(face.loops, FaceLoops::Classified { .. }) {
        let mut region = planar(face, resolved, shapes)?;
        region.base = base;
        return Ok(region);
    }
    let sign = match context.declared {
        NormalReading::Reversed => -1.0,
        _ => 1.0,
    };
    let mut loops = vertex_loops;
    for (index, shape) in &members {
        let counterclockwise = shape.integral.value * sign > 0.0;
        loops[*index] = LoopRegion {
            role: if counterclockwise {
                LoopRole::Outer
            } else {
                LoopRole::Inner
            },
            factor: sign,
        };
    }
    let exterior = exterior_constant(&members, sign)?;
    Ok(Region {
        rule: RegionRule::DeclaredOrientation,
        loops,
        base,
        classification: Classification::Unspecified,
        apex: None,
        exterior,
        closed: None,
    })
}

/// What a finite-domain classification reads beyond the loops.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FiniteContext {
    /// Whether the carrier is a curve-bounded construction with an implicit
    /// outer boundary.
    pub(crate) implicit_outer: bool,
    /// The oriented normal the face's sense states.
    pub(crate) declared: NormalReading,
}

/// The constant `c` of the region indicator: `1 − Σ sign·w(p)` at a point
/// `p` just left of the oriented loop with the longest segment.
fn exterior_constant(members: &[(usize, &LoopShape)], sign: f64) -> Measure<f64> {
    let mut best: Option<([f64; 2], [f64; 2], f64)> = None;
    for (_, shape) in members {
        let points = &shape.points;
        for (index, a) in points.iter().enumerate() {
            let b = points[(index + 1) % points.len()];
            let length = (b[0] - a[0]).hypot(b[1] - a[1]);
            if best.is_none_or(|(_, _, longest)| length > longest) {
                best = Some((*a, b, length));
            }
        }
    }
    let Some((a, b, length)) = best else {
        return Err(invalid("the loops have no segment"));
    };
    let size = members
        .iter()
        .map(|(_, shape)| polyline_size(&shape.points))
        .fold(0.0, f64::max);
    let offset = MEMBER_OFFSET_RELATIVE * size / length;
    let left = [-(b[1] - a[1]) * sign, (b[0] - a[0]) * sign];
    let point = [
        a[0].midpoint(b[0]) + left[0] * offset,
        a[1].midpoint(b[1]) + left[1] * offset,
    ];
    let mut winding = 0.0;
    for (_, shape) in members {
        let count = winding_number(&shape.points, point);
        let Some(count) = cadmpeg_core::convert::f64_from_i64(count) else {
            return Err(invalid("a winding number left the finite range"));
        };
        winding += sign * count;
    }
    let exterior = 1.0 - winding;
    if exterior == 0.0 || exterior == 1.0 {
        Ok(exterior)
    } else {
        Err(invalid("the loops' declared sides contradict each other"))
    }
}

/// The box diagonal of a parameter-plane polyline.
fn polyline_size(points: &[[f64; 2]]) -> f64 {
    let mut low = Point2::new(f64::INFINITY, f64::INFINITY);
    let mut high = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for point in points {
        low = Point2::new(low.u.min(point[0]), low.v.min(point[1]));
        high = Point2::new(high.u.max(point[0]), high.v.max(point[1]));
    }
    (high.u - low.u).hypot(high.v - low.v)
}
