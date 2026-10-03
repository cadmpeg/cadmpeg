// SPDX-License-Identifier: Apache-2.0

use crate::assemble::quintic_jet_pcurve;
use crate::families::e5::decode::{
    e5_occurrence_intersection_cache, e5_support_occurrence_intersection_context,
    parameter_range_agreement_tolerance, E5OccurrenceIntersectionSide,
    EPS_E5_DECODE_EXACT_GEOMETRY,
};

use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve, pcurve::PcurveGeometry, CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

#[test]
fn support_range_agreement_requires_matching_endpoints() {
    assert!(parameter_range_agreement_tolerance([0.0, 5.0], [0.0, 5.0]).is_some());
    assert!(parameter_range_agreement_tolerance([0.0, 5.0], [1.0, 6.0]).is_none());
}

#[test]
fn occurrence_context_refuses_before_retained_surface_copy() {
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("valid line pcurve"),
    );
    let sides = ["left", "right"].map(|name| E5OccurrenceIntersectionSide {
        surface: SurfaceId::mint(format!("catia:test:surface#{name}")).expect("identity grammar"),
        pcurve: pcurve.clone(),
        pcurve_range: [0.0, 1.0],
        curve: None,
    });
    let refused = crate::test_support::with_retained_limit(0, |ctx| {
        e5_support_occurrence_intersection_context(ctx, [0.0, 1.0], [0.0, 1.0], &sides)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_e5_occurrence_context_surface_id")
    );
}

#[test]
fn e5_boundary_nurbs_cache_refuses_before_copy() {
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        ).expect("fixture constructor admission")
        .expect("valid linear NURBS"),
    ));
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        curve.try_clone_for_decode(ctx, "catia_e5_boundary_curve_copy")
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_e5_boundary_curve_copy")
    );
}

#[test]
fn e5_intersection_context_copy_refuses_retained_limit() {
    let sides = ["left", "right"].map(|name| cadmpeg_ir::geometry::IntcurveSupportSide {
        surface: Some(
            SurfaceId::mint(format!("catia:test:surface#{name}")).expect("valid surface identity"),
        ),
        pcurve: None,
    });
    let context = cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
        sides,
        [0.0, 1.0],
        std::array::from_fn(|_| Vec::new()),
    )
    .expect("valid support context");
    let refused = crate::test_support::with_retained_limit(0, |ctx| {
        crate::resource::copy_intcurve_support_context(
            ctx,
            &context,
            "catia_e5_intersection_context",
        )
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_e5_intersection_context")
    );
    let copied = crate::test_support::with_service_context(|ctx| {
        crate::resource::copy_intcurve_support_context(
            ctx,
            &context,
            "catia_e5_intersection_context",
        )
    })
    .expect("service budget admits context copy");
    assert_eq!(copied, context);
}

#[test]
fn wide_occurrence_ranges_keep_matching_support_and_curve_cache() {
    let range = [-f64::MAX, f64::MAX];
    assert!(parameter_range_agreement_tolerance(range, range).is_some());
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("finite line"),
    ));
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        )
        .expect("finite pcurve"),
    );
    let sides = [
        E5OccurrenceIntersectionSide {
            surface: SurfaceId::mint("catia:test:surface#wide-left".to_string())
                .expect("identity grammar"),
            pcurve: pcurve.clone(),
            pcurve_range: range,
            curve: Some((line.clone(), range)),
        },
        E5OccurrenceIntersectionSide {
            surface: SurfaceId::mint("catia:test:surface#wide-right".to_string())
                .expect("identity grammar"),
            pcurve,
            pcurve_range: range,
            curve: Some((line.clone(), range)),
        },
    ];
    let context = crate::test_support::with_service_context(|ctx| {
        e5_support_occurrence_intersection_context(ctx, range, range, &sides)
    })
    .expect("service budget admits context")
    .expect("wide support context");
    assert_eq!(context.parameter_range().endpoints(), range);
    let (cached, cached_range) = crate::test_support::with_service_context(|ctx| {
        e5_occurrence_intersection_cache(ctx, &sides)
    })
    .expect("evaluation resources")
    .expect("wide exact carrier cache");
    assert_eq!(cached, &line);
    assert_eq!(cached_range, range);
}

