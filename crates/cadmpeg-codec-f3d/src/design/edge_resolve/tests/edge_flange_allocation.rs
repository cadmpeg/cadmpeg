// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn resolve_edge_flange(policy: &DecodePolicy) -> Result<(), CodecError> {
    let group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.preceding_boundary_edge_slots = vec![17, 18, 19];
    operand.changed_boundary_edge_slots = vec![17, 18];
    operand.updated_boundary_edge_slots = vec![17];
    operand.result_boundary_edge_slots = vec![17, 20];
    let feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#edge-flange").unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).unwrap();
    resolved_edge_flange_group(
        &group,
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[],
        Some(7),
        &feature_id,
        &ctx,
    )
    .map(|_| ())
}

fn assert_edge_flange_refusal(operation: &'static str, retained: bool) {
    let dimension = if retained {
        ResourceDimension::RetainedBytes
    } else {
        ResourceDimension::CollectionItems
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        let mut policy = DecodePolicy::default();
        if retained {
            policy.limits.max_retained_bytes = cap;
        } else {
            policy.limits.max_collection_items = cap;
        }
        resolve_edge_flange(&policy)
    });
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.operation == operation && failure.dimension == dimension));
}

#[test]
fn edge_flange_member_refuses_collection_limit() {
    assert_edge_flange_refusal("f3d edge flange member index", false);
}

#[test]
fn edge_flange_candidate_set_refuses_collection_limit() {
    assert_edge_flange_refusal("f3d edge flange candidate set", false);
}

#[test]
fn edge_flange_historical_edge_charges_collection_items() {
    // Released candidate scratch peaks above the historical edge charge.
    cadmpeg_test_support::refusal::assert_charged(
        ResourceDimension::CollectionItems,
        "f3d edge flange historical edge",
        || resolve_edge_flange(&DecodePolicy::service()),
    );
}

#[test]
fn edge_flange_historical_group_id_refuses_retained_limit() {
    assert_edge_flange_refusal("f3d edge flange historical group id", true);
}
