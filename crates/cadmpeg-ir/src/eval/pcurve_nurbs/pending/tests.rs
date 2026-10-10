// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;
use crate::eval::pcurve_nurbs::{differential, differential_pending};
use crate::geometry::pcurve::{PcurveNurbs, PolarNurbsPole, PolarPcurveNurbs};
use crate::units::FinitePoint2;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const EPS_PENDING_PCURVE: f64 = 64.0 * f64::EPSILON;
const EMPTY_ROOT_MATERIALIZED_ALLOWANCE: u64 = 16 * 1024 * 1024;

fn planar(degree: u32, rational: bool) -> PcurveNurbs {
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let count = usize::try_from(degree + 1).unwrap();
    let knots = std::iter::repeat_n(0.0, count).chain(std::iter::repeat_n(1.0, count)).collect::<Vec<_>>();
    let points = (0..=degree).map(|i| {
        let value = 2.0_f64.powi(i32::try_from(i).unwrap());
        Point2::new(value, -2.0 * value)
    }).collect();
    let weights = rational.then(|| (0..=degree).map(|i| 1.0 + f64::from(i) / f64::from(degree)).collect());
    let result = PcurveNurbs::from_lanes(&ctx, degree, knots, points, weights, false).unwrap().unwrap();
    ctx.finish_session().unwrap();
    result
}

fn polar(rational: bool) -> PolarPcurveNurbs {
    // Actual contextual constructor setup precedes the evaluation session.
    // This fixture does not certify Standard construction.
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let points = [0.0, 0.5, 1.0].map(|t| PolarNurbsPole { radial: Point2::new(1.0, t), axial: t });
    let result = PolarPcurveNurbs::from_lanes(&ctx, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        points.to_vec(), rational.then(|| vec![1.0; 3]), false).unwrap().unwrap();
    ctx.finish_session().unwrap();
    result
}

#[test]
fn pending_real_planar_lower_keeps_selected_polynomial_and_rational_laws() {
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for rational in [false, true] {
        let curve = planar(4, rational);
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            for parameter in [0.0, 0.5, 1.0] {
                let scratch = decode::Scratch::new(admission);
                let poles = DifferentialPoles::Stored(curve.pole_rows());
                let t = FiniteReal::new(parameter).unwrap();
                let old = differential(&scratch, curve.degree(), curve.knots(), poles, t).unwrap();
                let pending = differential_pending(&scratch, curve.degree(), curve.knots(), poles, t, 5).unwrap();
                let lower = pending.lower();
                assert_eq!(lower.point, old.point);
                assert_eq!(lower.tangent, old.tangent);
                assert_eq!(lower.acceleration, old.acceleration);
                assert_eq!(lower.higher, [Err(EvaluationFailure::NoValue); 3]);
                // Polynomial q=(1+t)^4. With w_i=1+i/4 and P_i=2^i,
                // H=(1+t)^3(1+3t), W=1+t, so q=(1+t)^2(1+3t).
                let expected = if rational { [18.0, 0.0, 0.0] }
                    else { [24.0 * (1.0 + parameter), 24.0, 0.0] };
                let actual = pending.complete().unwrap();
                assert_eq!(actual.point, old.point);
                assert_eq!(actual.tangent, old.tangent);
                assert_eq!(actual.acceleration, old.acceleration);
                for (actual, expected) in actual.higher.into_iter().zip(expected) {
                    let expected = Point2::new(expected, -2.0 * expected);
                    let actual = actual.unwrap().get();
                    assert!((actual.u - expected.u).abs() <= EPS_PENDING_PCURVE * (1.0 + expected.u.abs()));
                    assert!((actual.v - expected.v).abs() <= EPS_PENDING_PCURVE * (1.0 + expected.v.abs()));
                }
            }
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn pending_real_polar_lower_pair_keeps_original_combined_caps_and_release() {
    for rational in [false, true] {
        let curve = polar(rational);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 22;
        policy.limits.max_materialized_bytes = 512;
        policy.limits.max_work_units = 256;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let scratch = decode::Scratch::new(admission);
            let at = FiniteReal::HALF;
            let radial = differential_pending(&scratch, curve.degree(), curve.knots(),
                DifferentialPoles::PolarRadial(curve.pole_rows()), at, 5).unwrap();
            let axial = differential_pending(&scratch, curve.degree(), curve.knots(),
                DifferentialPoles::PolarAxial(curve.pole_rows()), at, 5).unwrap();
            assert_eq!(radial.lower().point.get(), Point2::new(1.0, 0.5));
            assert_eq!(axial.lower().point.get(), Point2::new(0.5, 0.0));
            assert_eq!(radial.lower().higher, [Err(EvaluationFailure::NoValue); 3]);
            assert_eq!(axial.lower().higher, [Err(EvaluationFailure::NoValue); 3]);
            let zero = FinitePoint2::from_coordinates(FiniteReal::ZERO, FiniteReal::ZERO);
            assert_eq!(radial.complete().unwrap().higher, [Ok(zero); 3]);
            assert_eq!(axial.complete().unwrap().higher, [Ok(zero); 3]);
        }
        drop(ctx.reserve_scoped_limit(512, "actual pending polar backing released").unwrap());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn pending_radial_completion_preserves_genuine_axial_lower_refusal() {
    for rational in [false, true] {
        let curve = polar(rational);
        let mut policy = DecodePolicy::service();
        // The radial lower owns3+3+2+3 items. The next axial point basis
        // genuinely requests3 more; higher formation has not run.
        policy.limits.max_collection_items = 11;
        policy.limits.max_materialized_bytes = 512;
        policy.limits.max_work_units = 256;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = decode::Scratch::new(&ctx);
        let radial = differential_pending(&scratch, curve.degree(), curve.knots(),
            DifferentialPoles::PolarRadial(curve.pole_rows()), FiniteReal::HALF, 5).unwrap();
        assert_eq!(radial.lower().point.get(), Point2::new(1.0, 0.5));
        let Err(EvaluationFailure::ResourceLimit(original)) = differential_pending(&scratch,
            curve.degree(), curve.knots(), DifferentialPoles::PolarAxial(curve.pole_rows()),
            FiniteReal::HALF, 5) else { panic!("actual axial lower must refuse"); };
        assert_eq!(original.dimension, ResourceDimension::CollectionItems);
        assert_eq!(original.operation, "IR B-spline basis");
        assert_eq!((original.limit, original.used, original.additional), (11, 11, 3));
        assert!(matches!(radial.complete(), Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
        drop(scratch);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
    }
}

#[test]
fn pending_actual_heap_basis_and_captured_rows_release_after_unwind() {
    let curve = planar(5, false);
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = decode::Scratch::new(&ctx);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let pending = differential_pending(&scratch, curve.degree(), curve.knots(),
            DifferentialPoles::Stored(curve.pole_rows()), FiniteReal::HALF, 5).unwrap();
        assert_eq!(pending.lower().higher, [Err(EvaluationFailure::NoValue); 3]);
        panic!("actual held pending basis unwind");
    }));
    assert!(panic.is_err());
    assert_eq!(ctx.resource_refusal(), None);
    drop(scratch);
    drop(ctx.reserve_scoped_limit(EMPTY_ROOT_MATERIALIZED_ALLOWANCE,
        "actual pending heap and capture backing destroyed").unwrap());
    ctx.finish_session().unwrap();
}
