// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn endpoint_clamping_final_step_has_no_blends() {
    with_test_context(|ctx| {
        let mut knots = vec![-1.0, -1.0, 0.0, 1.0, 2.0, 2.0];
        let mut points = vec![
            super::super::Homogeneous([0.0, 0.0, 0.0, 1.0]),
            super::super::Homogeneous([1.0, 1.0, 0.0, 1.0]),
            super::super::Homogeneous([2.0, 0.0, 0.0, 1.0]),
        ];
        let mut storage = ctx.reserve_scoped(0, "endpoint clamping test").unwrap();
        super::super::clamp_endpoint(ctx, &mut storage, &mut knots, &mut points, 2, 0.0, 0)
            .expect("the last insertion step has zero blends");
        assert_eq!(knots, [-1.0, -1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0]);
        assert_eq!(
            points.iter().map(|point| point.0).collect::<Vec<_>>(),
            [
                [0.0, 0.0, 0.0, 1.0],
                [0.5, 0.5, 0.0, 1.0],
                [0.5, 0.5, 0.0, 1.0],
                [1.0, 1.0, 0.0, 1.0],
                [2.0, 0.0, 0.0, 1.0],
            ]
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn degree_elevation_full_knot_break_copies_the_next_polygon() {
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(14.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("fixture admission")
    .expect("two disconnected linear spans");
    with_test_context(|ctx| {
        let elevated = super::super::elevate_to_degree(ctx, &curve, 2, 0)
            .expect("full knot multiplicity starts a new polygon");
        assert_eq!(elevated.degree(), 2);
        assert_eq!(elevated.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0]);
        assert_eq!(
            elevated.control_points().iter().map(|point| point.get()).collect::<Vec<_>>(),
            [0.0, 1.0, 2.0, 10.0, 12.0, 14.0].map(|x| Point3::new(x, 0.0, 0.0))
        );
        assert!(elevated.weights().is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
}
