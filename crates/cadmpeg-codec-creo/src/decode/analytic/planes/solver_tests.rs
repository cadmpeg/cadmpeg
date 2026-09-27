use super::solve_carriers_with_diagnostics;
use crate::decode::analytic::equations::{CarrierEquation, PlaneEquation, SphereEquation};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn tangent_pair() -> [CarrierEquation; 2] {
    [
        CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 1.0],
            normal: [0.0, 0.0, 1.0],
        }),
        CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 1.0,
        }),
    ]
}

fn orthogonal_planes() -> [CarrierEquation; 3] {
    [
        CarrierEquation::Plane(PlaneEquation {
            origin: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        }),
        CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 2.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        }),
        CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 3.0],
            normal: [0.0, 0.0, 1.0],
        }),
    ]
}

fn limit_error(carriers: &[CarrierEquation], limit: u64, operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = solve_carriers_with_diagnostics(&ctx, carriers)
        .expect_err("carrier candidates exceed the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn carrier_pair_candidates_refuse_collection_limit() {
    limit_error(&tangent_pair(), 0, "creo carrier pair candidates");
}

#[test]
fn carrier_triple_groups_refuse_collection_limit() {
    limit_error(&orthogonal_planes(), 0, "creo carrier triple groups");
}

#[test]
fn carrier_triple_candidates_refuse_collection_limit() {
    limit_error(&orthogonal_planes(), 3, "creo carrier triple candidates");
}

#[test]
fn carrier_unique_candidates_refuse_collection_limit() {
    limit_error(&tangent_pair(), 1, "creo carrier unique candidates");
}

#[test]
fn carrier_solver_keeps_pair_and_triple_solutions_under_service_policy() {
    crate::decode::with_test_decode_ctx(|ctx| {
        assert_eq!(
            solve_carriers_with_diagnostics(ctx, &tangent_pair())?.0,
            Some([0.0, 0.0, 1.0])
        );
        assert_eq!(
            solve_carriers_with_diagnostics(ctx, &orthogonal_planes())?.0,
            Some([1.0, 2.0, 3.0])
        );
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("service profile admits both carrier witnesses");
}
