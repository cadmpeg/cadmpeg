// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::geometry::{nurbs::NurbsCurve, SolvedCurveGeometry};
use crate::math::{Point3, Vector3};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

mod quadratic;

fn line(count: u32, rational: bool) -> SolvedCurveGeometry {
    // Greville abscissae reproduce C(t)=(t,0,0) for a degree2 open basis.
    let end = f64::from(count - 2);
    let knots: Vec<_> = [0.0; 3].into_iter()
        .chain((1..count - 2).map(f64::from)).chain([end; 3]).collect();
    let points = (0..count).map(|index| {
        let index = usize::try_from(index).unwrap();
        Point3::new(knots[index + 1].midpoint(knots[index + 2]), 0.0, 0.0)
    }).collect();
    let weights = rational.then(|| (0..count).map(|_| 1.0).collect());
    // Fixture construction is outside the evaluation session under test.
    SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2, knots, points, weights, false).unwrap().unwrap())
}

fn evaluate(ctx: &DecodeContext<'_>, geometry: &SolvedCurveGeometry, second: bool)
    -> Result<FiniteVector3, EvaluationFailure<()>>
{
    if second { crate::eval::curve_second_derivative_solved(ctx, geometry, 0.5) }
    else { crate::eval::curve_tangent_solved(ctx, geometry, 0.5) }
}

fn limits(second: bool) -> (u64, u64) {
    // First: basis3 + derivative3, both actual capacity4 f64 lanes.
    // Second also holds the lower derivative2 and second derivative3 lanes.
    (if second { 16 } else { 8 } * u64::try_from(std::mem::size_of::<f64>()).unwrap(),
        if second { 11 } else { 6 })
}

#[test]
fn stored_derivatives_borrow_small_and_large_polynomial_and_rational_rows() {
    for count in [3, 1024] {
        for rational in [false, true] {
            let geometry = line(count, rational);
            for second in [false, true] {
                let (bytes, items) = limits(second);
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = bytes;
                policy.limits.max_collection_items = items;
                policy.limits.max_retained_bytes = 0;
                // Span search <=10; First basis/inspection/collection <=18;
                // each homogeneous support3 sum <=27 admitted advances.
                // Second adds <=8 basis/inspection steps and one such sum.
                // Thus First <=82 and Second <=117, independently of poles.
                policy.limits.max_work_units = 128;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let expected = if second { Vector3::new(0.0, 0.0, 0.0) }
                    else { Vector3::new(1.0, 0.0, 0.0) };
                assert_eq!(evaluate(&ctx, &geometry, second).unwrap().get(), expected);
                drop(ctx.reserve_scoped_limit(bytes, "borrowed derivative backing released").unwrap());
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn borrowed_derivative_basis_refusals_keep_the_original_session_error() {
    for rational in [false, true] {
        let geometry = line(1024, rational);
        for second in [false, true] {
            for materialized in [false, true] {
                let (bytes, items) = limits(second);
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = bytes - u64::from(materialized);
                policy.limits.max_collection_items = items - u64::from(!materialized);
                policy.limits.max_retained_bytes = 0;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let Err(EvaluationFailure::ResourceLimit(original)) = evaluate(&ctx, &geometry, second)
                    else { panic!("one-below actual basis backing or item count must refuse"); };
                assert_eq!(original.dimension, if materialized { ResourceDimension::MaterializedBytes }
                    else { ResourceDimension::CollectionItems });
                assert_eq!(original.operation, if second { "IR B-spline second derivative basis" }
                    else { "IR B-spline derivative basis" });
                assert_eq!(crate::eval::curve_tangent_solved(&ctx, &geometry, f64::NAN),
                    Err(EvaluationFailure::ResourceLimit(original)));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
            }
        }
    }
}

#[test]
fn borrowed_stored_and_raw_lanes_keep_the_same_rational_quotient() {
    let geometry = line(3, true);
    let SolvedCurveGeometry::Nurbs(curve) = &geometry else { unreachable!() };
    let points = [0.0, 0.5, 1.0].map(|x| FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap());
    let weights = [1.0; 3];
    let scratch = decode::Scratch::new(super::super::admission::EvaluationAdmission::Standard);
    for order in [CurveDerivative::First, CurveDerivative::Second] {
        assert_eq!(derivative(&scratch, 2, curve.knots(), DerivativePoles::Stored(curve.pole_rows()),
            FiniteReal::HALF, order),
            derivative(&scratch, 2, curve.knots(), DerivativePoles::Lanes { points: &points, weights: Some(&weights) },
                FiniteReal::HALF, order));
    }
}
