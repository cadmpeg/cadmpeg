// SPDX-License-Identifier: Apache-2.0

use crate::eval::admission::EvaluationAdmission;
use crate::eval::{nurbs_surface_isocurve, nurbs_surface_isoline, IsolineDirection};
use crate::features::FinitePoint3;
use crate::geometry::nurbs::{
    NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes, SurfaceParameterAxis, WeightedPole3,
};
use crate::math::Point3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn surface(rational: bool) -> NurbsSurface {
    NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            rational.then(|| vec![vec![1.0; 2]; 2]),
        ),
        false,
    )
    .unwrap()
    .unwrap()
}

#[test]
fn isocurve_returns_original_refusals_in_every_used_dimension() {
    let surface = surface(true);
    for (dimension, operation) in [
        (
            ResourceDimension::MaterializedBytes,
            "IR surface isoline controls",
        ),
        (
            ResourceDimension::CollectionItems,
            "IR surface isoline controls",
        ),
        (ResourceDimension::RetainedBytes, "IR surface isoline knots"),
        (
            ResourceDimension::WorkUnits,
            "IR surface isoline pole visit",
        ),
    ] {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => unreachable!(),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original =
            nurbs_surface_isocurve(&ctx, &surface, SurfaceParameterAxis::U, 0.5).unwrap_err();
        assert_eq!(original.dimension, dimension);
        assert_eq!(original.operation, operation);
        assert_eq!(
            nurbs_surface_isoline(&ctx, &surface, IsolineDirection::ConstantU, f64::NAN),
            Err(original)
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }
}

#[test]
fn isocurve_admits_actual_copies_and_constructor_visits_once() {
    for rational in [false, true] {
        let surface = surface(rational);
        // Each of two poles visits once and copies its point. Rational
        // output also copies its homogeneous sums and derives weights.
        // Four knots copy eight bytes each, two output poles are converted,
        // and four knot-finiteness visits and three adjacent comparisons.
        let work = 2 * (1 + std::mem::size_of::<Point3>())
            + 32
            + 2
            + 7
            + if rational {
                2 * std::mem::size_of::<super::super::rational::Homogeneous>() + 22 + 2
            } else {
                0
            };
        for allowance in 0..=work {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::try_from(allowance).unwrap();
            policy.limits.max_recursion_depth = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = nurbs_surface_isocurve(&ctx, &surface, SurfaceParameterAxis::U, 0.5);
            if allowance < work {
                let original = result.unwrap_err();
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
                );
            } else {
                let curve = result.unwrap().unwrap();
                assert_eq!(
                    curve.control_points(),
                    [Point3::new(0.5, 0.0, 0.0), Point3::new(0.5, 1.0, 0.0)]
                );
                assert_eq!(curve.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
                assert_eq!(
                    curve.weights().map(|weights| weights
                        .into_iter()
                        .map(crate::scalar::NonZeroReal::get)
                        .collect::<Vec<_>>()),
                    rational.then(|| vec![1.0; 2])
                );
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn isocurve_retains_only_the_output_and_releases_its_scoped_lanes() {
    for rational in [false, true] {
        let surface = surface(rational);
        let pole_size = if rational {
            std::mem::size_of::<WeightedPole3<FinitePoint3>>()
        } else {
            std::mem::size_of::<FinitePoint3>()
        };
        // The retained knot copy reserves four values. The exact-size output
        // constructor reserves two poles, with no retained temporary lanes.
        let retained = 4 * std::mem::size_of::<f64>() + 2 * pole_size;
        let slots = if rational { 14 } else { 8 };
        for short_retained in [false, true] {
            for short_slots in [false, true] {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes =
                    u64::try_from(retained - usize::from(short_retained)).unwrap();
                policy.limits.max_collection_items = slots - u64::from(short_slots);
                policy.limits.max_materialized_bytes = 10_000;
                policy.limits.max_recursion_depth = 0;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = nurbs_surface_isocurve(&ctx, &surface, SurfaceParameterAxis::U, 0.5);
                if short_retained || short_slots {
                    let original = result.unwrap_err();
                    assert!(matches!(
                        original.dimension,
                        ResourceDimension::RetainedBytes | ResourceDimension::CollectionItems
                    ));
                    assert!(
                        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
                    );
                } else {
                    let curve = result.unwrap().unwrap();
                    let storage = ctx
                        .reserve_scoped_limit(10_000, "isocurve scratch released")
                        .unwrap();
                    drop(storage);
                    assert_eq!(
                        curve.control_points(),
                        [Point3::new(0.5, 0.0, 0.0), Point3::new(0.5, 1.0, 0.0)]
                    );
                    assert_eq!(
                        curve,
                        nurbs_surface_isocurve(
                            EvaluationAdmission::Standard,
                            &surface,
                            SurfaceParameterAxis::U,
                            0.5
                        )
                        .unwrap()
                        .unwrap()
                    );
                    let original = ctx
                        .charge_retained_limit(1, "output remains accounted")
                        .unwrap_err();
                    assert_eq!(original.used, u64::try_from(retained).unwrap());
                    assert!(
                        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
                    );
                }
            }
        }
    }
}

#[test]
fn polynomial_isocurve_needs_only_its_position_scratch() {
    let surface = surface(false);
    let point_bytes = 2 * std::mem::size_of::<Point3>();
    for short in [true, false] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes =
            u64::try_from(point_bytes - usize::from(short)).unwrap();
        policy.limits.max_retained_bytes = 80;
        policy.limits.max_collection_items = 8;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = nurbs_surface_isocurve(&ctx, &surface, SurfaceParameterAxis::U, 0.5);
        if short {
            let original = result.unwrap_err();
            assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(original.operation, "IR surface isoline controls");
            assert_eq!(original.used, 0);
            assert_eq!(original.additional, u64::try_from(point_bytes).unwrap());
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
            );
        } else {
            let curve = result.unwrap().unwrap();
            assert_eq!(
                curve.control_points(),
                [Point3::new(0.5, 0.0, 0.0), Point3::new(0.5, 1.0, 0.0)]
            );
            assert!(curve.weights().is_none());
            let storage = ctx
                .reserve_scoped_limit(
                    u64::try_from(point_bytes).unwrap(),
                    "polynomial positions released",
                )
                .unwrap();
            drop(storage);
            ctx.finish_session().unwrap();
        }
    }
}
