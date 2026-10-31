// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3, NurbsSurface};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use super::{solve, BlendCache, BlendInputs};
use crate::brep::evaluation::nurbs_surface_point;
use cadmpeg_core::CodecError;

const FIT_TOLERANCE: f64 = 1.0e-6;
const STEP: f64 = 1.0e-4;

/// The cache when every face vertex lies on it.
fn solve_holding(
    ctx: &DecodeContext<'_>,
    inputs: &BlendInputs<'_>,
) -> Result<Option<BlendCache>, CodecError> {
    Ok(solve(ctx, inputs)?
        .filter(|solved| solved.holds)
        .map(|solved| solved.cache))
}

fn plane(normal: Vector3, u_axis: Vector3) -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0), normal, u_axis).unwrap(),
    )
}

fn point(ctx: &DecodeContext<'_>, surface: &NurbsSurface, u: f64, v: f64) -> Point3 {
    nurbs_surface_point(ctx, surface, u, v)
        .unwrap()
        .unwrap()
        .get()
}

/// The natural normal of the cache by central differences.
fn normal(ctx: &DecodeContext<'_>, surface: &NurbsSurface, u: f64, v: f64) -> [f64; 3] {
    let difference =
        |left: Point3, right: Point3| [right.x - left.x, right.y - left.y, right.z - left.z];
    let along = difference(
        point(ctx, surface, u - STEP, v),
        point(ctx, surface, u + STEP, v),
    );
    let across = difference(
        point(ctx, surface, u, v - STEP),
        point(ctx, surface, u, v + STEP),
    );
    super::cross(along, across)
}

fn u_range(surface: &NurbsSurface) -> [f64; 2] {
    let knots = surface.u_knots().as_slice();
    [knots[0], knots[knots.len() - 1]]
}

#[test]
fn fillet_between_two_planes_is_a_quarter_cylinder_facing_its_spine() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let floor = plane(Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0));
    let wall = plane(Vector3::new(0.0, 1.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
    let spine = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 2.0, 2.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    ));
    let boundary = [
        Point3::new(0.0, 2.0, 0.0),
        Point3::new(10.0, 2.0, 0.0),
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(10.0, 0.0, 2.0),
    ];
    let cache = solve_holding(
        &ctx,
        &BlendInputs {
            spine: &spine,
            supports: [&floor, &wall],
            radius: 2.0,
            boundary: &boundary,
            vertices: &boundary,
            tolerance: FIT_TOLERANCE,
        },
    )
    .unwrap()
    .expect("a quarter cylinder");
    assert!(cache.fit_tolerance <= FIT_TOLERANCE);
    let [low, high] = u_range(&cache.surface);
    // The face spans x from 0 to 10; the cache covers it with a 5% margin.
    assert!((low + 0.5).abs() <= FIT_TOLERANCE && (high - 10.5).abs() <= FIT_TOLERANCE);
    for u in [0.5, 3.0, 7.5] {
        for v in [0.1, 0.5, 0.9] {
            let at = point(&ctx, &cache.surface, u, v);
            assert!(((at.y - 2.0).hypot(at.z - 2.0) - 2.0).abs() <= FIT_TOLERANCE);
            let outward = [0.0, at.y - 2.0, at.z - 2.0];
            assert!(super::dot(normal(&ctx, &cache.surface, u, v), outward) > 0.0);
        }
    }
}

#[test]
fn ring_around_a_boss_covers_the_whole_spine_as_a_torus() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let floor = plane(Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0));
    let boss = SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            10.0,
        )
        .unwrap(),
    );
    let spine = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(0.0, 0.0, 2.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            12.0,
        )
        .unwrap(),
    ));
    let mut boundary = Vec::new();
    for step in 0..36 {
        let angle = f64::from(step) * std::f64::consts::TAU / 36.0;
        boundary.push(Point3::new(12.0 * angle.cos(), 12.0 * angle.sin(), 0.0));
        boundary.push(Point3::new(10.0 * angle.cos(), 10.0 * angle.sin(), 2.0));
    }
    let cache = solve_holding(
        &ctx,
        &BlendInputs {
            spine: &spine,
            supports: [&floor, &boss],
            radius: 2.0,
            boundary: &boundary,
            vertices: &[],
            tolerance: FIT_TOLERANCE,
        },
    )
    .unwrap()
    .expect("a torus");
    assert!(cache.fit_tolerance <= FIT_TOLERANCE);
    let [low, high] = u_range(&cache.surface);
    assert!((high - low - std::f64::consts::TAU).abs() <= FIT_TOLERANCE);
    for fraction in [0.05, 0.3, 0.55, 0.8] {
        let u = low + fraction * (high - low);
        for v in [0.1, 0.5, 0.9] {
            let at = point(&ctx, &cache.surface, u, v);
            let ring = at.x.hypot(at.y);
            assert!(((ring - 12.0).hypot(at.z - 2.0) - 2.0).abs() <= 2.0 * FIT_TOLERANCE);
            let outward = [
                at.x - 12.0 * at.x / ring,
                at.y - 12.0 * at.y / ring,
                at.z - 2.0,
            ];
            assert!(super::dot(normal(&ctx, &cache.surface, u, v), outward) > 0.0);
        }
    }
}

