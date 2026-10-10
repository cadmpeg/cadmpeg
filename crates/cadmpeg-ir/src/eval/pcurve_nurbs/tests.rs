// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::geometry::pcurve::{PcurveGeometry, PcurveNurbs, PolarNurbsPole, PolarPcurveNurbs};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn large_line(polar: bool, rational: bool) -> PcurveGeometry {
    let count = 1024_u32;
    let end = f64::from(count - 2);
    let knots: Vec<_> = [0.0; 3].into_iter().chain((1..count - 2).map(f64::from))
        .chain([end; 3]).collect();
    let values = (0..count).map(|index| {
        let index = usize::try_from(index).unwrap();
        knots[index + 1].midpoint(knots[index + 2])
    });
    let weights = rational.then(|| (0..count).map(|_| 1.0).collect());
    // The actual constructors require a context; fixture setup precedes the
    // evaluation session. This does not certify Standard construction.
    let ctx = cadmpeg_test_support::service_decode_context();
    if polar {
        PcurveGeometry::PolarNurbs { nurbs: PolarPcurveNurbs::from_lanes(&ctx, 2, knots.clone(),
            values.map(|t| PolarNurbsPole { radial: Point2::new(1.0, t), axial: t }).collect(),
            weights, false).unwrap().unwrap() }
    } else {
        PcurveGeometry::Nurbs { nurbs: PcurveNurbs::from_lanes(&ctx, 2, knots.clone(),
            values.map(|t| Point2::new(t, 0.0)).collect(), weights, false).unwrap().unwrap() }
    }
}

#[test]
fn stored_planar_and_polar_differentials_borrow_large_paired_weight_rows() {
    for polar in [false, true] {
        for rational in [false, true] {
            let geometry = large_line(polar, rational);
            let mut policy = DecodePolicy::service();
            // Each local degree2 differential collects3+3+2+3 items. The
            // polar form evaluates the radial and axial lanes in that order.
            policy.limits.max_collection_items = if polar { 22 } else { 11 };
            // All captured local basis reservations fit512 bytes. The old
            // full1024-weight copy alone exceeded this cap (8192 bytes).
            policy.limits.max_materialized_bytes = 512;
            policy.limits.max_retained_bytes = 0;
            // Span<=10 plus local basis/sum/inspection<=107 per lane.
            policy.limits.max_work_units = 256;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let point = crate::eval::decode::pcurve_uv(&ctx, &geometry, 0.5).unwrap();
            let expected = if polar { Point2::new(0.5_f64.atan(), 0.5) }
                else { Point2::new(0.5, 0.0) };
            assert_eq!(point.get(), expected);
            drop(ctx.reserve_scoped_limit(512, "borrowed pcurve scope released").unwrap());
            ctx.finish_session().unwrap();
            let point = crate::eval::decode::pcurve_uv(super::super::admission::EvaluationAdmission::Standard,
                &geometry, 0.5).unwrap();
            assert_eq!(point.get(), expected);
        }
    }
}

#[test]
fn stored_planar_and_polar_basis_refusals_keep_original_error_and_order() {
    for polar in [false, true] {
        let geometry = large_line(polar, true);
        for cap in [2, 5, 10] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            policy.limits.max_retained_bytes = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let Err(EvaluationFailure::ResourceLimit(original)) =
                crate::eval::decode::pcurve_uv(&ctx, &geometry, 0.5)
                else { panic!("original session must refuse the next basis collection"); };
            assert_eq!(original.dimension, ResourceDimension::CollectionItems);
            assert_eq!(original.operation, match cap { 2 => "IR B-spline basis",
                5 => "IR B-spline derivative basis", 10 => "IR B-spline second derivative basis",
                _ => unreachable!() });
            assert_eq!(crate::eval::decode::pcurve_uv(&ctx, &geometry, f64::NAN),
                Err(EvaluationFailure::ResourceLimit(original)));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
        }
    }
}

#[test]
fn raw_differential_admits_only_reached_points_and_keeps_missing_weight_default() {
    let scratch = decode::Scratch::new(super::super::admission::EvaluationAdmission::Standard);
    let points = [Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), Point2::new(f64::NAN, 0.0)];
    for weights in [None, Some(&[1.0][..])] {
        let poles = DifferentialPoles::Raw { points: &points, weights };
        let value = differential(&scratch, 1, &[0.0, 0.0, 1.0, 2.0, 2.0], poles, FiniteReal::new(0.25).unwrap()).unwrap();
        assert_eq!(value.point.get(), Point2::new(0.25, 0.0));
        assert_eq!(value.tangent.unwrap().get(), Point2::new(1.0, 0.0));
        assert_eq!(value.acceleration.unwrap().get(), Point2::new(0.0, 0.0));
        assert!(matches!(differential(&scratch, 1, &[0.0, 0.0, 1.0, 2.0, 2.0], poles,
            FiniteReal::new(1.5).unwrap()), Err(EvaluationFailure::NoValue)));
    }
}

mod higher;
