// SPDX-License-Identifier: Apache-2.0
//! boundary cases tests.

use std::collections::HashMap;

use super::{
    e5_boundary_curve, e5_circle_carriers_have_same_ordered_sweep, e5_loop_members,
    e5_occurrence_intersection_context, equivalent_e5_curve_carriers, finite_lane, finite_pair,
    fixture_pcurve_on_surface, jet_pcurve, plan_boundary_with_points, positive, quintic_jet_pcurve,
    rational_pcurve_arc, BTreeMap, CurveGeometry, E5BoundEntry, E5Bounds, E5CurveSupport,
    E5CurveSupportKind, E5Edge, E5Face, E5Loop, E5OrientedMember, E5Pcurve, E5Surface, E5Topology,
    NurbsSurface, PcurveGeometry, PcurveNurbs, Point2, Point3, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry, SurfaceId, Vector3, EPS_E5_DECODE_EXACT_GEOMETRY,
};

#[test]
fn e5_boundary_line_rejects_overflowing_uv_start() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let native = E5Pcurve::Line {
        surface: 0,
        origin: finite_pair([f64::MAX, 0.0]),
        direction: finite_pair([f64::MAX, 0.0]),
        range: finite_pair([1.0, 2.0]),
    };
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(f64::MAX, 0.0),
            Point2::new(f64::MAX, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    assert!(
        crate::test_support::with_service_context(|ctx| e5_boundary_curve(
            ctx,
            &surface,
            &native,
            &pcurve,
            (
                [1.0, 2.0],
                [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
            ),
            finite_pair([1.0, 1.0]),
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service profile admits E5 boundary curve")
        .is_none()
    );
}

#[test]
fn e5_boundary_line_rejects_overflowing_uv_end() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("finite plane"),
    ));
    let native = E5Pcurve::Line {
        surface: 0,
        origin: finite_pair([f64::MAX, 0.0]),
        direction: finite_pair([f64::MAX * 0.5, 0.0]),
        range: finite_pair([0.0, 1.0]),
    };
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(f64::MAX, 0.0),
            Point2::new(f64::MAX * 0.5, 0.0),
        )
        .expect("finite line pcurve"),
    );
    assert!(
        crate::test_support::with_service_context(|ctx| e5_boundary_curve(
            ctx,
            &surface,
            &native,
            &pcurve,
            (
                [0.0, 1.0],
                [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
            ),
            finite_pair([1.0, 1.0]),
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service profile admits E5 boundary curve")
        .is_none()
    );
}

#[test]
fn e5_plane_boundary_line_keeps_finite_wide_parameter_endpoints() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("finite plane"),
    ));
    let direction = 1.0e-7;
    let range = [-f64::MAX, f64::MAX];
    let native = E5Pcurve::Line {
        surface: 0,
        origin: finite_pair([0.0, 0.0]),
        direction: finite_pair([direction, 0.0]),
        range: finite_pair(range),
    };
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(direction, 0.0),
        )
        .expect("finite line pcurve"),
    );
    let bound = f64::MAX * direction;
    let (curve, curve_range) = crate::test_support::with_service_context(|ctx| {
        e5_boundary_curve(
            ctx,
            &surface,
            &native,
            &pcurve,
            (
                range,
                [Point3::new(-bound, 0.0, 0.0), Point3::new(bound, 0.0, 0.0)],
            ),
            finite_pair([1.0, 1.0]),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service profile admits E5 boundary curve")
    .expect("finite line carrier across a wide parameter range");
    assert!(matches!(
        curve,
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
    ));
    assert_eq!(curve_range, [0.0, 2.0 * bound]);
}

#[test]
fn e5_boundary_line_normalizes_subnormal_chord() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let tiny = f64::from_bits(1);
    let native = E5Pcurve::Line {
        surface: 0,
        origin: finite_pair([0.0, 0.0]),
        direction: finite_pair([tiny, 0.0]),
        range: finite_pair([0.0, 1.0]),
    };
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid LinePcurve fixture"),
    );
    let (curve, range) = crate::test_support::with_service_context(|ctx| {
        e5_boundary_curve(
            ctx,
            &surface,
            &native,
            &pcurve,
            (
                [0.0, tiny],
                [Point3::new(0.0, 0.0, 0.0), Point3::new(tiny, 0.0, 0.0)],
            ),
            finite_pair([1.0, 1.0]),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service profile admits E5 boundary curve")
    .expect("subnormal line chord");
    assert_eq!(range, [0.0, tiny]);
    assert!(
        matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve))
        if {
            let direction = *line_curve.direction().as_raw();
            direction == Vector3::new(1.0, 0.0, 0.0)
        })
    );
}

