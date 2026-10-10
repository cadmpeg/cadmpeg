use super::{B5Pcurve, B5PcurveParameterization, B5Surface, CurveGeometry, SolvedCurveGeometry};
use crate::test_support::test_b5::{
    exact_unit, finite, finite_lane, finite_vector, frame, increasing, plane_frame, point,
    positive_length,
};

// Count pairs (including the current core end probe), emit pairs, write knots,
// collect raw points, admit poles, convert finite knots, compare adjacent knots.
const PLANE_NATIVE_WORK: u64 = (2 + 1) + 2 + 4 + (2 + 1) + (2 + 1) + (4 + 1) + 3;

fn fixture() -> (B5Pcurve, B5Surface) {
    let pcurve = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 1,
        distinct_knots: finite_lane(&[0.0, 1.0]),
        multiplicities: vec![2, 2],
        control_points: vec![finite_vector([0.0, 2.0]), finite_vector([3.0, 2.0])],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    let cylinder = B5Surface::Cylinder {
        origin: point([0.0; 3]),
        frame: frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        radius: positive_length(2.0),
        u_range: increasing([0.0, 4.0 * std::f64::consts::PI]),
        v_range: increasing([-1.0, 1.0]),
        angular_scale: finite(2.0),
        chart_origin: finite(0.0),
    };
    (pcurve, cylinder)
}

#[test]
fn analytic_lifting_does_not_allocate_discarded_knots() {
    let (mut pcurve, cylinder) = fixture();
    pcurve.multiplicities = vec![u32::MAX, u32::MAX];
    let geometry = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::super::pcurves::lifted_curve_geometry(ctx, &pcurve, &cylinder)
    })
    .expect("analytic lifting needs no expanded knot storage");
    assert!(
        matches!(geometry, Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle))) if circle.radius().get() == 2.0)
    );
}

#[test]
fn analytic_lifting_preserves_translated_knot_refusal() {
    let (mut pcurve, cylinder) = fixture();
    pcurve.distinct_knots = finite_lane(&[-f64::MAX, 1.0]);
    pcurve.parameterization = B5PcurveParameterization::Translated {
        native_origin: finite(f64::MAX),
    };
    let result = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::super::pcurves::lifted_curve_geometry(ctx, &pcurve, &cylinder)
    })
    .expect("invalid translation needs no storage");
    assert!(result.is_none());
    pcurve.multiplicities[0] = 0;
    assert!(super::lifted_curve_geometry(&pcurve, &cylinder).is_some());
}

#[test]
fn analytic_knot_validation_charges_only_visited_distinct_pairs() {
    let (mut pcurve, _) = fixture();
    pcurve.distinct_knots = finite_lane(&[-f64::MAX, 1.0]);
    pcurve.parameterization = B5PcurveParameterization::Translated {
        native_origin: finite(f64::MAX),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root fits the work limit");
    assert!(
        !crate::families::b5::graph::pcurve_knot_expansion_is_finite(&ctx, &pcurve)
            .expect("first invalid pair fits one visit")
    );
    assert!(ctx.resource_refusal().is_none());
    pcurve.parameterization = B5PcurveParameterization::Native;
    let cadmpeg_core::CodecError::ResourceLimit(original) =
        crate::families::b5::graph::pcurve_knot_expansion_is_finite(&ctx, &pcurve)
            .expect_err("second pair scan refuses")
    else {
        panic!("typed refusal")
    };
    assert_eq!((original.used, original.additional), (1, 1));
    pcurve.distinct_knots.clear();
    let empty = crate::families::b5::graph::pcurve_knot_expansion_is_finite(&ctx, &pcurve);
    assert!(matches!(empty.as_ref(), Ok(&true)));
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(
        ctx.finish(empty),
        Err(cadmpeg_core::CodecError::ResourceLimit(next)) if next == original
    ));
}

fn plane() -> B5Surface {
    B5Surface::Plane {
        origin: point([1.0, 2.0, 3.0]),
        frame: plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        direction_v: exact_unit([0.0, 1.0, 0.0]),
        u_range: increasing([-1.0, 1.0]),
        v_range: increasing([-1.0, 1.0]),
    }
}

