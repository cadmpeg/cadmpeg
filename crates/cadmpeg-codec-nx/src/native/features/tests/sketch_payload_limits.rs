// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_input_blocks;
use crate::native::features::feature_operation_labels;
use crate::native::features::feature_operation_records;
use crate::native::features::feature_sketch_construction_inputs;
use crate::native::features::feature_sketch_construction_payloads;
use crate::native::features::feature_sketch_records;
use crate::native::features::feature_sketch_references;

fn sketch_payload_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx,
            crate::test_support::test_prt::composed_feature_history_prt())
    }).expect("composed feature-history container");
    let constructions = crate::test_support::with_decode_context(|ctx| {
        let labels = feature_operation_labels(ctx, &container)?;
        let records = feature_operation_records(ctx, &container)?;
        let inputs = feature_input_blocks(ctx, &container)?;
        let references = feature_sketch_references(ctx, &container)?;
        let sketches = feature_sketch_records(ctx, &labels, &records, &inputs, &references)?;
        feature_sketch_construction_inputs(ctx, &sketches, &references)
    }).expect("sketch payload construction inputs");
    let admitted = crate::test_support::with_decode_context(|ctx| {
        feature_sketch_construction_payloads(ctx, &container, &constructions)
    }).expect("admitted sketch construction payloads");
    assert!(!admitted.is_empty());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_sketch_construction_payloads(&ctx, &container, &constructions)
        .expect_err("sketch payload resource limit")
}

#[test]
fn sketch_payload_refuses_collection_limit() {
    let error = sketch_payload_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn sketch_payload_refuses_retained_limit() {
    let error = sketch_payload_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn sketch_payload_refuses_scoped_limit() {
    let error = sketch_payload_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn sketch_payload_refuses_work_limit() {
    let error = sketch_payload_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