#[test]
fn e5_plane_circle_boundary_lifts_to_world_circle_carrier() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let native = crate::families::e5::graph::E5Pcurve::Circle {
        surface: 0,
        center: finite_pair([4.0, 5.0]),
        codes: [0, 0],
        radius: positive(2.0),
        range: finite_pair([0.0, std::f64::consts::PI]),
        tail: finite_pair([0.0, 0.0]),
    };
    let pcurve = rational_pcurve_arc(
        [4.0, 5.0],
        2.0,
        [0.0, std::f64::consts::FRAC_PI_2],
        &mut crate::nurbs::LaneRefusals::new(),
        "test record",
    )
    .expect("plane pcurve");
    let (curve, range) = crate::test_support::with_service_context(|ctx| {
        e5_boundary_curve(
            ctx,
            &surface,
            &native,
            &pcurve,
            (
                [0.0, std::f64::consts::FRAC_PI_2],
                [Point3::new(7.0, 7.0, 3.0), Point3::new(5.0, 9.0, 3.0)],
            ),
            finite_pair([1.0, 1.0]),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service profile admits E5 boundary curve")
    .expect("plane boundary circle");
    assert_eq!(range, [0.0, std::f64::consts::FRAC_PI_2]);
    assert!(
        matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))
                if {
                    let center = circle_curve.center().get();
        let axis = circle_curve.frame().axis().as_raw();
        let ref_direction = circle_curve.frame().reference().as_raw();
        let radius = circle_curve.radius().get();
                    center == Point3::new(5.0, 7.0, 3.0)
                        && *axis == Vector3::new(0.0, 0.0, 1.0)
                        && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
                        && radius == 2.0
                })
    );

    let (curve, range) = crate::test_support::with_service_context(|ctx| {
        e5_boundary_curve(
            ctx,
            &surface,
            &native,
            &pcurve,
            (
                [0.0, std::f64::consts::FRAC_PI_2],
                [Point3::new(-5.0, -3.0, 3.0), Point3::new(-3.0, -5.0, 3.0)],
            ),
            finite_pair([-1.0, -1.0]),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service profile admits E5 boundary curve")
    .expect("reflected plane boundary circle");
    assert_eq!(range, [0.0, std::f64::consts::FRAC_PI_2]);
    assert!(
        matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))
                if {
                    let center = circle_curve.center().get();
        let axis = circle_curve.frame().axis().as_raw();
        let ref_direction = circle_curve.frame().reference().as_raw();
        let radius = circle_curve.radius().get();
                    center == Point3::new(-3.0, -3.0, 3.0)
                        && *axis == Vector3::new(0.0, 0.0, 1.0)
                        && *ref_direction == Vector3::new(-1.0, 0.0, 0.0)
                        && radius == 2.0
                })
    );
}