#[test]
fn occurrence_intersection_maps_distinct_local_ranges_to_support_range() {
    let sides = vec![
        E5OccurrenceIntersectionSide {
            surface: SurfaceId::mint("catia:test:surface#left".to_string())
                .expect("identity grammar"),
            pcurve: PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                )
                .expect("valid LinePcurve fixture"),
            ),
            pcurve_range: [100.0, 200.0],
            curve: None,
        },
        E5OccurrenceIntersectionSide {
            surface: SurfaceId::mint("catia:test:surface#right".to_string())
                .expect("identity grammar"),
            pcurve: PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(0.0, 1.0),
                    Point2::new(1.0, 0.0),
                )
                .expect("valid LinePcurve fixture"),
            ),
            pcurve_range: [-5.0, 5.0],
            curve: None,
        },
    ];
    let context = crate::test_support::with_service_context(|ctx| {
        e5_support_occurrence_intersection_context(ctx, [10.0, 20.0], [10.0, 20.0], &sides)
    })
    .expect("service budget admits context")
    .expect("support intersection context");
    assert_eq!(context.parameter_range().endpoints(), [10.0, 20.0]);
    assert_eq!(
        context.sides()[0]
            .pcurve_parameter_range()
            .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints)
            .expect("left local range"),
        [100.0, 200.0]
    );
    assert_eq!(
        context.sides()[1]
            .pcurve_parameter_range()
            .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints)
            .expect("right local range"),
        [-5.0, 5.0]
    );
    assert_eq!(
        context.sides()[0]
            .pcurve_parameter(
                cadmpeg_ir::topology::ParameterInterval::new([10.0, 20.0])
                    .expect("finite ordered range"),
                15.0,
            )
            .expect("left mapped parameter")
            .get(),
        150.0
    );
    assert_eq!(
        context.sides()[1]
            .pcurve_parameter(
                cadmpeg_ir::topology::ParameterInterval::new([10.0, 20.0])
                    .expect("finite ordered range"),
                15.0,
            )
            .expect("right mapped parameter")
            .get(),
        0.0
    );
}

#[test]
fn occurrence_intersection_cache_requires_one_admitted_exact_carrier() {
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid LineCurve fixture"),
    ));
    let nurbs = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        ).expect("fixture constructor admission")
        .expect("valid linear NURBS"),
    ));
    let mut sides = vec![
        E5OccurrenceIntersectionSide {
            surface: SurfaceId::mint("catia:test:surface#left".to_string())
                .expect("identity grammar"),
            pcurve: PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                )
                .expect("valid LinePcurve fixture"),
            ),
            pcurve_range: [10.0, 20.0],
            curve: Some((line.clone(), [0.0, 1.0])),
        },
        E5OccurrenceIntersectionSide {
            surface: SurfaceId::mint("catia:test:surface#right".to_string())
                .expect("identity grammar"),
            pcurve: PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(0.0, 1.0),
                    Point2::new(1.0, 0.0),
                )
                .expect("valid LinePcurve fixture"),
            ),
            pcurve_range: [-4.0, 6.0],
            curve: Some((nurbs, [100.0, 110.0])),
        },
    ];
    let (cache, range) = crate::test_support::with_service_context(|ctx| {
        e5_occurrence_intersection_cache(ctx, &sides)
    })
    .expect("evaluation resources")
    .expect("analytic cache");
    assert_eq!(cache, &line);
    assert_eq!(range, [0.0, 1.0]);

    sides[1].curve = Some((
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .expect("valid CircleCurve fixture"),
        )),
        [0.0, 1.0],
    ));
    assert!(
        crate::test_support::with_service_context(|ctx| e5_occurrence_intersection_cache(
            ctx, &sides
        ))
        .expect("evaluation resources")
        .is_none()
    );

    let left_circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    let right_circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
            4.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    sides[0].curve = Some((left_circle.clone(), [0.0, std::f64::consts::PI]));
    sides[1].curve = Some((
        right_circle,
        [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
    ));
    let (cache, range) = crate::test_support::with_service_context(|ctx| {
        e5_occurrence_intersection_cache(ctx, &sides)
    })
    .expect("evaluation resources")
    .expect("frame-gauged circle cache");
    assert_eq!(cache, &left_circle);
    assert_eq!(range, [0.0, std::f64::consts::PI]);
}

#[test]
fn quintic_jet_reproduces_endpoint_second_order_data() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits input limit");
    let curve = quintic_jet_pcurve(
        &ctx,
        5,
        &[0.0, 2.0],
        &[[0.0, 0.0], [2.0, 0.0]],
        (&[[1.0, 0.0], [1.0, 0.0]], &[[0.0, 0.0], [0.0, 0.0]]),
        &mut crate::nurbs::LaneRefusals::new(),
        "test record",
    )
    .expect("service resource budget")
    .expect("linear quintic segment");
    for parameter in [0.0, 0.5, 1.0, 2.0] {
        let point = cadmpeg_ir::eval::decode::pcurve_uv(cadmpeg_ir::eval::admission::EvaluationAdmission::Standard, &curve, parameter).expect("jet evaluation");
        assert!((point.u - parameter).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!(point.v.abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
    }
}

#[test]
fn reversing_nurbs_preserves_tiny_knot_domain() {
    let tiny = 1e-200;
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            vec![tiny, tiny, 2.0 * tiny, 2.0 * tiny],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        ).expect("fixture constructor admission")
        .expect("valid tiny-domain NURBS"),
    ));
    let (reversed, range) = crate::test_support::with_service_context(|ctx| {
        crate::nurbs::reverse_curve_geometry(
            ctx,
            &curve,
            [tiny, 2.0 * tiny],
            &mut crate::nurbs::LaneRefusals::new(),
            "test record",
        )
    })
    .expect("service profile admits range operation")
    .expect("reversed NURBS");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(reversed)) = reversed else {
        panic!("expected NURBS");
    };
    assert_eq!(range, [tiny, 2.0 * tiny]);
    assert_eq!(
        reversed.knots().as_slice(),
        [tiny, tiny, 2.0 * tiny, 2.0 * tiny]
    );
    assert_eq!(
        reversed.control_points(),
        [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)]
    );
}
