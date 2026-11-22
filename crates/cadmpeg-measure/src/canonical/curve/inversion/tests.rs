// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use super::{CurveInverter, CurvePoints, MAX_CACHED_CURVE_POINTS};
use cadmpeg_ir::eval::{decode::nurbs_curve_point_at, nurbs_curve_parameter_near_point};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;

const FIT_TOLERANCE: f64 = 1e-8;

fn point(x: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap()
}

fn curve(rational: bool) -> NurbsCurve {
    NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        rational.then(|| vec![1.0, 2.0, 1.0]),
        false,
    )
    .unwrap()
}

#[test]
fn cached_boundary_points_keep_inverse_parameter_bits() {
    for rational in [false, true] {
        let curve = curve(rational);
        let geometry = SolvedCurveGeometry::Nurbs(curve.clone());
        let mut inverter = CurveInverter::new(&geometry).unwrap();
        with_policy(DecodePolicy::service(), |ctx| {
            for parameter in [0.125, 0.875, 0.5, 0.25, 0.75, 0.0, 1.0] {
                let target = nurbs_curve_point_at(ctx, &curve, parameter).unwrap().get();
                for seed in [0.0, 1.0, 0.5] {
                    let expected =
                        nurbs_curve_parameter_near_point(ctx, &curve, target, FIT_TOLERANCE, seed)
                            .unwrap();
                    let actual = inverter
                        .parameter(ctx, target, FIT_TOLERANCE, seed)
                        .unwrap();
                    assert_eq!(
                        actual.map(|value| value.get().to_bits()),
                        expected.map(|value| value.get().to_bits())
                    );
                    let reached = nurbs_curve_point_at(ctx, &curve, actual.unwrap().get())
                        .unwrap()
                        .get();
                    assert!(reached.distance(target) <= FIT_TOLERANCE);
                }
            }
            let outside = Point3::new(10.0, 10.0, 0.0);
            assert_eq!(
                inverter
                    .parameter(ctx, outside, FIT_TOLERANCE, 0.5)
                    .unwrap(),
                None
            );
        });
        assert!(!inverter.points.values.is_empty());
        assert!(inverter.boundary_index.is_some());
    }
}

#[test]
fn a_warm_cache_keeps_depth_admission() {
    let curve = curve(true);
    let geometry = SolvedCurveGeometry::Nurbs(curve.clone());
    let mut inverter = CurveInverter::new(&geometry).unwrap();
    with_policy(DecodePolicy::service(), |ctx| {
        let target = nurbs_curve_point_at(ctx, &curve, 0.5).unwrap().get();
        assert!(inverter
            .parameter(ctx, target, FIT_TOLERANCE, 0.0)
            .unwrap()
            .is_some());
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    with_policy(policy, |ctx| {
        assert!(matches!(
            inverter.parameter(ctx, Point3::new(0.5, 0.5, 0.0), FIT_TOLERANCE, 0.0),
            Err(CodecError::ResourceLimit(_))
        ));
    });
    with_policy(policy, |ctx| {
        assert!(matches!(
            nurbs_curve_parameter_near_point(
                ctx,
                &curve,
                Point3::new(0.5, 0.5, 0.0),
                FIT_TOLERANCE,
                0.0
            ),
            Err(CodecError::ResourceLimit(_))
        ));
    });
}

#[test]
fn boundary_keys_keep_parameter_bits_and_retry_errors() {
    let mut points = CurvePoints::default();
    let positive = FiniteReal::new(0.0).unwrap();
    let negative = FiniteReal::new(-0.0).unwrap();
    assert_eq!(
        points
            .point::<()>(positive, || Ok(Some(point(1.0))))
            .unwrap(),
        Some(point(1.0))
    );
    assert_eq!(
        points
            .point::<()>(negative, || Ok(Some(point(2.0))))
            .unwrap(),
        Some(point(2.0))
    );
    assert_eq!(
        points
            .point::<()>(positive, || panic!("reuse the positive zero point"))
            .unwrap(),
        Some(point(1.0))
    );
    let other = FiniteReal::new(1.0).unwrap();
    assert!(points.point::<()>(other, || Err(())).is_err());
    assert_eq!(points.point::<()>(other, || Ok(None)).unwrap(), None);
    assert_eq!(
        points
            .point::<()>(other, || panic!("reuse an absent boundary point"))
            .unwrap(),
        None
    );
}

#[test]
fn a_full_boundary_cache_keeps_evaluating_new_parameters() {
    let mut points = CurvePoints::default();
    for ordinal in 0..MAX_CACHED_CURVE_POINTS {
        let parameter = cadmpeg_core::convert::f64_from_index(ordinal)
            .and_then(FiniteReal::new)
            .unwrap();
        points
            .point::<()>(parameter, || Ok(Some(point(0.0))))
            .unwrap();
    }
    let parameter = cadmpeg_core::convert::f64_from_index(MAX_CACHED_CURVE_POINTS)
        .and_then(FiniteReal::new)
        .unwrap();
    let calls = Cell::new(0);
    for _ in 0..2 {
        assert_eq!(
            points
                .point::<()>(parameter, || {
                    calls.set(calls.get() + 1);
                    Ok(Some(point(1.0)))
                })
                .unwrap(),
            Some(point(1.0))
        );
    }
    assert_eq!(calls.get(), 2);
    assert_eq!(points.values.len(), MAX_CACHED_CURVE_POINTS);
}

fn with_policy<T>(policy: DecodePolicy, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    run(&ctx)
}

#[test]
fn a_periodic_upper_seed_keeps_the_stateless_witness_at_depth_one() {
    let curve = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 2.0, 1.0]),
        true,
    )
    .unwrap();
    let geometry = SolvedCurveGeometry::Nurbs(curve.clone());
    let mut inverter = CurveInverter::new(&geometry).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    with_policy(policy, |ctx| {
        for parameter in [0.125, 0.25, 0.5, 0.75, 0.875] {
            let target = nurbs_curve_point_at(ctx, &curve, parameter).unwrap().get();
            for seed in [0.0, 0.5, 1.0] {
                let expected =
                    nurbs_curve_parameter_near_point(ctx, &curve, target, FIT_TOLERANCE, seed)
                        .unwrap();
                let actual = inverter
                    .parameter(ctx, target, FIT_TOLERANCE, seed)
                    .unwrap();
                assert_eq!(
                    actual.map(|value| value.get().to_bits()),
                    expected.map(|value| value.get().to_bits())
                );
                let reached = nurbs_curve_point_at(ctx, &curve, actual.unwrap().get())
                    .unwrap()
                    .get();
                assert!(reached.distance(target) <= FIT_TOLERANCE);
            }
        }
    });
}