#[test]
fn e5_boundary_nurbs_lift_refuses_low_collection_and_retained_limits() {
    let plane = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
        Point3::new(1.0, 2.0, 3.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("valid plane fixture");
    let nurbs = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 2.0)],
        None,
        false,
    )
    .expect("fixture pcurve construction admission")
    .expect("valid pcurve fixture");
    let retained = crate::test_support::with_retained_limit(0, |ctx| {
        super::super::e5_lift_plane_nurbs(
            ctx,
            &plane,
            &nurbs,
            17,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(
        matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_e5_boundary_lifted_poles")
    );
    let collection = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::e5_lift_plane_nurbs(
            ctx,
            &plane,
            &nurbs,
            17,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(
        matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_e5_boundary_lifted_poles")
    );
    let lifted = crate::test_support::with_service_context(|ctx| {
        super::super::e5_lift_plane_nurbs(
            ctx,
            &plane,
            &nurbs,
            17,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service profile admits lifted pcurve")
    .expect("valid lifted NURBS");
    assert_eq!(lifted.pole_count(), 2);

    let rational = PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 2.0)],
        Some(vec![1.0, 2.0]),
        false,
    )
    .expect("fixture pcurve construction admission")
    .expect("valid rational pcurve fixture");
    let retained = crate::test_support::with_retained_limit(0, |ctx| {
        super::super::e5_lift_plane_nurbs(
            ctx,
            &plane,
            &rational,
            18,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(
        matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_e5_boundary_lifted_poles")
    );
    let collection = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::e5_lift_plane_nurbs(
            ctx,
            &plane,
            &rational,
            18,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(
        matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_e5_boundary_lifted_poles")
    );
    let lifted = crate::test_support::with_service_context(|ctx| {
        super::super::e5_lift_plane_nurbs(
            ctx,
            &plane,
            &rational,
            18,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service profile admits rational lift")
    .expect("valid rational NURBS");
    assert_eq!(lifted.pole_rows().weights(), Some(vec![1.0, 2.0]));
}

#[test]
fn e5_plane_jet_boundary_lifts_control_net_affinely() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits input limit");
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let points = vec![[0.0, 0.0], [1.0, 2.0]];
    let first = vec![[1.0, 2.0], [1.0, 2.0]];
    let second = vec![[0.0, 0.0], [0.0, 0.0]];
    let native = jet_pcurve(
        0,
        vec![0.0, 1.0],
        vec![6, 6],
        points.clone(),
        first.clone(),
        second.clone(),
        [0.0, 1.0],
    );
    let pcurve = quintic_jet_pcurve(
        &ctx,
        5,
        &[0.0, 1.0],
        &points,
        (&first, &second),
        &mut crate::nurbs::LaneRefusals::new(),
        "test record",
    )
    .expect("service resource budget")
    .expect("quintic pcurve");
    let (curve, range) = crate::test_support::with_service_context(|ctx| {
        e5_boundary_curve(
            ctx,
            &surface,
            &native,
            &pcurve,
            (
                [0.0, 1.0],
                [Point3::new(1.0, 2.0, 3.0), Point3::new(2.0, 4.0, 3.0)],
            ),
            finite_pair([1.0, 1.0]),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service profile admits E5 boundary curve")
    .expect("plane jet curve");
    assert_eq!(range, [0.0, 1.0]);
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = curve else {
        panic!("expected NURBS curve");
    };
    assert_eq!(
        nurbs.pole_rows().raw_points().first(),
        Some(&Point3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(
        nurbs.pole_rows().raw_points().last(),
        Some(&Point3::new(2.0, 4.0, 3.0))
    );
}

#[test]
fn e5_plane_jet_boundary_rejects_nonfinite_world_poles() {
    let large = f64::MAX * 0.75;
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(large, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let native = E5Pcurve::Jet {
        surface: 0,
        sites: Vec::new(),
        range: finite_pair([0.0, 1.0]),
    };
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(large, 0.0), Point2::new(large, 1.0)],
            None,
            false,
        )
        .expect("fixture pcurve construction admission")
        .expect("valid finite pcurve carrier"),
    };
    assert!(
        crate::test_support::with_service_context(|ctx| e5_boundary_curve(
            ctx,
            &surface,
            &native,
            &pcurve,
            (
                [0.0, 1.0],
                [
                    Point3::new(f64::MAX, 0.0, 0.0),
                    Point3::new(f64::MAX, 1.0, 0.0)
                ]
            ),
            finite_pair([1.0, 1.0]),
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service profile admits E5 boundary curve")
        .is_none()
    );
}

#[test]
fn e5_intersection_requires_equivalent_two_sided_carriers() {
    let left = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    let right = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(
        crate::test_support::with_service_context(|ctx| equivalent_e5_curve_carriers(
            ctx, &left, &right
        ))
        .expect("comparison budget")
    );
    let reversed_axis = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(
        !crate::test_support::with_service_context(|ctx| equivalent_e5_curve_carriers(
            ctx,
            &left,
            &reversed_axis
        ))
        .expect("comparison budget")
    );
    let shifted_reference = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
            4.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(
        !crate::test_support::with_service_context(|ctx| equivalent_e5_curve_carriers(
            ctx,
            &left,
            &shifted_reference
        ))
        .expect("comparison budget")
    );
    assert!(crate::test_support::with_service_context(|ctx| {
        e5_circle_carriers_have_same_ordered_sweep(
            ctx,
            &left,
            [0.0, std::f64::consts::PI],
            &shifted_reference,
            [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
        )
    })
    .expect("evaluation resources"));
    assert!(!crate::test_support::with_service_context(|ctx| {
        e5_circle_carriers_have_same_ordered_sweep(
            ctx,
            &left,
            [0.0, std::f64::consts::PI],
            &shifted_reference,
            [std::f64::consts::FRAC_PI_2, -std::f64::consts::FRAC_PI_2],
        )
    })
    .expect("evaluation resources"));
    let displaced = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.01),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    assert!(
        !crate::test_support::with_service_context(|ctx| equivalent_e5_curve_carriers(
            ctx, &left, &displaced
        ))
        .expect("comparison budget")
    );

    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid LineCurve fixture"),
    ));
    let parallel_line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid LineCurve fixture"),
    ));
    assert!(
        crate::test_support::with_service_context(|ctx| equivalent_e5_curve_carriers(
            ctx,
            &line,
            &parallel_line
        ))
        .expect("comparison budget")
    );
    let reversed_line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(-1.0, 0.0, 0.0),
        )
        .expect("valid LineCurve fixture"),
    ));
    assert!(
        !crate::test_support::with_service_context(|ctx| equivalent_e5_curve_carriers(
            ctx,
            &line,
            &reversed_line
        ))
        .expect("comparison budget")
    );
}

#[test]
fn e5_nonplanar_jet_normalizes_positions_and_derivatives() {
    let surface = crate::families::e5::records::E5Surface {
        pos: 0,
        record_id: 7,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("valid CylinderSurface fixture"),
        )),
        uv_scale: finite_pair([0.5, 1.0]),
    };
    let pcurve = jet_pcurve(
        7,
        vec![0.0, 1.0],
        vec![6, 6],
        vec![[0.0, 3.0], [std::f64::consts::PI, 3.0]],
        vec![[std::f64::consts::PI, 0.0], [std::f64::consts::PI, 0.0]],
        vec![[0.0, 0.0], [0.0, 0.0]],
        [0.0, 1.0],
    );
    let (geometry, range, endpoints) =
        fixture_pcurve_on_surface(&pcurve, &surface, &mut crate::nurbs::LaneRefusals::new())
            .expect("normalized cylinder jet");
    assert_eq!(range, [0.0, 1.0]);
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected NURBS pcurve");
    };
    let control_points = nurbs.pole_rows().raw_points();
    assert_eq!(
        control_points.first(),
        Some(&cadmpeg_ir::math::Point2::new(0.0, 3.0))
    );
    assert_eq!(
        control_points.last(),
        Some(&cadmpeg_ir::math::Point2::new(
            std::f64::consts::FRAC_PI_2,
            3.0
        ))
    );
    assert!(endpoints[0].distance(Point3::new(2.0, 0.0, 3.0)) < EPS_E5_DECODE_EXACT_GEOMETRY);
    assert!(endpoints[1].distance(Point3::new(0.0, 2.0, 3.0)) < EPS_E5_DECODE_EXACT_GEOMETRY);
}

#[test]
fn e5_nurbs_pcurve_evaluates_on_nurbs_surface() {
    let surface = E5Surface {
        pos: 0,
        record_id: 7,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                    vec![
                        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                    ],
                    None,
                ),
                false,
            )
            .expect("fixture constructor admission")
            .expect("valid planar NURBS surface"),
        )),
        uv_scale: finite_pair([1.0, 1.0]),
    };
    let pcurve = E5Pcurve::Nurbs {
        surface: 7,
        degree: 1,
        knots: finite_lane(&[0.0, 0.0, 1.0, 1.0]),
        control_points: vec![finite_pair([0.0, 0.0]), finite_pair([1.0, 1.0])],
        range: finite_pair([0.0, 1.0]),
    };

    let (geometry, range, endpoints) =
        fixture_pcurve_on_surface(&pcurve, &surface, &mut crate::nurbs::LaneRefusals::new())
            .expect("NURBS pcurve on surface");
    assert_eq!(range, [0.0, 1.0]);
    assert!(matches!(
        geometry,
        PcurveGeometry::Nurbs { nurbs }
            if nurbs.degree() == 1
                && nurbs.knots().as_slice() == [0.0, 0.0, 1.0, 1.0]
                && nurbs.control_points()
                    == [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)]
                && nurbs.weights().is_none()
                && !nurbs.periodic()
    ));
    assert_eq!(
        endpoints,
        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)]
    );
}

