use super::{departure, refine};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

const EPS_ON_SURFACE_MM: f64 = 1.0e-7;
const TARGET_MM: f64 = 5.0e-6;
const REACH_MM: f64 = 1.0;
const RADIUS_MM: f64 = 10.0;

fn plane(height: f64) -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, height),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane"),
    )
}

fn cylinder() -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            RADIUS_MM,
        )
        .expect("cylinder"),
    )
}

/// A chart polygon of three quarters of the circle section. Interior
/// points lie `offset` outside the cylinder and above the plane, end
/// points `end_offset`.
fn chart(count: u32, offset: f64, end_offset: f64) -> NurbsCurve {
    let mut points = Vec::new();
    for step in 0..=count {
        let angle = 0.75 * std::f64::consts::TAU * f64::from(step) / f64::from(count);
        let off = if step == 0 || step == count {
            end_offset
        } else {
            offset
        };
        let radius = RADIUS_MM + off;
        points.push(Point3::new(radius * angle.cos(), radius * angle.sin(), off));
    }
    let mut knots = vec![0.0];
    knots.extend((0..=count).map(f64::from));
    knots.push(f64::from(count));
    NurbsCurve::from_lanes(1, knots, points, None, false).expect("chart")
}

fn context(arena: &DecodeArena) -> DecodeContext<'_> {
    let policy = DecodePolicy::default();
    DecodeContext::from_root_bytes(&[0; 4], arena, &policy)
        .expect("context")
        .0
}

#[test]
fn coarse_chart_is_refined_onto_both_supports() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let (plane, cylinder) = (plane(0.0), cylinder());
    let coarse = chart(8, 1.0e-3, 0.0);
    let traced = refine(
        &ctx,
        [&plane, &cylinder],
        &coarse,
        None,
        TARGET_MM,
        REACH_MM,
    )
    .expect("evaluation")
    .expect("refined");
    assert!(traced.curve.control_points().len() > 100);
    assert!(traced.departure <= TARGET_MM);
    for point in traced.curve.control_points() {
        let point = point.get();
        assert!(point.z.abs() < EPS_ON_SURFACE_MM);
        assert!((point.x.hypot(point.y) - RADIUS_MM).abs() < EPS_ON_SURFACE_MM);
    }
    let measured = departure(&ctx, [&plane, &cylinder], &traced.curve)
        .expect("evaluation")
        .expect("departure");
    assert!(measured <= TARGET_MM);
    let before = departure(&ctx, [&plane, &cylinder], &coarse)
        .expect("evaluation")
        .expect("departure");
    assert!(before > 100.0 * TARGET_MM);
    ctx.finish_session().expect("session");
}

#[test]
fn end_points_keep_their_positions_and_state_their_distance() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let (plane, cylinder) = (plane(0.0), cylinder());
    let coarse = chart(8, 1.0e-3, 2.0e-3);
    let traced = refine(
        &ctx,
        [&plane, &cylinder],
        &coarse,
        None,
        TARGET_MM,
        REACH_MM,
    )
    .expect("evaluation")
    .expect("refined");
    let before = coarse.control_points();
    let after = traced.curve.control_points();
    assert_eq!(after.first(), before.first());
    assert_eq!(after.last(), before.last());
    let interior = &after[1..after.len() - 1];
    assert!(interior.len() > 100);
    assert!(interior
        .iter()
        .all(|point| point.get().z.abs() < EPS_ON_SURFACE_MM));
    // An end point 2e-3 above the plane and outside the cylinder.
    assert!(traced.departure >= 2.0e-3);
    assert!(traced.departure <= 3.0e-3);
    ctx.finish_session().expect("session");
}

#[test]
fn chart_stopping_short_of_its_vertices_continues_to_them() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let (plane, cylinder) = (plane(0.0), cylinder());
    let coarse = chart(8, 0.0, 0.0);
    let arc = 0.75 * std::f64::consts::TAU;
    let at = |angle: f64| Point3::new(RADIUS_MM * angle.cos(), RADIUS_MM * angle.sin(), 0.0);
    let (first, second) = (at(-0.01), at(arc + 0.01));
    // The vertices arrive in the order opposite to the chart.
    let traced = refine(
        &ctx,
        [&plane, &cylinder],
        &coarse,
        Some([second, first]),
        TARGET_MM,
        REACH_MM,
    )
    .expect("evaluation")
    .expect("refined");
    let points = traced.curve.control_points();
    assert_eq!(points.first().map(|point| point.get()), Some(first));
    assert_eq!(points.last().map(|point| point.get()), Some(second));
    assert!(traced.range[0] < 0.0);
    assert!(traced.range[1] > 8.0);
    assert!(traced.departure <= TARGET_MM);
    ctx.finish_session().expect("session");
}

#[test]
fn vertex_inside_the_chart_becomes_a_point_of_the_curve() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let (plane, cylinder) = (plane(0.0), cylinder());
    let coarse = chart(8, 0.0, 0.0);
    let angle = 0.75 * std::f64::consts::TAU * 2.4 / 8.0;
    let vertex = Point3::new(RADIUS_MM * angle.cos(), RADIUS_MM * angle.sin(), 0.0);
    let traced = refine(
        &ctx,
        [&plane, &cylinder],
        &coarse,
        Some([vertex, vertex]),
        TARGET_MM,
        REACH_MM,
    )
    .expect("evaluation")
    .expect("refined");
    assert!(traced
        .curve
        .control_points()
        .iter()
        .any(|point| point.get().distance(vertex) < EPS_ON_SURFACE_MM));
    assert!((traced.range[0]).abs() < EPS_ON_SURFACE_MM);
    assert!((traced.range[1] - 8.0).abs() < EPS_ON_SURFACE_MM);
    assert!(traced.departure <= TARGET_MM);
    ctx.finish_session().expect("session");
}

#[test]
fn chart_beyond_reach_of_a_support_is_not_refined() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let (plane, cylinder) = (plane(0.0), cylinder());
    let far = chart(8, 2.0, 2.0);
    assert!(
        refine(&ctx, [&plane, &cylinder], &far, None, TARGET_MM, 0.5)
            .expect("evaluation")
            .is_none()
    );
    ctx.finish_session().expect("session");
}

#[test]
fn parallel_supports_do_not_trace() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let (lower, upper) = (plane(0.0), plane(1.0));
    let coarse = chart(8, 0.0, 0.0);
    assert!(
        refine(&ctx, [&lower, &upper], &coarse, None, TARGET_MM, REACH_MM)
            .expect("evaluation")
            .is_none()
    );
    ctx.finish_session().expect("session");
}
