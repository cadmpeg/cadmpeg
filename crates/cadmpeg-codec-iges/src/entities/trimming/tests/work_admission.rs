// SPDX-License-Identifier: Apache-2.0
use super::super::{BoundarySurfaceKind, NonSimpleRing, SimpleRing};
use cadmpeg_core::{
    decode::{DecodePolicy, ResourceDimension},
    CodecError,
};

fn square() -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]]
}

fn assert_work_refusal<T>(result: Result<T, CodecError>, operation: &str) {
    assert!(
        matches!(result.err(), Some(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation)
    );
}

#[test]
fn simple_ring_duplicate_proof_refuses_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 14;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_work_refusal(
            SimpleRing::new(square(), ctx),
            "iges closed polyline duplicate comparisons",
        );
    });
}

#[test]
fn simple_ring_intersection_proof_refuses_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 24;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_work_refusal(
            SimpleRing::new(square(), ctx),
            "iges planar self-intersection comparisons",
        );
    });
}

#[test]
fn trim_relationship_propagates_intersection_work_refusal() {
    let rings = crate::test_support::with_service_context(&[], |ctx| {
        vec![
            SimpleRing::new(square(), ctx).unwrap().unwrap(),
            SimpleRing::new(
                vec![[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0], [1.0, 1.0]],
                ctx,
            )
            .unwrap()
            .unwrap(),
        ]
    });
    let plane = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_work_refusal(
            super::super::linear_boundary_relationship_is_valid(
                Ok(&rings),
                BoundarySurfaceKind::Trimmed,
                true,
                &plane,
                None,
                [false, false],
                ctx,
            ),
            "iges planar ring intersection comparisons",
        );
    });
    crate::test_support::with_service_context(&[], |ctx| {
        assert_eq!(
            super::super::linear_boundary_relationship_is_valid(
                Ok(&rings),
                BoundarySurfaceKind::Trimmed,
                true,
                &plane,
                None,
                [false, false],
                ctx
            )
            .unwrap(),
            Some(true)
        );
        assert_eq!(
            super::super::linear_boundary_relationship_is_valid(
                Err(&NonSimpleRing),
                BoundarySurfaceKind::Trimmed,
                true,
                &plane,
                None,
                [false, false],
                ctx
            )
            .unwrap(),
            Some(false)
        );
    });
}

#[test]
fn trim_containment_proof_refuses_work() {
    let ring = crate::test_support::with_service_context(&[], |ctx| {
        SimpleRing::new(square(), ctx).unwrap().unwrap()
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_work_refusal(
            super::super::planar_point_is_strictly_inside([1.0, 1.0], &ring, ctx),
            "iges planar point containment comparisons",
        );
    });
}

#[test]
fn implicit_outer_ring_relationship_propagates_pair_work_refusal() {
    let rings = crate::test_support::with_service_context(&[], |ctx| {
        vec![
            SimpleRing::new(square(), ctx).unwrap().unwrap(),
            SimpleRing::new(
                square().into_iter().map(|[x, y]| [x + 5.0, y]).collect(),
                ctx,
            )
            .unwrap()
            .unwrap(),
        ]
    });
    let plane = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_work_refusal(
            super::super::linear_boundary_relationship_is_valid(
                Ok(&rings),
                BoundarySurfaceKind::Trimmed,
                false,
                &plane,
                None,
                [false, false],
                ctx,
            ),
            "iges planar ring intersection comparisons",
        );
    });
}

#[test]
fn boundary_clustering_propagates_root_work_refusal() {
    let points = [
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
            .unwrap(),
    ];
    let mut policy = DecodePolicy::service();
    // Two parent initialization visits and one size fill precede the root read.
    policy.limits.max_work_units = 2 + 1;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        let result = super::super::cluster_boundary_positions(
            &points,
            cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
            ctx,
        );
        assert!(matches!(result,
            Err(super::super::BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(limit)))
                if limit.operation == "iges boundary cluster root traversal"
                    && ctx.resource_refusal() == Some(limit)));
    });
}

fn assert_scan_work_refusal<T>(
    operation: &str,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    let mut cap = 0_u64;
    for _ in 0..128 {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let ctx = cadmpeg_core::decode::DecodeContext::new(&arena, &policy, false);
        match run(&ctx) {
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
                if limit.operation == operation {
                    return;
                }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            Err(error) => panic!("unexpected error before {operation}: {error}"),
            Ok(_) => panic!("operation {operation} was not admitted"),
        }
    }
    panic!("operation {operation} was not reached");
}

#[test]
fn pcurve_internal_multiplicity_refuses_work_before_scan() {
    let controls = [[1.0, 0.0, 0.0, 0.0]; 4];
    let knots = [0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0];
    assert_scan_work_refusal("iges pcurve internal knot multiplicity", |ctx| {
        super::super::homogeneous_pcurve_spans(2, &knots, controls.to_vec(), ctx)
    });
}

#[test]
fn pcurve_split_refuses_work_before_level_creation() {
    let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
    assert_scan_work_refusal("iges pcurve split levels", |ctx| {
        super::super::split_homogeneous_pcurve(&controls, 0.5, ctx)
    });
}