#[test]
fn e5_cone_jet_uses_the_carrier_chart_for_positions_and_derivatives() {
    let half_angle = std::f64::consts::FRAC_PI_4;
    let surface = E5Surface {
        pos: 0,
        record_id: 7,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
                1.0,
                half_angle,
            )
            .expect("valid ConeSurface fixture"),
        )),
        uv_scale: finite_pair([0.5, half_angle.cos() / 4.0]),
    };
    let pcurve = jet_pcurve(
        7,
        vec![0.0, 1.0],
        vec![6, 6],
        vec![[0.0, 4.0], [std::f64::consts::PI, 4.0]],
        vec![[std::f64::consts::PI, 4.0], [std::f64::consts::PI, 4.0]],
        vec![[0.0, 0.0], [0.0, 0.0]],
        [0.0, 1.0],
    );

    let (geometry, range, endpoints) =
        fixture_pcurve_on_surface(&pcurve, &surface, &mut crate::nurbs::LaneRefusals::new())
            .expect("normalized cone jet");
    assert_eq!(range, [0.0, 1.0]);
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected NURBS pcurve");
    };
    let control_points = nurbs.pole_rows().raw_points();
    assert_eq!(
        control_points.first(),
        Some(&Point2::new(0.0, half_angle.cos()))
    );
    assert_eq!(
        control_points.last(),
        Some(&Point2::new(std::f64::consts::FRAC_PI_2, half_angle.cos()))
    );
    let first_control = control_points.get(1).expect("first derivative control");
    assert!((first_control.u - std::f64::consts::PI / 10.0).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
    assert!((first_control.v - half_angle.cos() * 1.2).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
    let last_control = control_points.get(4).expect("last derivative control");
    assert!(
        (last_control.u - 2.0 * std::f64::consts::PI / 5.0).abs() < EPS_E5_DECODE_EXACT_GEOMETRY
    );
    assert!((last_control.v - half_angle.cos() * 0.8).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
    let expected = [
        Point3::new(
            2.0 + half_angle.tan() * half_angle.cos(),
            0.0,
            half_angle.cos(),
        ),
        Point3::new(
            0.0,
            2.0 + half_angle.tan() * half_angle.cos(),
            half_angle.cos(),
        ),
    ];
    assert!(endpoints[0].distance(expected[0]) < EPS_E5_DECODE_EXACT_GEOMETRY);
    assert!(endpoints[1].distance(expected[1]) < EPS_E5_DECODE_EXACT_GEOMETRY);
}

#[test]
fn e5_pcurve_on_surface_rejects_nonfinite_scaled_line() {
    let surface = crate::families::e5::records::E5Surface {
        pos: 0,
        record_id: 7,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        uv_scale: finite_pair([f64::MAX, 1.0]),
    };
    let pcurve = crate::families::e5::graph::E5Pcurve::Line {
        surface: 7,
        origin: finite_pair([2.0, 0.0]),
        direction: finite_pair([1.0, 0.0]),
        range: finite_pair([0.0, 1.0]),
    };
    assert!(
        fixture_pcurve_on_surface(&pcurve, &surface, &mut crate::nurbs::LaneRefusals::new())
            .is_none()
    );
}

#[test]
fn e5_pcurve_on_surface_rejects_nonfinite_scaled_circle() {
    let surface = crate::families::e5::records::E5Surface {
        pos: 0,
        record_id: 7,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        uv_scale: finite_pair([f64::MAX, 1.0]),
    };
    let pcurve = crate::families::e5::graph::E5Pcurve::Circle {
        surface: 7,
        center: finite_pair([2.0, 0.0]),
        codes: [0, 0],
        radius: positive(1.0),
        range: finite_pair([0.0, 1.0]),
        tail: finite_pair([0.0, 0.0]),
    };
    assert!(
        fixture_pcurve_on_surface(&pcurve, &surface, &mut crate::nurbs::LaneRefusals::new())
            .is_none()
    );
}

#[test]
fn e5_pcurve_on_surface_rejects_nonfinite_generated_jet_poles() {
    let surface = crate::families::e5::records::E5Surface {
        pos: 0,
        record_id: 7,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        uv_scale: finite_pair([1.0, 1.0]),
    };
    let pcurve = jet_pcurve(
        7,
        vec![0.0, 1.0],
        vec![6, 6],
        vec![[f64::MAX, 0.0], [f64::MAX, 0.0]],
        vec![[f64::MAX, 0.0], [f64::MAX, 0.0]],
        vec![[0.0, 0.0], [0.0, 0.0]],
        [0.0, 1.0],
    );
    assert!(
        fixture_pcurve_on_surface(&pcurve, &surface, &mut crate::nurbs::LaneRefusals::new())
            .is_none()
    );
}

#[test]
fn e5_boundary_circle_rejects_nonfinite_center() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(f64::MAX, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let native = crate::families::e5::graph::E5Pcurve::Circle {
        surface: 0,
        center: finite_pair([f64::MAX, 0.0]),
        codes: [0, 0],
        radius: positive(1.0),
        range: finite_pair([0.0, 1.0]),
        tail: finite_pair([0.0, 0.0]),
    };
    let pcurve = rational_pcurve_arc(
        [f64::MAX, 0.0],
        1.0,
        [0.0, 1.0],
        &mut crate::nurbs::LaneRefusals::new(),
        "test record",
    )
    .expect("finite native circle");
    assert!(
        crate::test_support::with_service_context(|ctx| e5_boundary_curve(
            ctx,
            &surface,
            &native,
            &pcurve,
            (
                [0.0, 1.0],
                [
                    Point3::new(f64::MAX, 0.0, 0.0),
                    Point3::new(f64::MAX, 1.0, 0.0)
                ]
            ),
            finite_pair([1.0, 1.0]),
            &mut crate::nurbs::LaneRefusals::new()
        ))
        .expect("service profile admits E5 boundary curve")
        .is_none()
    );
}

#[test]
fn e5_nonplanar_circle_scales_its_rational_uv_control_net() {
    let surface = crate::families::e5::records::E5Surface {
        pos: 0,
        record_id: 7,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
                2.0,
            )
            .expect("valid TorusSurface fixture"),
        )),
        uv_scale: finite_pair([0.2, 0.5]),
    };
    let pcurve = crate::families::e5::graph::E5Pcurve::Circle {
        surface: 7,
        center: finite_pair([10.0, 4.0]),
        codes: [0, 0],
        radius: positive(2.0),
        range: finite_pair([0.0, std::f64::consts::PI]),
        tail: finite_pair([0.0, 0.0]),
    };
    let (geometry, range, endpoints) =
        fixture_pcurve_on_surface(&pcurve, &surface, &mut crate::nurbs::LaneRefusals::new())
            .expect("normalized torus circle");
    assert_eq!(range, [0.0, std::f64::consts::FRAC_PI_2]);
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected rational NURBS pcurve");
    };
    let control_points = nurbs.control_points();
    let first = control_points.first().expect("first control");
    let last = control_points.last().expect("last control");
    assert!(
        (first.u - 2.4).abs() < EPS_E5_DECODE_EXACT_GEOMETRY
            && (first.v - 2.0).abs() < EPS_E5_DECODE_EXACT_GEOMETRY
    );
    assert!(
        (last.u - 2.0).abs() < EPS_E5_DECODE_EXACT_GEOMETRY
            && (last.v - 3.0).abs() < EPS_E5_DECODE_EXACT_GEOMETRY
    );
    let expected = [Point2::new(2.4, 2.0), Point2::new(2.0, 3.0)].map(|uv| {
        cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface.geometry,
            uv.u,
            uv.v,
        )
        .expect("torus point")
        .get()
    });
    assert!(endpoints[0].distance(expected[0]) < EPS_E5_DECODE_EXACT_GEOMETRY);
    assert!(endpoints[1].distance(expected[1]) < EPS_E5_DECODE_EXACT_GEOMETRY);
}

