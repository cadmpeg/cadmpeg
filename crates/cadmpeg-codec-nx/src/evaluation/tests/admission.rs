// SPDX-License-Identifier: Apache-2.0

use super::{attach_complete_active_configuration, complete_block_ir};
use crate::evaluation::{active_configuration_is_admitted, evaluate_saved_body_census};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use std::collections::BTreeSet;

#[test]
fn census_replay_and_configuration_work_refuse_at_the_named_route() {
    let mut ir = complete_block_ir();
    attach_complete_active_configuration(&mut ir);
    for operation in [
        "NX saved body identity traversal",
        "NX body census replay traversal",
        "NX replay feature identity index",
        "NX new body replay identity",
        "NX body census identity comparison",
        "NX active configuration selection",
        "NX active configuration bodies",
        "NX active configuration neutral features",
    ] {
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            operation,
            |ctx| evaluate_saved_body_census(ctx, &ir),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn census_scratch_and_returned_identity_use_separate_storage_routes() {
    let ir = complete_block_ir();
    for (dimension, operation) in [
        (
            ResourceDimension::MaterializedBytes,
            "NX body census feature order",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "NX replay feature identity index",
        ),
        (
            ResourceDimension::RetainedBytes,
            "NX verified census body identity",
        ),
        (
            ResourceDimension::CollectionItems,
            "NX verified body census",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(&[], dimension, operation, |ctx| {
            evaluate_saved_body_census(ctx, &ir)
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn configuration_without_a_body_list_does_not_visit_the_suffix() {
    let mut ir = complete_block_ir();
    attach_complete_active_configuration(&mut ir);
    ir.model.configurations[0].bodies = None;
    let mut inactive = ir.model.configurations[0].clone();
    inactive.active = false;
    ir.model.configurations.extend(vec![inactive; 4096]);
    let saved = BTreeSet::from([&ir.model.bodies[0].id]);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 1;
        },
        |ctx| {
            assert!(!active_configuration_is_admitted(ctx, &ir, &saved).unwrap());
        },
    );
}

#[test]
fn extra_active_configuration_does_not_visit_the_suffix() {
    let mut ir = complete_block_ir();
    attach_complete_active_configuration(&mut ir);
    let active = ir.model.configurations[0].clone();
    let mut inactive = active.clone();
    inactive.active = false;
    ir.model.configurations.push(active);
    ir.model.configurations.extend(vec![inactive; 4096]);
    let saved = BTreeSet::from([&ir.model.bodies[0].id]);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 2;
        },
        |ctx| {
            assert!(!active_configuration_is_admitted(ctx, &ir, &saved).unwrap());
        },
    );
}

#[test]
fn boundary_family_matches_the_serialized_definition_tag() {
    let ir = complete_block_ir();
    let feature = &ir.model.features[0];
    let wire = serde_json::to_value(feature.evaluation.definition()).unwrap();
    let boundary = crate::test_support::with_decode_context(|ctx| {
        crate::evaluation::feature_boundary(ctx, feature)
    })
    .unwrap();
    assert_eq!(
        boundary.family.as_deref(),
        wire.get("definition").and_then(serde_json::Value::as_str)
    );
}
