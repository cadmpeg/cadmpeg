// SPDX-License-Identifier: Apache-2.0
//! Refinement of a degree-one surface-intersection chart onto the exact
//! intersection of its two supports: analytic surfaces, the pipe surface of a
//! rolling-ball blend, or a blend side's contact track.
//!
//! The chart is a solved cache of the intersection within its chordal error
//! (`sldprt.md` §7.3). Each chart point moves to the nearest point of the
//! intersection by Newton steps on the two supports' tangent planes, and
//! takes the intersection's tangent there. One cubic Hermite piece spans as
//! many stored points as it can while its interior stays within
//! [`REFINED_CHORD_TOLERANCE_MM`] of the intersection; a stored segment that
//! one piece cannot span is split at its parameter midpoint. A node keeps
//! its stored parameter. The chart's parameter domain is unchanged.
//!
//! Where the supports touch, a chart point that lies on both supports within
//! the tolerance stays where it is stored and takes the tangent of the
//! circular arc through its neighbour.

use std::cell::RefCell;

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};

/// Departure from the intersection that a refined chart admits: the linear
/// resolution (spec §7.2).
pub(super) const REFINED_CHORD_TOLERANCE_MM: f64 = super::graph::LINEAR_RESOLUTION_MM;
/// Departure a piece at the split depth limit may keep; the chart's edges
/// carry it as their tolerance.
const REFINED_CHORD_LIMIT_MM: f64 = 1.0e-3;
/// Steps a trace takes over the chord between its ends.
const TRACE_STEPS_PER_CHORD: usize = 32;
/// Steps from the far end within which a trace arrives at it.
const TRACE_ARRIVAL_STEPS: f64 = 1.5;
/// Share of a step a traced point must advance; a smaller advance has
/// stalled.
const TRACE_MIN_ADVANCE: f64 = 0.25;
/// Points a trace may hold.
const MAX_TRACE_POINTS: usize = 1 << 12;
/// Distance between successive Newton iterates at which a point has converged.
const NEWTON_STEP_TOLERANCE_MM: f64 = 1.0e-11;
/// Newton iterations allowed for one point.
const MAX_NEWTON_STEPS: usize = 32;
/// Smallest `|n1 × n2|²` of a transversal intersection.
const MIN_TRANSVERSALITY: f64 = 1.0e-8;
/// Midpoint splits allowed below one stored chart segment.
const MAX_SUBDIVISION_DEPTH: usize = 10;
/// Largest share of its piece's departure a half piece may keep when a
/// checked point was measured against the supports, beyond which splitting
/// no longer converges.
const SUPPORT_SPLIT_CONVERGENCE: f64 = 0.5;
/// Pieces a refined chart may hold.
const MAX_REFINED_PIECES: usize = 1 << 10;
/// Interior fractions of a piece at which its departure is checked.
const CHECK_FRACTIONS: [f64; 7] = [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875];

/// A refined chart and the largest departure of a checked interior point
/// from the intersection.
pub(super) struct RefinedChart {
    pub(super) curve: NurbsCurve,
    pub(super) chord_error_mm: f64,
}

/// One node of the refined chart: its parameter, its intersection point and
/// the unit tangent of the intersection, oriented along increasing parameter.
#[derive(Clone, Copy)]
struct Node {
    parameter: f64,
    point: Point3,
    tangent: Vector3,
}