#[test]
fn occurrence_intersection_accepts_roundoff_equivalent_side_ranges() {
    let sides = vec![
        (
            SurfaceId::mint("catia:test:surface#left".to_string()).expect("identity grammar"),
            PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                )
                .expect("valid LinePcurve fixture"),
            ),
            [-2.0, 3.0],
        ),
        (
            SurfaceId::mint("catia:test:surface#right".to_string()).expect("identity grammar"),
            PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(0.0, 1.0),
                    Point2::new(1.0, 0.0),
                )
                .expect("valid LinePcurve fixture"),
            ),
            [-2.0 - 1e-14, 3.0 + 1e-14],
        ),
    ];
    let context = e5_occurrence_intersection_context(&sides).expect("intersection context");
    assert_eq!(context.parameter_range().endpoints(), [-2.0, 3.0]);
    assert_eq!(
        context.sides()[0]
            .surface
            .as_ref()
            .expect("left surface")
            .as_str(),
        "catia:test:surface#left"
    );
    assert_eq!(
        context.sides()[1]
            .surface
            .as_ref()
            .expect("right surface")
            .as_str(),
        "catia:test:surface#right"
    );

    let tiny = 1e-200_f64;
    let mut tiny_sides = sides;
    tiny_sides[0].2 = [0.0, tiny];
    tiny_sides[1].2 = [0.0, tiny];
    assert!(e5_occurrence_intersection_context(&tiny_sides).is_some());
    tiny_sides[1].2 = [0.0, 2.0 * tiny];
    assert!(e5_occurrence_intersection_context(&tiny_sides).is_none());
}

