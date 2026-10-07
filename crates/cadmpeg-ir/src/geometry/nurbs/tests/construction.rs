// SPDX-License-Identifier: Apache-2.0

use crate::features::FinitePoint3;
use crate::geometry::nurbs::{
    KnotVector, NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
    NurbsSurfaceLanes, WeightedPole3,
};
use crate::math::Point3;
use crate::scalar::{FiniteReal, NonZeroReal};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn knots() -> Vec<f64> {
    vec![0.0, 0.0, 1.0, 1.0]
}
fn finite_knots() -> Vec<FiniteReal> {
    vec![
        FiniteReal::ZERO,
        FiniteReal::ZERO,
        FiniteReal::ONE,
        FiniteReal::ONE,
    ]
}
fn raw_points() -> Vec<Point3> {
    vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)]
}
fn finite_points() -> Vec<FinitePoint3> {
    raw_points()
        .into_iter()
        .map(|point| FinitePoint3::new(point).expect("finite"))
        .collect()
}

#[test]
fn final_nurbs_construction_admits_raw_conversion_and_all_knot_visits() {
    for operation in ["IR NURBS admitted poles", "IR NURBS knot finiteness", "IR NURBS knot order"] {
        cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation,
            |cap| crate::geometry::tests::budget::with_limit(ResourceDimension::WorkUnits, cap, |ctx| {
                NurbsCurve::new(ctx, 1, knots(), NurbsPoles3::Polynomial { points: raw_points() }, false)
            }));
    }
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(limit)) = NurbsCurve::new(
            &ctx,
            1,
            knots(),
            NurbsPoles3::Polynomial {
                points: raw_points(),
            },
            false,
        ) else {
            panic!("final pole storage refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, "IR NURBS admitted poles");
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    for operation in ["IR NURBS grid row shape", "IR NURBS admitted grid rows", "IR NURBS admitted poles",
        "IR NURBS knot finiteness", "IR NURBS knot order"] {
        cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation,
            |cap| crate::geometry::tests::budget::with_limit(ResourceDimension::WorkUnits, cap, |ctx| {
                NurbsSurface::new(ctx, NurbsSurfaceAxis::new(1, knots(), false),
                    NurbsSurfaceAxis::new(1, knots(), false),
                    NurbsPoleGrid::Polynomial { rows: vec![raw_points(), raw_points()] }, false)
            }));
    }
}

#[test]
fn final_nurbs_construction_moves_admitted_storage_without_copy_or_scalar_readmission() {
    let setup = cadmpeg_test_support::service_decode_context();
    let curve_knots = KnotVector::new(&setup, knots())
        .expect("setup")
        .expect("knots");
    let knot_address = curve_knots.as_ptr();
    let points = finite_points();
    let point_address = points.as_ptr();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let curve = NurbsCurve::new(
        &ctx,
        1,
        curve_knots,
        NurbsPoles3::Polynomial { points },
        false,
    )
    .expect("owned admitted storage")
    .expect("valid curve");
    assert_eq!(curve.knots().as_ptr(), knot_address);
    let NurbsPoles3::Polynomial { points } = curve.pole_rows() else {
        panic!("polynomial");
    };
    assert_eq!(points.as_ptr(), point_address);
    ctx.finish_session()
        .expect("no input-sized operation needed");
    let u_knots = KnotVector::new(&setup, knots())
        .expect("setup")
        .expect("knots");
    let v_knots = KnotVector::new(&setup, knots())
        .expect("setup")
        .expect("knots");
    let rows = vec![finite_points(), finite_points()];
    let outer = rows.as_ptr();
    let inner = rows[0].as_ptr();
    let u_address = u_knots.as_ptr();
    let v_address = v_knots.as_ptr();
    let arena = DecodeArena::new();
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let surface = NurbsSurface::new(
        &ctx,
        NurbsSurfaceAxis::new(1, u_knots, false),
        NurbsSurfaceAxis::new(1, v_knots, false),
        NurbsPoleGrid::Polynomial { rows },
        true,
    )
    .expect("two row-width visits")
    .expect("valid surface");
    let NurbsPoleGrid::Polynomial { rows } = surface.pole_grid() else {
        panic!("polynomial");
    };
    assert_eq!(rows.as_ptr(), outer);
    assert_eq!(rows[0].as_ptr(), inner);
    assert_eq!(surface.u_knots().as_ptr(), u_address);
    assert_eq!(surface.v_knots().as_ptr(), v_address);
    ctx.finish_session().expect("exact row-width work");
    let wire = serde_json::to_value(&surface).expect("wire");
    assert_eq!(
        serde_json::from_value::<NurbsSurface>(wire).expect("context-free reconstruction"),
        surface
    );
}

