// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::design::geometry as owner;

#[test]
fn numerical_audit_line_circle_intersections_are_scale_invariant() {
    for radius in [1.0e-150, 1.0e-4, 1.0, 1.0e150] {
        let circle = ProfileBoundarySegment::Arc {
            center: Point2::new(0.0, 0.0),
            radius: positive_radius(radius),
            start_angle: 0.0,
            end_angle: std::f64::consts::TAU,
        };
        assert!(owner::line_arc_intersection_points(
            (
                Point2::new(-radius, 2.0 * radius),
                Point2::new(radius, 2.0 * radius)
            ),
            &circle
        )
        .unwrap()
        .iter()
        .all(Option::is_none));
        let tangent = owner::line_arc_intersection_points(
            (Point2::new(-radius, radius), Point2::new(radius, radius)),
            &circle,
        )
        .unwrap();
        assert_eq!(
            tangent.into_iter().flatten().collect::<Vec<_>>(),
            vec![Point2::new(0.0, radius)]
        );
    }
}

#[test]
fn numerical_followup_profile_speed_bound_retains_common_weights() {
    for weight in [1.0, 1e-200, 1e200] {
        let curve = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0., 0., 1., 1.],
            vec![Point2::new(0., 0.), Point2::new(1., 0.)],
            Some(vec![weight; 2]),
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap();
        assert_eq!(
            owner::nurbs_speed_bound(&cadmpeg_test_support::service_decode_context(), &curve)
                .expect("speed bound admission"),
            Some(1.0)
        );
    }
}

#[test]
fn implicit_profile_unit_weights_match_explicit_units() {
    let points = vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)];
    let implicit = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        points.clone(),
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .expect("polynomial pcurve");
    let explicit = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        points,
        Some(vec![1.0, 1.0]),
        false,
    )
    .expect("fixture pcurve construction admission")
    .expect("unit-weight pcurve");
    assert_eq!(
        owner::nurbs_speed_bound(&cadmpeg_test_support::service_decode_context(), &implicit)
            .expect("speed bound admission"),
        owner::nurbs_speed_bound(&cadmpeg_test_support::service_decode_context(), &explicit)
            .expect("speed bound admission")
    );
}

#[test]
fn numerical_followup_boolean_line_arc_matches_point_intersections() {
    let radius = 1e-4;
    let arc = ProfileBoundarySegment::Arc {
        center: Point2::new(0., 0.),
        radius: positive_radius(radius),
        start_angle: 0.,
        end_angle: std::f64::consts::TAU,
    };
    let line = ProfileBoundarySegment::Line {
        start: Point2::new(-radius, 2. * radius),
        end: Point2::new(radius, 2. * radius),
    };
    assert!(!owner::boundary_segments_intersect(&line, &arc));
    let point = ProfileBoundarySegment::Line {
        start: Point2::new(radius, 0.),
        end: Point2::new(radius, 0.),
    };
    assert!(owner::boundary_segments_intersect(&point, &arc));
}

#[test]
fn scaled_planar_intersections_preserve_separation_and_witnesses() {
    for r in [1e-200, 1e-100, 1., 1e200] {
        let a = Point2::new(0., 0.);
        let b = Point2::new(r, 0.);
        assert!(!owner::segments_intersect(
            (a, b),
            (Point2::new(0., 2. * r), Point2::new(r, 2. * r))
        ));
        assert!(owner::segments_intersect(
            (a, Point2::new(r, r)),
            (Point2::new(0., r), b)
        ));
        assert_eq!(owner::point_distance(a, b), r);
        assert_eq!(
            owner::point_segment_distance(Point2::new(0.5 * r, r), (a, b)),
            r
        );
        let arc = |x| ProfileBoundarySegment::Arc {
            center: Point2::new(x, 0.),
            radius: positive_radius(r),
            start_angle: 0.,
            end_angle: std::f64::consts::TAU,
        };
        let points = owner::arc_intersection_points(&arc(0.), &arc(r)).unwrap();
        assert_eq!(points.iter().flatten().count(), 2);
        assert!(owner::boundary_segments_intersect(&arc(0.), &arc(r)));
        for point in points.into_iter().flatten() {
            assert!((point.u / r - 0.5).abs() <= 4. * f64::EPSILON);
        }
    }
}

#[test]
fn disparate_segment_and_point_scales_preserve_distance() {
    let segment = (Point2::new(0., 0.), Point2::new(1e-200, 0.));
    assert_eq!(
        owner::point_segment_distance(Point2::new(0., 1e200), segment),
        1e200
    );
    let arc = ProfileBoundarySegment::Arc {
        center: Point2::new(0., 0.),
        radius: positive_radius(1e-6),
        start_angle: 0.,
        end_angle: std::f64::consts::TAU,
    };
    assert!(owner::line_arc_intersection_points(
        (Point2::new(-1e200, 1.), Point2::new(1e200, 1.)),
        &arc
    )
    .unwrap()
    .iter()
    .all(Option::is_none));
}

#[test]
fn numerical_0922b_line_arc_crossings() {
    let arc = ProfileBoundarySegment::Arc {
        center: Point2::new(0., 0.),
        radius: positive_radius(0.001),
        start_angle: 0.,
        end_angle: std::f64::consts::TAU,
    };
    for half in [0.001, 1., 1e4] {
        let points = owner::line_arc_intersection_points(
            (Point2::new(-half, 0.), Point2::new(half, 0.)),
            &arc,
        )
        .unwrap();
        println!("Fusion line[-{half},{half}],r=.001 => {points:?}");
        assert_eq!(
            points.into_iter().flatten().collect::<Vec<_>>(),
            vec![Point2::new(-0.001, 0.), Point2::new(0.001, 0.)]
        );
    }
}

#[test]
fn numerical_0922b_large_line_crossing() {
    for s in [1., 1e200] {
        let a = ProfileBoundarySegment::Line {
            start: Point2::new(-s, 0.),
            end: Point2::new(s, 0.),
        };
        let b = ProfileBoundarySegment::Line {
            start: Point2::new(0., -s),
            end: Point2::new(0., s),
        };
        let r = analytic_segment_intersections(&a, &b).unwrap();
        println!("Fusion crossing scale{s:e}: {r:?}");
        assert_eq!(
            r.into_iter().flatten().collect::<Vec<_>>(),
            vec![Point2::new(0., 0.)]
        );
    }
}

#[test]
fn numerical_0922b_large_line_split() {
    for scale in [1., 1e200] {
        let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0., 0.),
            end: Point2::new(scale, 0.),
        })
        .unwrap();
        let r = crate::test_support::with_decode_context(|decode_ctx| {
            owner::arrangement_split_parameters(
                &line,
                [0., 1.],
                &[Point2::new(0.5 * scale, 0.)],
                LARGE_LINE_TOLERANCE,
                decode_ctx,
            )
        })
        .unwrap()
        .unwrap();
        println!("Fusion split scale{scale:e}: {r:?}");
        assert_eq!(r, vec![0., 0.5, 1.]);
    }
}

#[test]
fn numerical_0922b_wide_segment_incidence() {
    for scale in [1., 1e308] {
        let r = owner::point_segment_distance(
            Point2::new(0., 0.),
            (Point2::new(-scale, 0.), Point2::new(scale, 0.)),
        );
        println!("Fusion segment[-{scale:e},{scale:e}],origin distance{r:e}");
        assert_eq!(r, 0.);
    }
}
