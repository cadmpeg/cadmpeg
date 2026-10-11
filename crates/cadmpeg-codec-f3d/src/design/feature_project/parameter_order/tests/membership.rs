// SPDX-License-Identifier: Apache-2.0
use crate::design::feature_project::test_support::assert_original_refusal;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

const CYCLE_MEMBERSHIP: &str = "visit F3D parameter cycle self-edge membership";

fn parameter(id: &str, dependencies: &[&str]) -> cadmpeg_ir::features::DesignParameter {
    serde_json::from_value(serde_json::json!({
        "id": id, "name": "A", "expression": "A", "dependencies": dependencies,
    }))
    .unwrap()
}

#[test]
fn membership_parameter_self_edge_keeps_original_refusal() {
    let parameters = [parameter("f3d:test:parameter#A", &["f3d:test:parameter#A"])];
    let original = parameters.clone();
    let unresolved = std::collections::BTreeSet::from([0]);
    assert_original_refusal(CYCLE_MEMBERSHIP, |ctx| {
        super::super::cyclic_parameter_components(ctx, &parameters, &unresolved)
    });
    assert_eq!(parameters, original);
}

#[test]
fn membership_parameter_self_edge_keeps_exact_component() {
    let parameters = [parameter("f3d:test:parameter#A", &["f3d:test:parameter#A"])];
    let original = parameters.clone();
    let unresolved = std::collections::BTreeSet::from([0]);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            super::super::cyclic_parameter_components(ctx, &parameters, &unresolved)
                .unwrap(),
            [(0, Vec::<usize>::new())]
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(parameters, original);
}

#[test]
fn membership_parameter_empty_edges_skip_self_edge_query() {
    let parameters = [parameter("f3d:test:parameter#A", &[])];
    let unresolved = std::collections::BTreeSet::from([0]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, CYCLE_MEMBERSHIP, None);
        assert!(
            super::super::cyclic_parameter_components(ctx, &parameters, &unresolved)
                .unwrap()
                .is_empty()
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn membership_parameter_multi_node_cycle_skips_unused_self_edge_query() {
    let parameters = [
        parameter("f3d:test:parameter#A", &["f3d:test:parameter#B"]),
        parameter("f3d:test:parameter#B", &["f3d:test:parameter#A"]),
    ];
    let original = parameters.clone();
    let unresolved = std::collections::BTreeSet::from([0, 1]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, CYCLE_MEMBERSHIP, None);
        let cycles =
            super::super::cyclic_parameter_components(ctx, &parameters, &unresolved)
                .unwrap();
        let [(root, others)] = cycles.as_slice() else {
            panic!("one actual strongly connected component");
        };
        assert_eq!(others.len(), 1);
        assert_ne!(*root, others[0]);
        assert!(*root < 2 && others[0] < 2);
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(parameters, original);
}