#[test]
fn plane_lifting_keeps_finite_knots_at_exact_work_limit() {
    for translated in [false, true] {
        let (mut pcurve, _) = fixture();
        let mut work = PLANE_NATIVE_WORK;
        if translated {
            pcurve.distinct_knots = finite_lane(&[4.0, 5.0]);
            pcurve.parameterization = B5PcurveParameterization::Translated {
                native_origin: finite(4.0),
            };
            work += 4 + 1; // Translate four knots and the current core end probe.
        }
        crate::test_support::with_work_limit(work, |ctx| {
            let Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))) =
                super::super::super::pcurves::lifted_curve_geometry(ctx, &pcurve, &plane())
                    .expect("checked knots fit the actual construction work")
            else {
                panic!("plane lift must retain its NURBS carrier")
            };
            assert_eq!(curve.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
            assert_eq!(
                curve.control_points(),
                vec![
                    cadmpeg_ir::math::Point3::new(1.0, 4.0, 3.0),
                    cadmpeg_ir::math::Point3::new(4.0, 4.0, 3.0),
                ]
            );
            assert!(ctx.resource_refusal().is_none());
        });
    }
}

#[test]
fn plane_knot_order_refusal_remains_sticky() {
    let (mut pcurve, _) = fixture();
    crate::test_support::with_work_limit(PLANE_NATIVE_WORK - 1, |ctx| {
        let cadmpeg_core::CodecError::ResourceLimit(original) =
            super::super::super::pcurves::lifted_curve_geometry(ctx, &pcurve, &plane())
                .expect_err("the last knot-order comparison must refuse")
        else {
            panic!("typed caller work refusal")
        };
        assert_eq!(original.operation, "IR NURBS knot order");
        assert_eq!(
            (original.used, original.additional),
            (PLANE_NATIVE_WORK - 1, 1)
        );
        assert_eq!(ctx.resource_refusal(), Some(original));
        pcurve.distinct_knots.clear();
        pcurve.multiplicities.clear();
        assert!(matches!(
            super::super::super::pcurves::lifted_curve_geometry(ctx, &pcurve, &plane()),
            Err(cadmpeg_core::CodecError::ResourceLimit(next)) if next == original
        ));
    });
}

#[test]
fn plane_finite_knots_still_require_order_and_cardinality() {
    let (mut pcurve, _) = fixture();
    pcurve.distinct_knots = finite_lane(&[1.0, 0.0]);
    assert!(super::lifted_curve_geometry(&pcurve, &plane()).is_none());
    pcurve.distinct_knots = finite_lane(&[0.0, 1.0]);
    pcurve.multiplicities = vec![1, 2];
    assert!(super::lifted_curve_geometry(&pcurve, &plane()).is_none());
    pcurve.multiplicities = vec![2, 2];
    pcurve.distinct_knots = finite_lane(&[-f64::MAX, 1.0]);
    pcurve.parameterization = B5PcurveParameterization::Translated {
        native_origin: finite(f64::MAX),
    };
    assert!(super::lifted_curve_geometry(&pcurve, &plane()).is_none());
}

#[test]
fn plane_lifting_retains_only_the_constructed_knot_and_pole_lanes() {
    for rational in [false, true] {
        let (mut pcurve, _) = fixture();
        if rational {
            pcurve.weights = Some(vec![
                crate::test_support::test_b5::positive(1.0),
                crate::test_support::test_b5::positive(2.0),
            ]);
        }
        // Four f64 knots and two finite poles. Rational poles add one f64 weight.
        let retained = 4 * 8 + 2 * if rational { 32 } else { 24 };
        crate::test_support::with_retained_limit(retained, |ctx| {
            let Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))) =
                super::super::super::pcurves::lifted_curve_geometry(ctx, &pcurve, &plane())
                    .expect("only the final lanes need retained allowance")
            else {
                panic!("plane NURBS lift")
            };
            assert_eq!(curve.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
            assert_eq!(curve.pole_rows().count(), 2);
        });
    }
}
