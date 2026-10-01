// SPDX-License-Identifier: Apache-2.0
use super::super::{SimpleRing, NonSimpleRing, BoundarySurfaceKind};
use cadmpeg_core::{CodecError, decode::{DecodePolicy, ResourceDimension}};

fn square() -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]]
}

fn assert_work_refusal<T>(result: Result<T, CodecError>, operation: &str) {
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn simple_ring_duplicate_proof_refuses_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 14;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_work_refusal(SimpleRing::new(square(), ctx), "iges closed polyline duplicate comparisons");
    });
}

#[test]
fn simple_ring_intersection_proof_refuses_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 24;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_work_refusal(SimpleRing::new(square(), ctx), "iges planar self-intersection comparisons");
    });
}

#[test]
fn trim_relationship_propagates_intersection_work_refusal() {
    let rings = crate::test_support::with_service_context(&[], |ctx| {
        vec![SimpleRing::new(square(), ctx).unwrap().unwrap(),
            SimpleRing::new(vec![[1.0,1.0],[2.0,1.0],[2.0,2.0],[1.0,2.0],[1.0,1.0]], ctx).unwrap().unwrap()]
    });
    let plane = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0,0.0,0.0),
                cadmpeg_ir::math::Vector3::new(0.0,0.0,1.0),
                cadmpeg_ir::math::Vector3::new(1.0,0.0,0.0)).unwrap()));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_work_refusal(super::super::linear_boundary_relationship_is_valid(
            Ok(&rings), BoundarySurfaceKind::Trimmed, true, &plane, None, [false,false], ctx),
            "iges planar ring intersection comparisons");
    });
    crate::test_support::with_service_context(&[], |ctx| {
        assert_eq!(super::super::linear_boundary_relationship_is_valid(
            Ok(&rings), BoundarySurfaceKind::Trimmed, true, &plane, None, [false,false], ctx).unwrap(), Some(true));
        assert_eq!(super::super::linear_boundary_relationship_is_valid(
            Err(&NonSimpleRing), BoundarySurfaceKind::Trimmed, true, &plane, None, [false,false], ctx).unwrap(), Some(false));
    });
}

#[test]
fn trim_containment_proof_refuses_work() {
    let ring = crate::test_support::with_service_context(&[], |ctx| SimpleRing::new(square(), ctx).unwrap().unwrap());
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_work_refusal(super::super::planar_point_is_strictly_inside([1.0,1.0], &ring, ctx),
            "iges planar point containment comparisons");
    });
}

#[test]
fn implicit_outer_ring_relationship_propagates_pair_work_refusal() {
    let rings = crate::test_support::with_service_context(&[], |ctx| {
        vec![SimpleRing::new(square(), ctx).unwrap().unwrap(),
            SimpleRing::new(square().into_iter().map(|[x,y]| [x+5.0,y]).collect(), ctx).unwrap().unwrap()]
    });
    let plane = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0,0.0,0.0),
                cadmpeg_ir::math::Vector3::new(0.0,0.0,1.0),
                cadmpeg_ir::math::Vector3::new(1.0,0.0,0.0)).unwrap()));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_work_refusal(super::super::linear_boundary_relationship_is_valid(
            Ok(&rings), BoundarySurfaceKind::Trimmed, false, &plane, None, [false,false], ctx),
            "iges planar ring intersection comparisons");
    });
}

#[test]
fn boundary_clustering_propagates_root_work_refusal() {
    let points = [cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0,0.0,0.0)).unwrap()];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        let result = super::super::cluster_boundary_positions(&points,
            cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),ctx);
        assert!(matches!(result,
            Err(super::super::BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(limit)))
                if limit.operation == "iges boundary cluster root traversal"
                    && ctx.resource_refusal() == Some(limit)));
    });
}
