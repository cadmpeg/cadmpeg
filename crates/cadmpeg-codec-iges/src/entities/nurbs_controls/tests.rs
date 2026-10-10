// SPDX-License-Identifier: Apache-2.0
//! Exact tensor-product restriction keeps the original parameter chart.
#![allow(clippy::unwrap_used)]
use super::{homogeneous_control_points, insert_homogeneous_knot, trim_surface};
use cadmpeg_core::decode::{
    index_from_u32, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::{admission::EvaluationAdmission, decode::nurbs_surface_point};
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::math::Point3;

const EPS_TRIM_POINT: f64 = 1.0e-12;

fn rational_surface() -> NurbsSurface {
    NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            (0..4)
                .map(|u| {
                    (0..3)
                        .map(|v| Point3::new(f64::from(u), f64::from(v), f64::from(u * v)))
                        .collect()
                })
                .collect(),
            Some(vec![
                vec![1.0, 1.5, 1.0],
                vec![2.0, 1.0, 0.5],
                vec![1.0, 2.0, 1.0],
                vec![0.5, 1.0, 2.0],
            ]),
        ),
        true,
    )
    .unwrap()
    .unwrap()
}

#[test]
fn rational_multispan_surface_crop_preserves_points_and_normal_chart() {
    let source = rational_surface();
    let cropped = trim_surface(
        &cadmpeg_test_support::service_decode_context(),
        &source,
        [[0.2, 0.8], [0.1, 0.9]],
    )
    .unwrap()
    .unwrap();
    assert!(cropped.normal_reversed());
    assert_eq!(cropped.u_knots()[index_from_u32(cropped.u_degree())], 0.2);
    assert_eq!(cropped.u_knots()[cropped.u_count()], 0.8);
    assert_eq!(cropped.v_knots()[index_from_u32(cropped.v_degree())], 0.1);
    assert_eq!(cropped.v_knots()[cropped.v_count()], 0.9);
    assert!(cropped.weights().is_some());
    for u in [0.2, 0.35, 0.5, 0.65, 0.8] {
        for v in [0.1, 0.4, 0.7, 0.9] {
            let before = nurbs_surface_point(EvaluationAdmission::Standard, &source, u, v).unwrap();
            let after = nurbs_surface_point(EvaluationAdmission::Standard, &cropped, u, v).unwrap();
            assert!(
                before.get().distance(after.get()) <= EPS_TRIM_POINT,
                "{u}, {v}"
            );
        }
    }
}

#[test]
fn surface_crop_refuses_ranges_outside_the_basis() {
    let source = rational_surface();
    let ctx = cadmpeg_test_support::service_decode_context();
    for ranges in [
        [[0.0, 1.1], [0.0, 1.0]],
        [[0.0, 1.0], [-0.1, 1.0]],
        [[0.4, 0.4], [0.0, 1.0]],
    ] {
        assert!(trim_surface(&ctx, &source, ranges).unwrap().is_none());
    }
}

#[test]
fn surface_crop_propagates_allocation_limits() {
    let source = rational_surface();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(trim_surface(&ctx, &source, [[0.2, 0.8], [0.1, 0.9]]), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn surface_crop_refuses_a_discontinuous_terminal_boundary() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let source = NurbsSurface::from_lanes(
        &ctx,
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            (0..4)
                .map(|u| {
                    vec![
                        Point3::new(f64::from(u), 0.0, 0.0),
                        Point3::new(f64::from(u), 1.0, 0.0),
                    ]
                })
                .collect(),
            None,
        ),
        false,
    )
    .unwrap()
    .unwrap();
    let source_endpoint =
        nurbs_surface_point(EvaluationAdmission::Standard, &source, 0.5, 0.5).unwrap();
    assert_eq!(source_endpoint.get(), Point3::new(2.0, 0.5, 0.0));
    assert!(trim_surface(&ctx, &source, [[0.0, 0.5], [0.0, 1.0]])
        .unwrap()
        .is_none());
}

#[test]
fn homogeneous_control_points_refuse_collection_limit() {
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("fixture constructor admission")
    .expect("valid test NURBS");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = homogeneous_control_points(&ctx, &curve).unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 2
    ));
}

#[test]
fn composite_knot_insertion_refuses_knot_collection_limit() {
    let controls = [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
    let knots = [0.0, 0.0, 1.0, 1.0];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = insert_homogeneous_knot(&ctx, &controls, &knots, 1, 0.5).unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 5
    ));
}

#[test]
fn composite_knot_insertion_refuses_control_collection_limit() {
    let controls = [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
    let knots = [0.0, 0.0, 1.0, 1.0];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = insert_homogeneous_knot(&ctx, &controls, &knots, 1, 0.5).unwrap_err();
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 5
                && limit.additional == 3
    ));
}
