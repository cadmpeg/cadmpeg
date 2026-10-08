use super::{B5Pcurve, B5PcurveParameterization, B5Surface, CurveGeometry, SolvedCurveGeometry};
use crate::test_support::test_b5::{
    finite, finite_lane, finite_vector, frame, increasing, point, positive_length,
};

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
    crate::test_support::with_work_limit(1, |ctx| {
        assert!(!crate::families::b5::graph::pcurve_knot_expansion_is_finite(ctx, &pcurve)?);
        assert!(ctx.resource_refusal().is_none());
        pcurve.parameterization = B5PcurveParameterization::Native;
        let cadmpeg_core::CodecError::ResourceLimit(original) =
            crate::families::b5::graph::pcurve_knot_expansion_is_finite(ctx, &pcurve)
                .expect_err("second pair scan refuses")
        else {
            panic!("typed refusal")
        };
        assert_eq!((original.used, original.additional), (1, 1));
        pcurve.distinct_knots.clear();
        assert!(matches!(
            crate::families::b5::graph::pcurve_knot_expansion_is_finite(ctx, &pcurve),
            Err(cadmpeg_core::CodecError::ResourceLimit(next)) if next == original
        ));
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("admission assertions");
}
