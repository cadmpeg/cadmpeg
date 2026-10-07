// SPDX-License-Identifier: Apache-2.0

use crate::geometry::nurbs::{KnotVector, NurbsError};
use crate::geometry::pcurve::{
    PcurveNurbs, PcurveNurbsPoles, PolarNurbsPole, PolarNurbsPoles, PolarPcurveNurbs,
    WeightedPolarNurbsPole, WeightedPole2,
};
use crate::math::Point2;
use crate::scalar::{FiniteReal, NonZeroReal};
use crate::units::FinitePoint2;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn knots() -> Vec<f64> {
    vec![0.0, 0.0, 1.0, 1.0]
}
fn points() -> Vec<Point2> {
    vec![Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)]
}
fn polar_poles() -> Vec<PolarNurbsPole> {
    points()
        .into_iter()
        .enumerate()
        .map(|(index, radial)| PolarNurbsPole {
            radial,
            axial: if index == 0 { 5.0 } else { 6.0 },
        })
        .collect()
}

#[test]
fn pcurve_construction_preserves_original_refusals_for_pairing_conversion_and_knots() {
    for polar in [false, true] {
        for operation in [if polar { "IR polar admitted poles" } else { "IR pcurve admitted poles" },
            "IR NURBS knot finiteness", "IR NURBS knot order"] {
            cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation,
                |cap| crate::geometry::tests::budget::with_limit(ResourceDimension::WorkUnits, cap, |ctx| {
if polar {
                PolarPcurveNurbs::new(
                    ctx,
                    1,
                    knots(),
                    PolarNurbsPoles::Polynomial {
                        poles: polar_poles(),
                    },
                    false,
                )
                .map(|result| result.map(|_| ()))
            } else {
                PcurveNurbs::new(
                    ctx,
                    1,
                    knots(),
                    PcurveNurbsPoles::Polynomial { points: points() },
                    false,
                )
                .map(|result| result.map(|_| ()))
            }
                }));
        }
        for checked in [false, true] {
            let operation = if polar { "IR polar paired poles" } else { "IR pcurve paired poles" };
            cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation,
                |cap| crate::geometry::tests::budget::with_limit(ResourceDimension::WorkUnits, cap, |ctx| {
match (polar, checked) {
                    (false, false) => {
                        PcurveNurbsPoles::from_lanes(ctx, points(), Some(vec![1.0; 2]))
                            .map(|result| result.map(|_| ()))
                    }
                    (false, true) => PcurveNurbsPoles::from_checked_lanes(
                        ctx,
                        points(),
                        Some(vec![NonZeroReal::ONE; 2]),
                    )
                    .map(|result| result.map(|_| ())),
                    (true, false) => {
                        PolarNurbsPoles::from_lanes(ctx, polar_poles(), Some(vec![1.0; 2]))
                            .map(|result| result.map(|_| ()))
                    }
                    (true, true) => PolarNurbsPoles::from_checked_lanes(
                        ctx,
                        polar_poles(),
                        Some(vec![NonZeroReal::ONE; 2]),
                    )
                    .map(|result| result.map(|_| ())),
                }
                }));
        }
    }
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "IR pcurve paired poles",
        |cap| crate::geometry::tests::budget::with_limit(ResourceDimension::WorkUnits, cap, |ctx| {
            PcurveNurbsPoles::from_finite_lanes(ctx, points(), Some(vec![FiniteReal::ONE; 2]))
        }));
}

