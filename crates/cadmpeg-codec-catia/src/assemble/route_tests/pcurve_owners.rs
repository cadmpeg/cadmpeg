//! Actual pcurve output promotion boundaries.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, WeightedPole2};
use cadmpeg_ir::units::FinitePoint2;

const EPS_ARC_EVALUATION: f64 = 1.0e-12;
const EPS_QUINTIC_EVALUATION: f64 = 1.0e-12;

#[test]
fn rational_pcurve_promotion_owns_both_surviving_lanes() {
    let run = |ctx: &DecodeContext<'_>| {
        super::super::rational_pcurve_arc(
            ctx,
            [0.0, 0.0],
            2.0,
            [0.0, std::f64::consts::FRAC_PI_2],
            &mut crate::nurbs::LaneRefusals::new(),
            "quarter arc",
        )
    };
    // One quadratic span keeps six raw knots and three admitted weighted poles.
    // Raw input poles, input weights and intermediate paired poles are temporary.
    let retained = u64_from_index(
        6 * std::mem::size_of::<f64>() + 3 * std::mem::size_of::<WeightedPole2<FinitePoint2>>(),
    );
    for cap in [0, retained - 1] {
        crate::test_support::with_retained_limit(cap, |ctx| {
            let CodecError::ResourceLimit(limit) = run(ctx).expect_err("complete output promotion")
            else {
                panic!("resource refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, "catia_rational_arc_retained_knots");
            assert_eq!(limit.limit, cap);
            assert_eq!(limit.used, 0);
            assert_eq!(limit.additional, retained);
            assert_eq!(ctx.resource_refusal(), Some(limit.clone()));
            assert!(matches!(run(ctx), Err(CodecError::ResourceLimit(later)) if later == limit));
        });
    }
    let geometry = crate::test_support::with_retained_limit(retained, run)
        .expect("only surviving backing fits")
        .expect("valid quarter arc");
    let PcurveGeometry::Nurbs { nurbs } = &geometry else {
        panic!("rational arc");
    };
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(
        nurbs.knots().as_slice(),
        [
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2
        ]
    );
    assert_eq!(nurbs.pole_rows().count(), 3);
    crate::test_support::with_service_context(|ctx| {
        for (parameter, expected) in [
            (0.0, [2.0, 0.0]),
            (std::f64::consts::FRAC_PI_4, [std::f64::consts::SQRT_2; 2]),
            (std::f64::consts::FRAC_PI_2, [0.0, 2.0]),
        ] {
            let point = cadmpeg_ir::eval::decode::pcurve_uv(ctx, &geometry, parameter)
                .expect("admitted arc evaluation")
                .get();
            assert!((point.u - expected[0]).abs() < EPS_ARC_EVALUATION);
            assert!((point.v - expected[1]).abs() < EPS_ARC_EVALUATION);
        }
    });
}

#[test]
fn quintic_pcurve_promotion_owns_the_complete_jet() {
    let run = |ctx: &DecodeContext<'_>| {
        super::super::quintic_jet_pcurve(
            ctx,
            5,
            &[0.0, 1.0],
            &[[0.0, 0.0], [1.0, 0.0]],
            (&[[1.0, 0.0]; 2], &[[0.0, 0.0]; 2]),
            &mut crate::nurbs::LaneRefusals::new(),
            "linear quintic jet",
        )
    };
    // One quintic span keeps twelve knots and six admitted UV controls.
    let retained =
        u64_from_index(12 * std::mem::size_of::<f64>() + 6 * std::mem::size_of::<FinitePoint2>());
    crate::test_support::with_retained_limit(retained - 1, |ctx| {
        let CodecError::ResourceLimit(limit) = run(ctx).expect_err("complete jet promotion") else {
            panic!("resource refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, "catia_quintic_pcurve_jet");
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, retained);
        assert_eq!(ctx.resource_refusal(), Some(limit.clone()));
        assert!(matches!(run(ctx), Err(CodecError::ResourceLimit(later)) if later == limit));
    });
    let geometry = crate::test_support::with_retained_limit(retained, run)
        .expect("exact complete jet backing fits")
        .expect("linear quintic pcurve");
    let PcurveGeometry::Nurbs { nurbs } = &geometry else {
        panic!("quintic jet");
    };
    assert_eq!(nurbs.degree(), 5);
    assert_eq!(nurbs.knots().len(), 12);
    assert_eq!(nurbs.pole_rows().count(), 6);
    assert!(nurbs.weights().is_none());
    crate::test_support::with_service_context(|ctx| {
        for (parameter, expected) in [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)] {
            let point = cadmpeg_ir::eval::decode::pcurve_uv(ctx, &geometry, parameter)
                .expect("admitted quintic evaluation")
                .get();
            assert!((point.u - expected).abs() < EPS_QUINTIC_EVALUATION);
            assert!(point.v.abs() < EPS_QUINTIC_EVALUATION);
        }
    });
}