#[test]
fn boundary_planning_rejects_overflowing_reversed_uv_origin() {
    let surface = E5Surface {
        pos: 0,
        record_id: 100,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        uv_scale: finite_pair([1.0, 1.0]),
    };
    let topology = E5Topology {
        bodies: Vec::new(),
        faces: vec![E5Face {
            record_id: 1,
            surface: 100,
            trailer_sign: crate::families::e5::graph::Sign::Positive,
            loops: vec![E5Loop {
                record_id: 2,
                surface: 100,
                members: e5_loop_members(&[20], &[200], &[false]),
                oriented_members: Some(vec![E5OrientedMember {
                    serialized_index: 0,
                    reversed: false,
                }]),
                outer: Some(true),
                orientation_hint: None,
            }],
        }],
        edges: BTreeMap::from([(
            200,
            E5Edge {
                support: 300,
                start_vertex: 400,
                end_vertex: 401,
                parameter_start: 500,
                parameter_end: 501,
                tail: Vec::new(),
            },
        )]),
        pcurves: BTreeMap::from([(
            20,
            E5Pcurve::Line {
                surface: 100,
                origin: finite_pair([0.0, 0.0]),
                direction: finite_pair([1.0, 0.0]),
                range: finite_pair([1e308, 1.1e308]),
            },
        )]),
        bounds: BTreeMap::from([
            (
                500,
                E5Bounds {
                    entries: vec![E5BoundEntry {
                        representation: 20,
                        parameter: crate::test_support::test_b5::finite(0.0),
                        code: 0,
                    }],
                },
            ),
            (
                501,
                E5Bounds {
                    entries: vec![E5BoundEntry {
                        representation: 20,
                        parameter: crate::test_support::test_b5::finite(0.0),
                        code: 0,
                    }],
                },
            ),
        ]),
        curve_supports: BTreeMap::from([(
            300,
            E5CurveSupport {
                kind: E5CurveSupportKind::Boundary(20),
                mode: 0,
                range: finite_pair([1e308, 1.1e308]),
                tail: Vec::new(),
            },
        )]),
        vertex_refs: vec![400, 401],
    };
    let surfaces = HashMap::from([(
        100,
        (
            SurfaceId::mint("catia:test:surface#surface".to_string()).expect("identity grammar"),
            &surface,
        ),
    )]);
    let points = HashMap::from([
        (400, Point3::new(1.1e308, 0.0, 0.0)),
        (401, Point3::new(1e308, 0.0, 0.0)),
    ]);

    let native = topology.pcurves.get(&20).expect("native line");
    let mut refusal = crate::nurbs::LaneRefusals::new();
    let (geometry, range, endpoints) = fixture_pcurve_on_surface(native, &surface, &mut refusal)
        .expect("finite normalized UV endpoints");
    assert_eq!(endpoints, [points[&401], points[&400]]);
    let (curve, curve_range) = crate::test_support::with_service_context(|ctx| {
        e5_boundary_curve(
            ctx,
            &surface.geometry,
            native,
            &geometry,
            (range, endpoints),
            surface.uv_scale,
            &mut refusal,
        )
    })
    .expect("lift budget")
    .expect("finite lifted line");
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::nurbs::reverse_curve_geometry(ctx, &curve, curve_range, &mut refusal, "lifted line")
    })
    .expect("reverse budget")
    .is_some());
    let forward_points = HashMap::from([(400, endpoints[0]), (401, endpoints[1])]);
    assert!(crate::test_support::with_service_context(|ctx| {
        let mut scratch = ctx.reserve_scoped(0, "catia_e5_test_boundary_scratch")?;
        Ok::<_, cadmpeg_core::CodecError>(
            plan_boundary_with_points(
                ctx,
                &mut scratch,
                &topology,
                &surfaces,
                &forward_points,
                &mut crate::nurbs::LaneRefusals::new(),
            )?
            .is_some(),
        )
    })
    .expect("forward planning budget"));
    assert!(crate::test_support::with_service_context(|ctx| {
        let mut scratch = ctx.reserve_scoped(0, "catia_e5_test_boundary_scratch")?;
        Ok::<_, cadmpeg_core::CodecError>(
            plan_boundary_with_points(
                ctx,
                &mut scratch,
                &topology,
                &surfaces,
                &points,
                &mut crate::nurbs::LaneRefusals::new(),
            )?
            .is_none(),
        )
    })
    .expect("service resource budget"));
}