/// Refine a degree-one polynomial chart onto the intersection of its
/// supports, or `None` when a point does not converge or a chart point where
/// the supports touch lies off either.
///
/// # Errors
///
/// Propagates an evaluator's resource refusal.
pub(super) fn refine(
    chart: &CurveGeometry,
    supports: [Support<'_, '_, '_>; 2],
) -> Result<Option<RefinedChart>, CodecError> {
    let refiner = Refiner {
        supports,
        refusal: RefCell::new(None),
    };
    let refined = refine_with(&refiner, chart);
    finish(refiner, refined)
}

/// The intersection of two supports traced from `ends[0]` to `ends[1]`, as a
/// refined chart parameterized by chord length in millimetres, or `None` when
/// an end does not project onto the intersection, the ends coincide, or the
/// trace does not reach the far end.
///
/// # Errors
///
/// Propagates an evaluator's resource refusal.
pub(super) fn trace(
    supports: [Support<'_, '_, '_>; 2],
    ends: [Point3; 2],
) -> Result<Option<RefinedChart>, CodecError> {
    let refiner = Refiner {
        supports,
        refusal: RefCell::new(None),
    };
    let traced = trace_with(&refiner, ends);
    finish(refiner, traced)
}

/// `result`, or the first refusal the refiner or a blend support met.
fn finish(
    refiner: Refiner<'_, '_>,
    result: Option<RefinedChart>,
) -> Result<Option<RefinedChart>, CodecError> {
    for support in refiner.supports {
        if let Support::Pipe(blend) | Support::Track(blend, _) = support {
            if let Some(error) = blend.take_refusal() {
                refiner.refusal.borrow_mut().get_or_insert(error);
            }
        }
    }
    match refiner.refusal.into_inner() {
        Some(error) => Err(error),
        None => Ok(result),
    }
}

fn trace_with(refiner: &Refiner<'_, '_>, ends: [Point3; 2]) -> Option<RefinedChart> {
    let (start, mut tangent) = project(refiner, ends[0])?;
    let (end, _) = project(refiner, ends[1])?;
    let chord = start.distance(end);
    if !(chord > NEWTON_STEP_TOLERANCE_MM && chord.is_finite()) {
        return None;
    }
    let step = chord / cadmpeg_core::convert::f64_from_index(TRACE_STEPS_PER_CHORD)?;
    if tangent.dot(end.vector_from(start)) < 0.0 {
        tangent = Vector3::new(-tangent.x, -tangent.y, -tangent.z);
    }
    let mut points = vec![start];
    loop {
        let current = *points.last()?;
        if current.distance(end) <= step * TRACE_ARRIVAL_STEPS {
            points.push(end);
            break;
        }
        if points.len() > MAX_TRACE_POINTS {
            return None;
        }
        let ahead = Point3::new(
            current.x + tangent.x * step,
            current.y + tangent.y * step,
            current.z + tangent.z * step,
        );
        let (next, next_tangent) = project(refiner, ahead)?;
        let advance = next.vector_from(current);
        if advance.norm() < step * TRACE_MIN_ADVANCE {
            return None;
        }
        tangent = if next_tangent.dot(advance) < 0.0 {
            Vector3::new(-next_tangent.x, -next_tangent.y, -next_tangent.z)
        } else {
            next_tangent
        };
        points.push(next);
    }
    let mut knots = vec![0.0, 0.0];
    let mut length = 0.0;
    for pair in points.windows(2) {
        length += pair[0].distance(pair[1]);
        knots.push(length);
    }
    knots.push(length);
    let polyline = NurbsCurve::new(1, knots, NurbsPoles3::Polynomial { points }, false).ok()?;
    refine_with(
        refiner,
        &CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(polyline)),
    )
}

/// The evaluation context and supports of one refinement, and the first
/// resource refusal an evaluation met.
struct Refiner<'a, 'ctx> {
    supports: [Support<'a, 'a, 'ctx>; 2],
    refusal: RefCell<Option<CodecError>>,
}

/// One support of a chart: an analytic surface, the pipe surface of a
/// rolling-ball blend, or one side's contact track of such a blend, which a
/// blend-boundary record names.
#[derive(Clone, Copy)]
pub(super) enum Support<'a, 'g, 'ctx> {
    Analytic(&'a SurfaceGeometry),
    Pipe(&'a super::blend_fit::BlendGeometry<'g, 'ctx>),
    Track(&'a super::blend_fit::BlendGeometry<'g, 'ctx>, usize),
}

fn refine_with(refiner: &Refiner<'_, '_>, chart: &CurveGeometry) -> Option<RefinedChart> {
    let supports = refiner.supports;
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = chart else {
        return None;
    };
    if nurbs.degree() != 1 || nurbs.periodic() {
        return None;
    }
    let NurbsPoles3::Polynomial { points } = nurbs.pole_rows() else {
        return None;
    };
    if supports
        .iter()
        .any(|support| matches!(support, Support::Analytic(surface) if !analytic(surface)))
    {
        return None;
    }
    let knots = nurbs.knots().as_slice();
    if knots.len() != points.len() + 2 || points.len() < 2 {
        return None;
    }
    let mut stored = Vec::with_capacity(points.len());
    for (index, point) in points.iter().enumerate() {
        let parameter = *knots.get(index + 1)?;
        let (point, tangent) = match project(refiner, point.get()) {
            Some((point, tangent)) => (point, Some(tangent)),
            None if on_supports(refiner, point.get())? <= REFINED_CHORD_TOLERANCE_MM => {
                (point.get(), None)
            }
            None => return None,
        };
        stored.push((parameter, point, tangent));
    }
    let mut nodes = Vec::with_capacity(stored.len());
    for (index, &(parameter, point, tangent)) in stored.iter().enumerate() {
        let next = stored.get(index + 1);
        let previous = index.checked_sub(1).and_then(|i| stored.get(i));
        let toward = match (next, previous) {
            (Some(next), _) => next.1.vector_from(point),
            (None, Some(previous)) => point.vector_from(previous.1),
            (None, None) => return None,
        };
        let tangent = match tangent {
            Some(tangent) => tangent,
            None => {
                let neighbour = next.or(previous)?;
                arc_tangent(point, neighbour.1, neighbour.2)?
            }
        };
        let sign = if tangent.dot(toward) < 0.0 { -1.0 } else { 1.0 };
        nodes.push(Node {
            parameter,
            point,
            tangent: Vector3::new(tangent.x * sign, tangent.y * sign, tangent.z * sign),
        });
    }
    let mut refined = vec![*nodes.first()?];
    let mut chord_error_mm: f64 = 0.0;
    let last = nodes.len() - 1;
    let mut from = 0;
    while from < last {
        // The farthest stored node one piece reaches within the tolerance.
        let mut reach = None;
        for to in from + 1..=last {
            match departure(refiner, nodes[from], nodes[to]) {
                Some(Departure { error, .. }) if error <= REFINED_CHORD_TOLERANCE_MM => {
                    reach = Some((to, error));
                }
                _ => break,
            }
        }
        match reach {
            Some((to, error)) => {
                chord_error_mm = chord_error_mm.max(error);
                refined.push(nodes[to]);
                from = to;
            }
            None => {
                let (start, end) = (nodes[from], nodes[from + 1]);
                split(
                    refiner,
                    start,
                    end,
                    0,
                    None,
                    &mut refined,
                    &mut chord_error_mm,
                )?;
                from += 1;
            }
        }
        if refined.len() > MAX_REFINED_PIECES {
            return None;
        }
    }
    let curve = hermite_curve(&refined)?;
    Some(RefinedChart {
        curve,
        chord_error_mm,
    })
}

/// The Bézier control points of the cubic Hermite piece between two nodes.
/// The end derivatives have the node tangents' directions and the handle
/// length of the circular arc that turns between them.
fn piece(start: Node, end: Node) -> Option<[Point3; 4]> {
    let span = end.parameter - start.parameter;
    if !(span > 0.0 && span.is_finite()) {
        return None;
    }
    // The handle length that reproduces a circular arc between the two
    // tangents: `chord / (3·cos²(θ/4))` for turning angle `θ`.
    let turning = start.tangent.dot(end.tangent).clamp(-1.0, 1.0).acos();
    let quarter_cosine = (turning / 4.0).cos();
    let third = start.point.distance(end.point) / (3.0 * quarter_cosine * quarter_cosine);
    let offset = |point: Point3, direction: Vector3, scale: f64| {
        Point3::new(
            point.x + direction.x * scale,
            point.y + direction.y * scale,
            point.z + direction.z * scale,
        )
    };
    Some([
        start.point,
        offset(start.point, start.tangent, third),
        offset(end.point, end.tangent, -third),
        end.point,
    ])
}

fn bezier(control: &[Point3; 4], fraction: f64) -> Point3 {
    let other = 1.0 - fraction;
    let weights = [
        other * other * other,
        3.0 * other * other * fraction,
        3.0 * other * fraction * fraction,
        fraction * fraction * fraction,
    ];
    let mut point = Point3::new(0.0, 0.0, 0.0);
    for (control, weight) in control.iter().zip(weights) {
        point.x += control.x * weight;
        point.y += control.y * weight;
        point.z += control.z * weight;
    }
    point
}

/// The largest distance of a checked interior point of the piece between two
/// nodes from the intersection.
fn departure(refiner: &Refiner<'_, '_>, start: Node, end: Node) -> Option<Departure> {
    let control = piece(start, end)?;
    let mut departure: f64 = 0.0;
    let mut measured_on_supports = false;
    for fraction in CHECK_FRACTIONS {
        let sample = bezier(&control, fraction);
        let error = match project(refiner, sample) {
            Some((exact, _)) => exact.distance(sample),
            None => {
                measured_on_supports = true;
                on_supports(refiner, sample)?
            }
        };
        departure = departure.max(error);
    }
    Some(Departure {
        error: departure,
        measured_on_supports,
    })
}

/// The departure of a piece, and whether a checked point was measured
/// against the supports.
#[derive(Clone, Copy)]
struct Departure {
    error: f64,
    measured_on_supports: bool,
}

/// The larger distance of `point` from the two supports' surfaces.
fn on_supports(refiner: &Refiner<'_, '_>, point: Point3) -> Option<f64> {
    let mut distance: f64 = 0.0;
    for support in refiner.supports {
        let (foot, _) = match support {
            Support::Analytic(surface) => super::evaluation::analytic_surface_foot(surface, point)?,
            Support::Pipe(blend) | Support::Track(blend, _) => blend.pipe_foot(point)?,
        };
        distance = distance.max(foot.distance(point));
    }
    Some(distance)
}

/// The unit tangent at `point` of the circular arc that leaves `neighbour`
/// along `tangent`, up to sign, or the chord's direction when `tangent` is
/// absent.
fn arc_tangent(point: Point3, neighbour: Point3, tangent: Option<Vector3>) -> Option<Vector3> {
    let chord = point.vector_from(neighbour);
    let length = chord.norm();
    if !(length > 0.0 && length.is_finite()) {
        return None;
    }
    let chord = Vector3::new(chord.x / length, chord.y / length, chord.z / length);
    let Some(tangent) = tangent else {
        return Some(chord);
    };
    // The arc's end tangent is its start tangent reflected about the chord.
    let along = 2.0 * tangent.dot(chord);
    Some(Vector3::new(
        chord.x * along - tangent.x,
        chord.y * along - tangent.y,
        chord.z * along - tangent.z,
    ))
}

/// Append the nodes after `start` up to and including `end`. `parent` is the
/// departure of the piece this one halves.
fn split(
    refiner: &Refiner<'_, '_>,
    start: Node,
    end: Node,
    depth: usize,
    parent: Option<f64>,
    out: &mut Vec<Node>,
    chord_error_mm: &mut f64,
) -> Option<()> {
    let control = piece(start, end)?;
    let Departure {
        error: departure,
        measured_on_supports,
    } = departure(refiner, start, end)?;
    let stalled = measured_on_supports
        && parent.is_some_and(|parent| departure > parent * SUPPORT_SPLIT_CONVERGENCE);
    if departure <= REFINED_CHORD_TOLERANCE_MM
        || ((stalled || depth >= MAX_SUBDIVISION_DEPTH) && departure <= REFINED_CHORD_LIMIT_MM)
    {
        *chord_error_mm = chord_error_mm.max(departure);
        out.push(end);
        return Some(());
    }
    if stalled || depth >= MAX_SUBDIVISION_DEPTH || out.len() >= MAX_REFINED_PIECES {
        return None;
    }
    let (point, tangent) = match project(refiner, bezier(&control, 0.5)) {
        Some(exact) => exact,
        None => {
            let middle = bezier(&control, 0.5);
            (
                middle,
                arc_tangent(middle, start.point, Some(start.tangent))?,
            )
        }
    };
    let toward = end.point.vector_from(start.point);
    let sign = if tangent.dot(toward) < 0.0 { -1.0 } else { 1.0 };
    let middle = Node {
        parameter: start.parameter.midpoint(end.parameter),
        point,
        tangent: Vector3::new(tangent.x * sign, tangent.y * sign, tangent.z * sign),
    };
    split(
        refiner,
        start,
        middle,
        depth + 1,
        Some(departure),
        out,
        chord_error_mm,
    )?;
    split(
        refiner,
        middle,
        end,
        depth + 1,
        Some(departure),
        out,
        chord_error_mm,
    )
}

/// The cubic B-spline of consecutive Hermite pieces: each interior node is a
/// knot of multiplicity three.
fn hermite_curve(nodes: &[Node]) -> Option<NurbsCurve> {
    let first = nodes.first()?;
    let last = nodes.last()?;
    let mut knots = vec![first.parameter; 4];
    let mut poles = vec![first.point];
    for pair in nodes.windows(2) {
        let control = piece(pair[0], pair[1])?;
        poles.extend_from_slice(&control[1..]);
        if pair[1].parameter < last.parameter {
            knots.extend([pair[1].parameter; 3]);
        }
    }
    knots.extend([last.parameter; 4]);
    NurbsCurve::new(3, knots, NurbsPoles3::Polynomial { points: poles }, false).ok()
}

const fn analytic(surface: &SurfaceGeometry) -> bool {
    matches!(
        surface,
        SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Plane(_)
                | SolvedSurfaceGeometry::Cylinder(_)
                | SolvedSurfaceGeometry::Cone(_)
                | SolvedSurfaceGeometry::Sphere(_)
                | SolvedSurfaceGeometry::Torus(_)
        )
    )
}

/// The point of the supports' intersection nearest `point`, and the unit
/// tangent of the intersection there.
fn project(refiner: &Refiner<'_, '_>, point: Point3) -> Option<(Point3, Vector3)> {
    for support in refiner.supports {
        if let Support::Track(blend, side) = support {
            return blend.track_point(side, point);
        }
    }
    let mut current = point;
    for _ in 0..MAX_NEWTON_STEPS {
        let (a, na) = support_foot(refiner.supports[0], current)?;
        let (b, nb) = support_foot(refiner.supports[1], current)?;
        let cross = na.cross(nb);
        if cross.dot(cross) < MIN_TRANSVERSALITY {
            return None;
        }
        // The point of the tangent planes' common line nearest `current`:
        // `current + λa·na + λb·nb` with both plane equations satisfied.
        let (aa, ab, bb) = (na.dot(na), na.dot(nb), nb.dot(nb));
        let ra = na.dot(a.vector_from(current));
        let rb = nb.dot(b.vector_from(current));
        let determinant = aa * bb - ab * ab;
        let la = (ra * bb - rb * ab) / determinant;
        let lb = (rb * aa - ra * ab) / determinant;
        let next = Point3::new(
            current.x + la * na.x + lb * nb.x,
            current.y + la * na.y + lb * nb.y,
            current.z + la * na.z + lb * nb.z,
        );
        if !(next.x.is_finite() && next.y.is_finite() && next.z.is_finite()) {
            return None;
        }
        let step = next.distance(current);
        current = next;
        if step <= NEWTON_STEP_TOLERANCE_MM {
            let length = cross.norm();
            return Some((
                current,
                Vector3::new(cross.x / length, cross.y / length, cross.z / length),
            ));
        }
    }
    None
}

/// The point of `support` at or nearest `point` and its unit normal there.
fn support_foot(support: Support<'_, '_, '_>, point: Point3) -> Option<(Point3, Vector3)> {
    match support {
        Support::Analytic(surface) => super::evaluation::analytic_surface_foot(surface, point),
        Support::Pipe(blend) => blend.pipe_foot(point),
        Support::Track(..) => None,
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::geometry::analytic::PlaneSurface;
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    use super::Support;

    #[test]
    fn a_chart_where_its_supports_touch_stays_on_both() {
        // The floor z = 0 and a plane tilted 1e-5 from it about the y axis
        // touch along the y axis.
        let floor = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ));
        let tilted = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(-1.0e-5, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 1.0e-5),
            )
            .unwrap(),
        ));
        let points = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
        ];
        let chart = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 3.0, 3.0],
                NurbsPoles3::Polynomial { points },
                false,
            )
            .unwrap(),
        ));
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let refined = super::refine(
            &chart,
            [Support::Analytic(&floor), Support::Analytic(&tilted)],
        )
        .unwrap()
        .unwrap();
        assert!(refined.chord_error_mm <= super::REFINED_CHORD_TOLERANCE_MM);
        let solved = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(refined.curve));
        for s in [0.0, 0.5, 1.0, 2.0, 3.0] {
            let point = super::super::evaluation::curve_point(&ctx, &solved, s)
                .unwrap()
                .unwrap()
                .get();
            assert!(
                point.x.abs() < 1.0e-9 && point.z.abs() < 1.0e-9,
                "{point:?}"
            );
            assert!((point.y - s).abs() < 1.0e-9, "{point:?}");
        }
    }

    #[test]
    fn a_node_without_a_tangent_takes_the_arc_through_its_neighbour() {
        // On the unit circle about the origin, the arc leaving (1, 0, 0)
        // along +y reaches (0, 1, 0) heading along -x.
        let tangent = super::arc_tangent(
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Some(Vector3::new(0.0, 1.0, 0.0)),
        )
        .unwrap();
        assert!(tangent.cross(Vector3::new(-1.0, 0.0, 0.0)).norm() < 1.0e-12);
    }

    fn floor() -> SurfaceGeometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ))
    }

    fn tube() -> SurfaceGeometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                10.0,
            )
            .unwrap(),
        ))
    }

    /// Assert that `curve` lies within `tolerance` of the circle of radius 10
    /// in z = 0 at `samples` parameters, and return its two ends.
    fn assert_on_circle(curve: NurbsCurve, samples: u32, tolerance: f64) -> [Point3; 2] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let knots = curve.knots().as_slice().to_vec();
        let (low, high) = (knots[0], knots[knots.len() - 1]);
        let solved = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
        let point = |t: f64| {
            super::super::evaluation::curve_point(&ctx, &solved, t)
                .unwrap()
                .unwrap()
                .get()
        };
        for index in 0..=samples {
            let p = point(low + (high - low) * f64::from(index) / f64::from(samples));
            assert!(p.z.abs() <= tolerance, "{p:?}");
            assert!((p.x.hypot(p.y) - 10.0).abs() <= tolerance, "{p:?}");
        }
        [point(low), point(high)]
    }

    #[test]
    fn a_chart_on_a_plane_and_a_cylinder_refines_onto_their_circle() {
        // Chart points lie near the circle of radius 10 in z = 0, off it by
        // the chord sag of a coarse polyline.
        let points = (0..=8)
            .map(|k| {
                let angle = std::f64::consts::FRAC_PI_2 * f64::from(k) / 8.0;
                Point3::new(10.0 * angle.cos(), 10.0 * angle.sin(), 0.0)
            })
            .collect::<Vec<_>>();
        let mut knots = vec![0.0];
        knots.extend((0..=8).map(f64::from));
        knots.push(8.0);
        let chart = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::new(1, knots, NurbsPoles3::Polynomial { points }, false).unwrap(),
        ));
        let (plane, cylinder) = (floor(), tube());
        let refined = super::refine(
            &chart,
            [Support::Analytic(&plane), Support::Analytic(&cylinder)],
        )
        .unwrap()
        .unwrap();
        assert!(refined.chord_error_mm <= super::REFINED_CHORD_TOLERANCE_MM);
        let [start, end] = assert_on_circle(refined.curve, 64, super::REFINED_CHORD_TOLERANCE_MM);
        assert!(start.distance(Point3::new(10.0, 0.0, 0.0)) < 1.0e-9);
        assert!(end.distance(Point3::new(0.0, 10.0, 0.0)) < 1.0e-9);
    }

    /// Departure a refined curve may keep between the points at which it is
    /// checked.
    const SAMPLED_DEPARTURE_ALLOWANCE_MM: f64 = 2.0 * super::REFINED_CHORD_TOLERANCE_MM;

    #[test]
    fn a_trace_runs_along_the_intersection_between_its_ends() {
        let (plane, cylinder) = (floor(), tube());
        let supports = [Support::Analytic(&plane), Support::Analytic(&cylinder)];
        let ends = [Point3::new(10.0, 0.0, 0.0), Point3::new(0.0, 10.0, 0.0)];
        let traced = super::trace(supports, ends).unwrap().unwrap();
        assert!(traced.chord_error_mm <= super::REFINED_CHORD_TOLERANCE_MM);
        let [start, end] = assert_on_circle(traced.curve, 64, SAMPLED_DEPARTURE_ALLOWANCE_MM);
        assert!(start.distance(ends[0]) < 1.0e-9);
        assert!(end.distance(ends[1]) < 1.0e-9);
        assert!(super::trace(supports, [ends[0], ends[0]])
            .unwrap()
            .is_none());
    }
}
