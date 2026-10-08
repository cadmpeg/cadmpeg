// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_input_blocks;
use crate::native::features::feature_operation_labels;
use crate::native::features::feature_operation_records;
use crate::native::features::feature_sketch_construction_inputs;
use crate::native::features::feature_sketch_construction_payloads;
use crate::native::features::feature_sketch_payload_scalars;
use crate::native::features::feature_sketch_records;
use crate::native::features::feature_sketch_references;

#[derive(Clone, Copy)]
enum SketchPayloadRoute {
    Construction,
    Scalar,
}

fn sketch_reference_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            crate::test_support::test_prt::composed_feature_history_prt_over_sort_scratch(),
        )
    })
    .expect("composed feature-history container");
    let decode =
        |ctx: &cadmpeg_core::decode::DecodeContext<'_>| feature_sketch_references(ctx, &container);
    assert!(!crate::test_support::with_decode_context(|ctx| decode(ctx))
        .expect("admitted sketch references")
        .is_empty());

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("sketch reference resource limit"),
    )
}

#[test]
fn sketch_reference_refuses_collection_limit() {
    let error = sketch_reference_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn sketch_reference_refuses_retained_limit() {
    let error = sketch_reference_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn sketch_reference_refuses_scoped_limit() {
    let error = sketch_reference_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn sketch_reference_refuses_work_limit() {
    let error = sketch_reference_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn sketch_payload_refusal(
    route: SketchPayloadRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            crate::test_support::test_prt::composed_feature_history_prt(),
        )
    })
    .expect("composed feature-history container");
    let constructions = crate::test_support::with_decode_context(|ctx| {
        let labels = feature_operation_labels(ctx, &container)?;
        let records = feature_operation_records(ctx, &container)?;
        let inputs = feature_input_blocks(ctx, &container)?;
        let references = feature_sketch_references(ctx, &container)?;
        let sketches = feature_sketch_records(ctx, &labels, &records, &inputs, &references)?;
        feature_sketch_construction_inputs(ctx, &sketches, &references)
    })
    .expect("sketch payload construction inputs");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        SketchPayloadRoute::Construction => {
            feature_sketch_construction_payloads(ctx, &container, &constructions)
                .map(|rows| rows.len())
        }
        SketchPayloadRoute::Scalar => {
            feature_sketch_payload_scalars(ctx, &container, &constructions).map(|rows| rows.len())
        }
    };
    assert!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted sketch payload route")
            > 0
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("sketch payload resource limit"),
    )
}

#[test]
fn sketch_payload_refuses_collection_limit() {
    let error = sketch_payload_refusal(SketchPayloadRoute::Construction, |policy| {
        policy.limits.max_collection_items = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn sketch_payload_refuses_retained_limit() {
    let error = sketch_payload_refusal(SketchPayloadRoute::Construction, |policy| {
        policy.limits.max_retained_bytes = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn sketch_payload_refuses_scoped_limit() {
    let error = sketch_payload_refusal(SketchPayloadRoute::Construction, |policy| {
        policy.limits.max_materialized_bytes = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn sketch_payload_refuses_work_limit() {
    let error = sketch_payload_refusal(SketchPayloadRoute::Construction, |policy| {
        policy.limits.max_work_units = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn sketch_payload_scalar_refuses_collection_limit() {
    let error = sketch_payload_refusal(SketchPayloadRoute::Scalar, |policy| {
        policy.limits.max_collection_items = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn sketch_payload_scalar_refuses_retained_limit() {
    let error = sketch_payload_refusal(SketchPayloadRoute::Scalar, |policy| {
        policy.limits.max_retained_bytes = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn sketch_payload_scalar_refuses_scoped_limit() {
    let error = sketch_payload_refusal(SketchPayloadRoute::Scalar, |policy| {
        policy.limits.max_materialized_bytes = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn sketch_payload_scalar_refuses_work_limit() {
    let error = sketch_payload_refusal(SketchPayloadRoute::Scalar, |policy| {
        policy.limits.max_work_units = 0;
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