#[test]
fn chart_spine_is_moved_onto_the_ball_centres() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let floor = plane(Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0));
    let boss = SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            10.0,
        )
        .unwrap(),
    );
    // A closed 24-chord polyline through the centre circle stands in for a
    // stored intersection chart: its chords stray up to 0.2 mm inside it.
    let chords = 24_u32;
    let points = (0..=chords)
        .map(|step| {
            let angle = f64::from(step) * std::f64::consts::TAU / f64::from(chords);
            Point3::new(12.0 * angle.cos(), 12.0 * angle.sin(), 2.0)
        })
        .collect::<Vec<_>>();
    let mut knots = vec![0.0];
    knots.extend((0..=chords).map(f64::from));
    knots.push(f64::from(chords));
    let spine = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::new(1, knots, NurbsPoles3::Polynomial { points }, false).unwrap(),
    ));
    let mut boundary = Vec::new();
    for step in 0..36 {
        let angle = f64::from(step) * std::f64::consts::TAU / 36.0;
        boundary.push(Point3::new(12.0 * angle.cos(), 12.0 * angle.sin(), 0.0));
        boundary.push(Point3::new(10.0 * angle.cos(), 10.0 * angle.sin(), 2.0));
    }
    let cache = solve_holding(
        &ctx,
        &BlendInputs {
            spine: &spine,
            supports: [&floor, &boss],
            radius: 2.0,
            boundary: &boundary,
            vertices: &[],
            tolerance: FIT_TOLERANCE,
        },
    )
    .unwrap()
    .expect("a torus through the corrected centres");
    assert!(cache.fit_tolerance <= 2.0 * FIT_TOLERANCE);
    let [low, high] = u_range(&cache.surface);
    for fraction in [0.02, 0.21, 0.5, 0.77] {
        let u = low + fraction * (high - low);
        for v in [0.0, 0.3, 0.7, 1.0] {
            let at = point(&ctx, &cache.surface, u, v);
            let ring = at.x.hypot(at.y);
            assert!(((ring - 12.0).hypot(at.z - 2.0) - 2.0).abs() <= 2.0 * FIT_TOLERANCE);
        }
    }
}

#[test]
fn supports_the_ball_cannot_touch_together_leave_no_cache() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let floor = plane(Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0));
    let ceiling = SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 10.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    );
    let spine = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 5.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    ));
    let boundary = [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 10.0)];
    let cache = solve_holding(
        &ctx,
        &BlendInputs {
            spine: &spine,
            supports: [&floor, &ceiling],
            radius: 2.0,
            boundary: &boundary,
            vertices: &[],
            tolerance: FIT_TOLERANCE,
        },
    )
    .unwrap();
    assert!(cache.is_none());
}

#[test]
fn cache_that_misses_a_face_vertex_is_refused() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let floor = plane(Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0));
    let wall = plane(Vector3::new(0.0, 1.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
    let spine = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 2.0, 2.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    ));
    let boundary = [
        Point3::new(0.0, 2.0, 0.0),
        Point3::new(10.0, 2.0, 0.0),
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(10.0, 0.0, 2.0),
    ];
    // A vertex 0.5 mm inside the quarter cylinder, where no point of the
    // blend lies.
    let mut vertices = boundary.to_vec();
    vertices.push(Point3::new(5.0, 2.0, 0.5));
    let cache = solve_holding(
        &ctx,
        &BlendInputs {
            spine: &spine,
            supports: [&floor, &wall],
            radius: 2.0,
            boundary: &boundary,
            vertices: &vertices,
            tolerance: FIT_TOLERANCE,
        },
    )
    .unwrap();
    assert!(cache.is_none());
}

#[test]
fn a_vertex_miss_is_the_largest_vertex_distance_from_the_cache() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let floor = plane(Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0));
    let wall = plane(Vector3::new(0.0, 1.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
    let spine = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 2.0, 2.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    ));
    let boundary = [
        Point3::new(0.0, 2.0, 0.0),
        Point3::new(10.0, 2.0, 0.0),
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(10.0, 0.0, 2.0),
    ];
    let cache = solve_holding(
        &ctx,
        &BlendInputs {
            spine: &spine,
            supports: [&floor, &wall],
            radius: 2.0,
            boundary: &boundary,
            vertices: &boundary,
            tolerance: FIT_TOLERANCE,
        },
    )
    .unwrap()
    .expect("a quarter cylinder");
    let miss = |vertices: &[Point3]| {
        super::vertex_miss(&ctx, &cache, &spine, vertices)
            .unwrap()
            .unwrap()
    };
    assert!(miss(&boundary) <= FIT_TOLERANCE);
    // A point 0.01 mm outside the quarter cylinder, half way round it.
    let out = 2.0 + 0.01;
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let mut vertices = boundary.to_vec();
    vertices.push(Point3::new(5.0, 2.0 - out * half, 2.0 - out * half));
    assert!((miss(&vertices) - 0.01).abs() <= 10.0 * FIT_TOLERANCE);
}
