// SPDX-License-Identifier: Apache-2.0
//! Fixed scalar equality admits only actual nonempty membership queries.

use super::{assert_original_refusal, fixtures};
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::identity::Located;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_ir::features::PathRef;

const SPLIT_MEMBERSHIP: &str = "visit F3D SplitFace path edge membership";
const CYCLE_MEMBERSHIP: &str = "visit F3D parameter cycle self-edge membership";

fn split_fixture(
    slots: &[i64],
) -> (
    crate::records::feature::scope::DesignParameterScope,
    crate::records::topology::construction::DesignConstructionOperandGroup,
    Vec<crate::records::topology::entity_selection::DesignEntitySelectionOperand>,
) {
    let scope = fixtures::scope(DesignFeatureKind::SplitFace, true);
    let mut group = fixtures::group(100, 0, DesignOperandRole::ROLE_0X21);
    let members: Vec<_> = slots
        .iter()
        .enumerate()
        .map(|(ordinal, _)| Located {
            value: 200 + u32::try_from(ordinal).unwrap(),
            offset: 1026 + cadmpeg_core::decode::u64_from_index(ordinal) * 11,
        })
        .collect();
    group.try_set_members(members).unwrap();
    let selections = slots
        .iter()
        .enumerate()
        .map(|(ordinal, slot)| {
            let ordinal = u32::try_from(ordinal).unwrap();
            fixtures::selection(200 + ordinal, ordinal, *slot)
        })
        .collect();
    (scope, group, selections)
}

#[test]
fn membership_split_edge_keeps_original_refusal_and_native_operands() {
    let (scope, group, selections) = split_fixture(&[42, 43]);
    let original = selections.clone();
    assert_original_refusal(SPLIT_MEMBERSHIP, |ctx| {
        super::super::super::resolved_split_face_path(ctx, &scope, &group, &selections, &[])
    });
    assert_eq!(selections, original);
}

#[test]
fn membership_split_edge_keeps_historical_state_and_path_order() {
    let (scope, group, selections) = split_fixture(&[43, 42]);
    let original = selections.clone();
    crate::test_support::with_decode_context(|ctx| {
        let Some(PathRef::HistoricalEdges {
            state,
            edges,
            native,
        }) = super::super::super::resolved_split_face_path(ctx, &scope, &group, &selections, &[])
            .unwrap()
        else {
            panic!("historical path");
        };
        let feature = crate::ids::neutral_feature_id(&scope);
        let prefix = crate::ids::history_input_prefix(&feature.key(), 7);
        assert_eq!(state, fixtures::historical_state(&scope));
        assert_eq!(
            edges.as_slice(),
            &[
                crate::ids::history_input_edge_id(&prefix, 43),
                crate::ids::history_input_edge_id(&prefix, 42),
            ]
        );
        assert_eq!(native.as_str(), group.id);
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(selections, original);
}

#[test]
fn membership_split_duplicate_returns_before_historical_edge_construction() {
    let (scope, group, selections) = split_fixture(&[42, 42, 99]);
    let original = selections.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "f3d SplitFace historical edge id",
            None,
        );
        assert!(super::super::super::resolved_split_face_path(
            ctx,
            &scope,
            &group,
            &selections,
            &[]
        )
        .unwrap()
        .is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(selections, original);
}

#[test]
fn membership_split_first_edge_skips_empty_query() {
    let (scope, group, selections) = split_fixture(&[42]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, SPLIT_MEMBERSHIP, None);
        assert!(super::super::super::resolved_split_face_path(
            ctx,
            &scope,
            &group,
            &selections,
            &[]
        )
        .unwrap()
        .is_some());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

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
        super::super::super::cyclic_parameter_components(ctx, &parameters, &unresolved)
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
            super::super::super::cyclic_parameter_components(ctx, &parameters, &unresolved)
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
            super::super::super::cyclic_parameter_components(ctx, &parameters, &unresolved)
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
            super::super::super::cyclic_parameter_components(ctx, &parameters, &unresolved)
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
