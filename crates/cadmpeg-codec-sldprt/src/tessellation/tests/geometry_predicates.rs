// SPDX-License-Identifier: Apache-2.0

use super::super::{
    chordal_hole_constraint, circular_outer_and_holes, is_simple_polygon, planar_arc_segments,
    polygon_contains, triangulate_polygon, CircularHole, PlanarHole, EPS_DISPLAY_QUANTIZATION,
    MAX_PLANAR_TRIM_ARC_SEGMENTS,
};
use cadmpeg_ir::math::planar::segments_intersect;
use cadmpeg_ir::math::Point2;
const EPS_DISTANCE: f64 = 1e-7;
#[test]
fn numerical_audit_tessellation_keeps_small_crossings_and_triangles() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    for d in [1., 1e-4] {
        assert!(segments_intersect(
            Point2::new(-d, 0.),
            Point2::new(d, 0.),
            Point2::new(0., -d),
            Point2::new(0., d),
            EPS_DISTANCE
        ));
        let p = [Point2::new(0., 0.), Point2::new(d, 0.), Point2::new(0., d)];
        assert!(is_simple_polygon(&p, EPS_DISTANCE));
        assert_eq!(
            triangulate_polygon(&ctx, &p, EPS_DISTANCE).unwrap(),
            Some(vec![p])
        );
        let square = [
            Point2::new(0., 0.),
            Point2::new(d, 0.),
            Point2::new(d, d),
            Point2::new(0., d),
        ];
        assert_eq!(
            triangulate_polygon(&ctx, &square, EPS_DISTANCE)
                .unwrap()
                .unwrap()
                .len(),
            2
        );
    }
}
#[test]
fn numerical_audit_tessellation_keeps_translated_area() {
    for offset in [0., 1e8] {
        let p = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
            .map(|p| Point2::new(offset + p[0], offset + p[1]));
        assert_eq!(
            cadmpeg_ir::math::planar::polygon_area_twice(&p)
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            Some(2.)
        );
        assert!(is_simple_polygon(&p, EPS_DISTANCE));
    }
}

#[test]
fn planar_trim_accepts_concave_simple_loops_and_rejects_crossings() {
    const CONTAINMENT_TOLERANCE: f64 = 1.0e-9;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    let concave = vec![
        Point2::new(0.0, 0.0),
        Point2::new(4.0, 0.0),
        Point2::new(4.0, 4.0),
        Point2::new(2.0, 4.0),
        Point2::new(2.0, 2.0),
        Point2::new(0.0, 2.0),
    ];
    assert!(is_simple_polygon(&concave, CONTAINMENT_TOLERANCE));
    assert!(
        PlanarHole::polygon(&ctx, concave.clone(), CONTAINMENT_TOLERANCE)
            .unwrap()
            .is_some()
    );
    assert!(polygon_contains(
        &ctx,
        &concave,
        Point2::new(1.0, 1.0),
        CONTAINMENT_TOLERANCE
    ).unwrap());
    assert!(polygon_contains(
        &ctx,
        &concave,
        Point2::new(3.0, 3.0),
        CONTAINMENT_TOLERANCE
    ).unwrap());
    assert!(!polygon_contains(
        &ctx,
        &concave,
        Point2::new(1.0, 3.0),
        CONTAINMENT_TOLERANCE
    ).unwrap());

    let crossing = vec![
        Point2::new(0.0, 0.0),
        Point2::new(4.0, 4.0),
        Point2::new(0.0, 4.0),
        Point2::new(4.0, 0.0),
    ];
    assert!(!is_simple_polygon(&crossing, CONTAINMENT_TOLERANCE));
}

const EPS_FOLLOWUP_ARC_SAGITTA: f64 = 1e-9;

#[test]
fn numerical_followup_arc_error_retains_the_sagitta_at_the_segment_cap() {
    let (segments, error) =
        planar_arc_segments(1e-5, 1e12, EPS_FOLLOWUP_ARC_SAGITTA).expect("segment count is exact");
    assert_eq!(segments, MAX_PLANAR_TRIM_ARC_SEGMENTS);
    let expected = 2e12
        * (1e-5
            / (4.0
                * cadmpeg_core::convert::f64_from_index(segments)
                    .expect("segment count is exact")))
        .sin()
        .powi(2);
    assert!(error > EPS_FOLLOWUP_ARC_SAGITTA);
    assert!((error / expected - 1.0).abs() <= 4.0 * f64::EPSILON);
}

#[test]
fn chordal_hole_constraint_uses_the_boundary_sampling_sagitta() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    let hole = CircularHole {
        center: Point2::new(0.0, 0.0),
        radius: 1.0,
    };
    let boundary = (0..6)
        .map(|index| {
            let angle = f64::from(index) * std::f64::consts::TAU / 6.0;
            Point2::new(angle.cos(), angle.sin())
        })
        .collect::<Vec<_>>();
    let mut chordal = boundary.clone();
    let angle = std::f64::consts::PI / 6.0;
    chordal.push(Point2::new(0.9 * angle.cos(), 0.9 * angle.sin()));
    let (exclusion, boundary_circle) =
        chordal_hole_constraint(&ctx, hole, &chordal, EPS_DISPLAY_QUANTIZATION)
            .unwrap()
            .unwrap();
    assert_eq!(boundary_circle.radius, hole.radius);
    assert!(exclusion.radius < hole.radius);
    assert!(exclusion.radius > 0.8);

    let mut deep = boundary;
    deep.push(Point2::new(0.7 * angle.cos(), 0.7 * angle.sin()));
    assert!(
        chordal_hole_constraint(&ctx, hole, &deep, EPS_DISPLAY_QUANTIZATION)
            .unwrap()
            .is_none()
    );

    let interior = vec![Point2::new(0.5, 0.0), Point2::new(0.0, 0.5)];
    assert!(
        chordal_hole_constraint(&ctx, hole, &interior, EPS_DISPLAY_QUANTIZATION)
            .unwrap()
            .is_none()
    );
}

#[test]
fn circular_planar_bounds_choose_one_enclosing_outer() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    let circles = vec![
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 10.0,
        },
        CircularHole {
            center: Point2::new(6.0, 0.0),
            radius: 2.0,
        },
        CircularHole {
            center: Point2::new(-6.0, 0.0),
            radius: 2.0,
        },
    ];
    let (outer, holes) = circular_outer_and_holes(&ctx, &circles, EPS_DISPLAY_QUANTIZATION)
        .unwrap()
        .unwrap();
    assert_eq!(outer.radius, 10.0);
    assert_eq!(holes.len(), 2);

    let ambiguous = vec![
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 10.0,
        },
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 10.0,
        },
    ];
    assert!(
        circular_outer_and_holes(&ctx, &ambiguous, EPS_DISPLAY_QUANTIZATION)
            .unwrap()
            .is_none()
    );
}