#[test]
fn boundary_planning_checks_reversal_after_fallback_surface_curve() {
    let surface = E5Surface {
        pos: 0,
        record_id: 100,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .expect("finite cylinder"),
        )),
        uv_scale: finite_pair([1.0, 1.0]),
    };
    let pcurve = |reversed, range| {
        jet_pcurve(
            100,
            vec![0.0, 1.0],
            vec![6, 6],
            if reversed {
                vec![[0.0, 1.0], [0.0, 0.0]]
            } else {
                vec![[0.0, 0.0], [0.0, 1.0]]
            },
            vec![[0.0, 0.0]; 2],
            vec![[0.0, 0.0]; 2],
            range,
        )
    };
    let face = |record_id, pcurve| E5Face {
        record_id,
        surface: 100,
        trailer_sign: crate::families::e5::graph::Sign::Positive,
        loops: vec![E5Loop {
            record_id: record_id + 10,
            surface: 100,
            members: e5_loop_members(&[pcurve], &[200], &[false]),
            oriented_members: Some(vec![E5OrientedMember {
                serialized_index: 0,
                reversed: false,
            }]),
            outer: Some(true),
            orientation_hint: None,
        }],
    };
    let mut topology = E5Topology {
        bodies: Vec::new(),
        faces: vec![face(1, 20), face(2, 21)],
        edges: BTreeMap::from([(
            200,
            E5Edge {
                support: 300,
                start_vertex: 400,
                end_vertex: 401,
                parameter_start: 500,
                parameter_end: 501,
                tail: Vec::new(),
            },
        )]),
        pcurves: BTreeMap::from([
            (20, pcurve(false, [0.0, 1.0])),
            (21, pcurve(true, [0.0, 1.0])),
        ]),
        bounds: BTreeMap::new(),
        curve_supports: BTreeMap::from([(
            300,
            E5CurveSupport {
                kind: E5CurveSupportKind::Boundary(20),
                mode: 0,
                range: finite_pair([0.0, 1.0]),
                tail: Vec::new(),
            },
        )]),
        vertex_refs: vec![400, 401],
    };
    let surfaces = HashMap::from([(
        100,
        (
            SurfaceId::mint("catia:test:surface#cylinder".to_string()).expect("identity grammar"),
            &surface,
        ),
    )]);
    let points = HashMap::from([
        (400, Point3::new(1.0, 0.0, 0.0)),
        (401, Point3::new(1.0, 0.0, 1.0)),
    ]);
    assert!(crate::test_support::with_service_context(|ctx| {
        let mut scratch = ctx.reserve_scoped(0, "test_boundary_scratch")?;
        Ok::<_, cadmpeg_core::CodecError>(
            plan_boundary_with_points(
                ctx,
                &mut scratch,
                &topology,
                &surfaces,
                &points,
                &mut crate::nurbs::LaneRefusals::new(),
            )?
            .is_some(),
        )
    })
    .expect("finite fallback planning"));

    topology.pcurves.insert(21, pcurve(true, [1e308, 1.1e308]));
    let native = &topology.pcurves[&21];
    let mut refusal = crate::nurbs::LaneRefusals::new();
    let (geometry, range, endpoints) = fixture_pcurve_on_surface(native, &surface, &mut refusal)
        .expect("finite normalized jet samples");
    assert_eq!(endpoints, [points[&401], points[&400]]);
    assert!(
        crate::test_support::with_service_context(|ctx| e5_boundary_curve(
            ctx,
            &surface.geometry,
            native,
            &geometry,
            (range, endpoints),
            surface.uv_scale,
            &mut refusal,
        ))
        .expect("lift budget")
        .is_none()
    );
    assert!(crate::test_support::with_service_context(|ctx| {
        let mut scratch = ctx.reserve_scoped(0, "test_boundary_scratch")?;
        Ok::<_, cadmpeg_core::CodecError>(
            plan_boundary_with_points(
                ctx,
                &mut scratch,
                &topology,
                &surfaces,
                &points,
                &mut refusal,
            )?
            .is_none(),
        )
    })
    .expect("fallback planning budget"));
    assert_eq!(refusal.note_count(), 1);
    assert!(
        matches!(crate::test_support::with_work_refusal("catia_e5_reversed_pcurve_knots", |ctx| {
        let mut scratch = ctx.reserve_scoped(0, "test_boundary_scratch")?;
        plan_boundary_with_points(
            ctx, &mut scratch, &topology, &surfaces, &points,
            &mut crate::nurbs::LaneRefusals::new(),
        ).map(|plan| plan.is_none())
    }), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_e5_reversed_pcurve_knots")
    );
}
