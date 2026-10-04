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
    // Two pole yields plus their terminal probe, four finite knots, and three pairs total 10.
    for (cap, operation) in [
        (0, "IR NURBS admitted poles"),
        (1, "IR NURBS admitted poles"),
        (2, "IR NURBS admitted poles"),
        (3, "IR NURBS knot finiteness"),
        (6, "IR NURBS knot finiteness"),
        (7, "IR NURBS knot order"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
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
            panic!("original outer refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, operation);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
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
    // Two shape visits, nine nested collection probes, and fourteen knot scans total 25.
    for (cap, operation) in [
        (0, "IR NURBS grid row shape"),
        (1, "IR NURBS grid row shape"),
        (2, "IR NURBS admitted grid rows"),
        (3, "IR NURBS admitted poles"),
        (4, "IR NURBS admitted poles"),
        (5, "IR NURBS admitted poles"),
        (6, "IR NURBS admitted grid rows"),
        (7, "IR NURBS admitted poles"),
        (8, "IR NURBS admitted poles"),
        (9, "IR NURBS admitted poles"),
        (10, "IR NURBS admitted grid rows"),
        (11, "IR NURBS knot finiteness"),
        (12, "IR NURBS knot finiteness"),
        (15, "IR NURBS knot order"),
        (16, "IR NURBS knot order"),
        (19, "IR NURBS knot finiteness"),
        (20, "IR NURBS knot finiteness"),
        (23, "IR NURBS knot order"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(limit)) = NurbsSurface::new(
            &ctx,
            NurbsSurfaceAxis::new(1, knots(), false),
            NurbsSurfaceAxis::new(1, knots(), false),
            NurbsPoleGrid::Polynomial {
                rows: vec![raw_points(), raw_points()],
            },
            false,
        ) else {
            panic!("outer row, pole or knot refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, operation);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
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
    // Four finite-lane values use five collection probes, then three order comparisons: eight.
    for (cap, operation) in [
        (0, "IR finite knot values"),
        (1, "IR finite knot values"),
        (2, "IR finite knot values"),
        (3, "IR finite knot values"),
        (4, "IR finite knot values"),
        (5, "IR NURBS knot order"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(limit)) =
            NurbsCurve::from_finite_lanes(&ctx, 1, finite_knots(), finite_points(), None, false)
        else {
            panic!("finite lane admission must use caller");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, operation);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    // Two shape visits, ten knot conversion probes, and six order comparisons total 18.
    for (cap, operation) in [
        (0, "IR NURBS grid row shape"),
        (1, "IR NURBS grid row shape"),
        (2, "IR finite knot values"),
        (6, "IR finite knot values"),
        (7, "IR NURBS knot order"),
        (10, "IR finite knot values"),
        (11, "IR finite knot values"),
        (14, "IR finite knot values"),
        (15, "IR NURBS knot order"),
        (16, "IR NURBS knot order"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(limit)) = NurbsSurface::from_finite_lanes(
            &ctx,
            NurbsSurfaceAxis::new(1, finite_knots(), false),
            NurbsSurfaceAxis::new(1, finite_knots(), false),
            NurbsSurfaceLanes::new(vec![finite_points(), finite_points()], None),
            false,
        ) else {
            panic!("finite grid knot admission must use caller");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, operation);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
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
    // Curves cost 2 pairing + 3 pole probes + 7 knot checks = 12; surfaces cost 6 + 2 + 9 + 14 = 31.
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
        policy.limits.max_work_units = if surface { 31 } else { 12 };
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        if surface {
            let refusal_arena = DecodeArena::new();
            let mut refusal_policy = DecodePolicy::service();
            refusal_policy.limits.max_retained_bytes =
                u64::try_from(retained).expect("small fixture");
            refusal_policy.limits.max_materialized_bytes =
                u64::try_from(temporary).expect("small fixture");
            refusal_policy.limits.max_collection_items = 12;
            refusal_policy.limits.max_work_units = 30;
            let (refusal_ctx, _) =
                DecodeContext::from_root_bytes(&[], &refusal_arena, &refusal_policy).expect("root");
            let Err(CodecError::ResourceLimit(limit)) = NurbsSurface::from_checked_lanes(
                &refusal_ctx,
                NurbsSurfaceAxis::new(1, knots(), false),
                NurbsSurfaceAxis::new(1, knots(), false),
                NurbsSurfaceLanes::new(
                    vec![raw_points(), raw_points()],
                    Some(vec![vec![NonZeroReal::ONE; 2]; 2]),
                ),
                false,
            ) else {
                panic!("cap 30 must refuse the final knot comparison");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "IR NURBS knot order");
            assert_eq!(limit.used, 30);
            assert_eq!(limit.additional, 1);
            assert!(matches!(
                refusal_ctx.finish_session(),
                Err(CodecError::ResourceLimit(sticky)) if sticky == limit
            ));
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