#[test]
fn pcurve_construction_accounts_pair_storage_final_storage_and_scratch_lifetime() {
    for polar in [false, true] {
        let retained = if polar {
            2 * std::mem::size_of::<WeightedPolarNurbsPole<FinitePoint2, FiniteReal>>()
        } else {
            2 * std::mem::size_of::<WeightedPole2<FinitePoint2>>()
        };
        let temporary = if polar {
            4 * std::mem::size_of::<WeightedPolarNurbsPole>()
        } else {
            4 * std::mem::size_of::<WeightedPole2>()
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(retained).expect("fixture");
        policy.limits.max_materialized_bytes = u64::try_from(temporary).expect("fixture");
        policy.limits.max_collection_items = 4;
        policy.limits.max_work_units = 16;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        if polar {
            let result = PolarPcurveNurbs::from_checked_lanes(
                &ctx,
                1,
                knots(),
                polar_poles(),
                Some(vec![NonZeroReal::ONE; 2]),
                false,
            )
            .expect("exact admissions")
            .expect("valid");
            assert_eq!(result.pole_rows().poles()[1].axial.get(), 6.0);
            let wire = serde_json::to_value(&result).expect("wire");
            assert_eq!(
                serde_json::from_value::<PolarPcurveNurbs>(wire)
                    .expect("context-free reconstruction"),
                result
            );
        } else {
            let result = PcurveNurbs::from_checked_lanes(
                &ctx,
                1,
                knots(),
                points(),
                Some(vec![NonZeroReal::ONE; 2]),
                false,
            )
            .expect("exact admissions")
            .expect("valid");
            assert_eq!(
                result.pole_rows().point_at(1).expect("last pole").get(),
                Point2::new(3.0, 4.0)
            );
            let wire = serde_json::to_value(&result).expect("wire");
            assert_eq!(
                serde_json::from_value::<PcurveNurbs>(wire).expect("context-free reconstruction"),
                result
            );
        }
        let reuse = ctx
            .reserve_scoped(
                u64::try_from(temporary).expect("fixture"),
                "pcurve construction scratch reuse",
            )
            .expect("pair storage released");
        drop(reuse);
        ctx.finish_session().expect("exact resource dimensions");
        for dimension in [
            ResourceDimension::RetainedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::MaterializedBytes,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = if polar {
                PolarPcurveNurbs::from_lanes(
                    &ctx,
                    1,
                    knots(),
                    polar_poles(),
                    Some(vec![1.0; 2]),
                    false,
                )
                .map(|result| result.map(|_| ()))
            } else {
                PcurveNurbs::from_lanes(&ctx, 1, knots(), points(), Some(vec![1.0; 2]), false)
                    .map(|result| result.map(|_| ()))
            };
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("outer allocation refusal");
            };
            assert_eq!(limit.dimension, dimension);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
    }
}

#[test]
fn pcurve_construction_moves_admitted_lanes_and_admits_finite_knot_conversion() {
    let setup = cadmpeg_test_support::service_decode_context();
    let knots = KnotVector::new(&setup, knots())
        .expect("setup admission")
        .expect("knots");
    let knot_address = knots.as_ptr();
    let points: Vec<_> = points()
        .into_iter()
        .map(|point| WeightedPole2 {
            point: FinitePoint2::new(point).expect("finite"),
            weight: NonZeroReal::ONE,
        })
        .collect();
    let pole_address = points.as_ptr();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let built = PcurveNurbs::new(&ctx, 1, knots, PcurveNurbsPoles::Rational { points }, false)
        .expect("move requires no storage or visits")
        .expect("valid");
    let PcurveNurbsPoles::Rational { points } = built.pole_rows() else {
        panic!("rational");
    };
    assert_eq!(points.as_ptr(), pole_address);
    assert_eq!(built.knots().as_ptr(), knot_address);
    ctx.finish_session().expect("no duplicate admission");
    for operation in ["IR finite knot values", "IR NURBS knot order"] {
        cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation,
            |cap| crate::geometry::tests::budget::with_limit(ResourceDimension::WorkUnits, cap, |ctx| {
PcurveNurbs::from_finite_lanes(
            ctx,
            1,
            vec![
                FiniteReal::ZERO,
                FiniteReal::ZERO,
                FiniteReal::ONE,
                FiniteReal::ONE,
            ],
            vec![FinitePoint2::ZERO; 2],
            None,
            false,
        )
            }));
    }
}

#[test]
fn pcurve_construction_admits_diagnostics_without_changing_geometry_refusal_order() {
    let setup = cadmpeg_test_support::service_decode_context();
    let error = PcurveNurbs::from_lanes(
        &setup,
        0,
        Vec::<f64>::new(),
        points(),
        Some(vec![0.0]),
        false,
    )
    .expect("diagnostics admitted")
    .expect_err("lane mismatch precedes weights and degree");
    assert!(matches!(
        error,
        NurbsError::WeightLaneLength {
            poles: 2,
            weights: 1,
            ..
        }
    ));
    let error = PolarPcurveNurbs::from_lanes(
        &setup,
        0,
        Vec::<f64>::new(),
        polar_poles(),
        Some(vec![1.0, 0.0]),
        false,
    )
    .expect("diagnostics admitted")
    .expect_err("weight precedes degree");
    assert!(matches!(error, NurbsError::UnusableWeight { index: 1, .. }));
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::WorkUnits,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(limit)) =
            PcurveNurbs::from_lanes(&ctx, 1, knots(), points(), Some(vec![1.0]), false)
        else {
            panic!("diagnostic admission refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}