#[test]
fn finite_geometry_construction_admits_final_knot_conversion_in_the_caller() {
    for operation in ["IR finite knot values", "IR NURBS knot order"] {
        cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation,
            |cap| crate::geometry::tests::budget::with_limit(ResourceDimension::WorkUnits, cap, |ctx| {
                NurbsCurve::from_finite_lanes(ctx, 1, finite_knots(), finite_points(), None, false)
            }));
    }
    for operation in ["IR NURBS grid row shape", "IR finite knot values", "IR NURBS knot order"] {
        cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation,
            |cap| crate::geometry::tests::budget::with_limit(ResourceDimension::WorkUnits, cap, |ctx| {
                NurbsSurface::from_finite_lanes(ctx,
                    NurbsSurfaceAxis::new(1, finite_knots(), false),
                    NurbsSurfaceAxis::new(1, finite_knots(), false),
                    NurbsSurfaceLanes::new(vec![finite_points(), finite_points()], None), false)
            }));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let Err(CodecError::ResourceLimit(limit)) =
        NurbsCurve::from_finite_lanes(&ctx, 1, finite_knots(), finite_points(), None, false)
    else {
        panic!("retained final knot copy must refuse");
    };
    assert_eq!(limit.operation, "IR finite knot values");
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn checked_geometry_construction_releases_raw_pairing_and_keeps_only_final_storage() {
    // Curves use 4 pairing visits, 3 pole probes and 9 knot probes.
    // Surfaces use 12 pairing visits, 2 shape probes, 9 pole probes and 18 knot probes.
    for surface in [false, true] {
        let rows = if surface { 2 } else { 0 };
        let poles = if surface { 4 } else { 2 };
        let retained = rows * std::mem::size_of::<Vec<WeightedPole3<FinitePoint3>>>()
            + poles * std::mem::size_of::<WeightedPole3<FinitePoint3>>();
        let temporary = if surface {
            4 * std::mem::size_of::<Vec<WeightedPole3<Point3>>>()
                + 8 * std::mem::size_of::<WeightedPole3<Point3>>()
        } else {
            4 * std::mem::size_of::<WeightedPole3<Point3>>()
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(retained).expect("small fixture");
        policy.limits.max_materialized_bytes = u64::try_from(temporary).expect("small fixture");
        policy.limits.max_collection_items = if surface { 12 } else { 4 };
        policy.limits.max_work_units = if surface { 41 } else { 16 };
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        if surface {
            let mut refusal_policy = policy.clone();
            refusal_policy.limits.max_work_units = u64::MAX;
            cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "IR NURBS knot order",
                |cap| crate::geometry::tests::budget::with_policy(ResourceDimension::WorkUnits, cap,
                    refusal_policy.clone(), |ctx| NurbsSurface::from_checked_lanes(ctx,
                        NurbsSurfaceAxis::new(1, knots(), false), NurbsSurfaceAxis::new(1, knots(), false),
                        NurbsSurfaceLanes::new(vec![raw_points(), raw_points()], Some(vec![vec![NonZeroReal::ONE; 2]; 2])), false)));
            let built = NurbsSurface::from_checked_lanes(
                &ctx,
                NurbsSurfaceAxis::new(1, knots(), false),
                NurbsSurfaceAxis::new(1, knots(), false),
                NurbsSurfaceLanes::new(
                    vec![raw_points(), raw_points()],
                    Some(vec![vec![NonZeroReal::ONE; 2]; 2]),
                ),
                false,
            )
            .expect("exact admitted output and scoped scratch")
            .expect("valid surface");
            assert_eq!(
                built.pole(1, 1).expect("last pole").get(),
                Point3::new(4.0, 5.0, 6.0)
            );
        } else {
            let built = NurbsCurve::from_checked_lanes(
                &ctx,
                1,
                knots(),
                raw_points(),
                Some(vec![NonZeroReal::ONE; 2]),
                false,
            )
            .expect("exact admitted output and scoped scratch")
            .expect("valid curve");
            assert_eq!(
                built.pole_rows().point_at(1).expect("last pole").get(),
                Point3::new(4.0, 5.0, 6.0)
            );
        }
        let reuse = ctx
            .reserve_scoped(
                u64::try_from(temporary).expect("small fixture"),
                "construction scratch reuse",
            )
            .expect("pairing reservation released at return");
        drop(reuse);
        ctx.finish_session().expect("exact construction admissions");
    }
}
