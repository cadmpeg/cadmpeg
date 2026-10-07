// SPDX-License-Identifier: Apache-2.0
use super::super::{cluster_boundary_positions, BoundarySurfaceKind, BoundaryVertexClusterError, BoundaryVertexCreationError, NonSimpleRing, SimpleRing};
use cadmpeg_ir::math::Point3;
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
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation, |cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        crate::test_support::with_policy_context(&[], &policy, |ctx| run(ctx))
    });
}

#[test]
fn pcurve_knot_insertion_refuses_work_before_shift() {
    let controls = [[1.0, 0.0, 0.0, 0.0]; 4];
    let knots = [0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0];
    assert_scan_work_refusal("iges pcurve inserted knots", |ctx| {
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

#[test]
fn boundary_clustering_chain_uses_no_call_stack_depth() {
    let m = 256_u32;
    let points: Vec<_> = (0..=m)
        .map(|i| f64::from(i) * 1.5)
        .chain((0..m).rev().map(|i| f64::from(i) * 1.5 + 0.75))
        .map(|x| cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap())
        .collect();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert!(matches!(
            cluster_boundary_positions(
                &points,
                cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
                ctx
            ),
            Err(BoundaryVertexCreationError::Cluster(
                BoundaryVertexClusterError::NonTransitive
            ))
        ));
    });
}

#[test]
fn boundary_clustering_root_walk_refuses_before_traversal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        let result = super::super::find_cluster_root(&mut [0], 0, ctx);
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "iges boundary cluster root traversal"
                && limit.used == 0 && limit.additional == 1)
        );
    });
}


fn assert_identity_parse_refusal<T>(result: Result<T, CodecError>) {
    // No preceding admission; the identity suffix has one digit.
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "iges native identity sequence"
            && limit.used == 0 && limit.additional == 1));
}

#[test]
fn native_identity_sequence_parse_refuses_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_identity_parse_refusal(
            super::super::native_sequence_from_id("iges:model:surface#D1", "iges:model:surface#D", ctx),
        );
    });
}

#[test]
fn support_interval_identity_parse_refusal_precedes_missing_record_fallback() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let id = cadmpeg_ir::ids::SurfaceId::mint("iges:model:surface#D1").unwrap();
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        assert_identity_parse_refusal(
            super::super::surface_parameter_bound_intervals(
                Some([None; 4]),
                &id,
                &std::collections::BTreeMap::new(),
                &std::collections::BTreeMap::new(),
                crate::global::RealPrecision { single_significance: 7, double_significance: 15 },
                ctx,
            ),
        );
    });
}

#[test]
fn pcurve_knot_runs_preserve_quadratic_span_controls() {
    let knots = [0.0, 0.0, 0.0, 0.25, 0.75, 1.0, 1.0, 1.0];
    let controls = (0..5).map(|value| [1.0, f64::from(value), 0.0, 0.0]).collect();
    crate::test_support::with_service_context(&[], |ctx| {
        let spans = super::super::homogeneous_pcurve_spans(2, &knots, controls, ctx)
            .unwrap().unwrap();
        assert_eq!(spans.len(), 3);
        let expected = [
            ([0.0, 0.25], [0.0, 1.0, 4.0 / 3.0]),
            ([0.25, 0.75], [4.0 / 3.0, 2.0, 8.0 / 3.0]),
            ([0.75, 1.0], [8.0 / 3.0, 3.0, 4.0]),
        ];
        for (span, (domain, ordinates)) in spans.iter().zip(expected) {
            assert_eq!(span.domain, domain);
            assert_eq!(span.controls.len(), 3);
            for (control, ordinate) in span.controls.iter().zip(ordinates) {
                assert_eq!(control[0], 1.0);
                assert!((control[1] - ordinate).abs() <= 4.0 * f64::EPSILON);
                assert_eq!(&control[2..], &[0.0, 0.0]);
            }
        }
    });
}

#[test]
fn pcurve_knot_validation_stops_at_the_first_invalid_value() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(&[], &policy, |ctx| {
        let knots = [f64::NAN, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0];
        assert!(super::super::homogeneous_pcurve_spans(
            2, &knots, vec![[1.0, 0.0, 0.0, 0.0]; 4], ctx,
        ).unwrap().is_none());
    });
}
