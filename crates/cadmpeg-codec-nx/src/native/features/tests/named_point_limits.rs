// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_sketch_named_point_block_uses;
use crate::native::features::feature_sketch_preceding_named_point_uses;
use crate::native::features::feature_sketch_references;
use crate::native::features::offset_store_named_points;

fn named_point_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            crate::test_support::test_prt::composed_feature_history_prt(),
        )
    })
    .expect("composed feature-history container");
    let admitted =
        crate::test_support::with_decode_context(|ctx| offset_store_named_points(ctx, &container))
            .expect("admitted named point route");
    assert!(!admitted.is_empty());
    assert!(admitted[0].data_blocks.len() >= 2);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    offset_store_named_points(&ctx, &container).expect_err("named point resource limit")
}

#[test]
fn named_point_refuses_collection_limit() {
    let error = named_point_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn named_point_refuses_retained_limit() {
    let error = named_point_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn named_point_refuses_scoped_limit() {
    let error = named_point_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn named_point_refuses_work_limit() {
    let error = named_point_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn named_point_block_use_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            crate::test_support::test_prt::composed_feature_history_prt(),
        )
    })
    .expect("composed feature-history container");
    let (references, points) = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            feature_sketch_references(ctx, &container)?,
            offset_store_named_points(ctx, &container)?,
        ))
    })
    .expect("named point block-use inputs");
    assert!(!crate::test_support::with_decode_context(|ctx| {
        feature_sketch_named_point_block_uses(ctx, &references, &points)
    })
    .expect("admitted block use")
    .is_empty());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_sketch_named_point_block_uses(&ctx, &references, &points)
        .expect_err("named point block-use resource limit")
}

#[test]
fn named_point_block_use_refuses_collection_limit() {
    let error = named_point_block_use_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn named_point_block_use_refuses_retained_limit() {
    let error = named_point_block_use_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn named_point_block_use_refuses_work_limit() {
    let error = named_point_block_use_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn preceding_named_point_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let reference = |ordinal, block: &str| crate::native::features::FeatureSketchReference {
        id: format!("reference-{ordinal}"),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        position: crate::om::sketch_references::SketchReferencePosition::new(
            crate::om::sketch_references::SketchReferenceCount::from_count_byte(2),
            ordinal,
        )
        .expect("sketch position"),
        token: crate::om::reference_index::ReferenceIndexToken::from_wire(
            12 + ordinal,
            &[0xf0, u8::try_from(12 + ordinal).expect("fixture value fits u8")],
        )
        .expect("reference token"),
        data_block: Some(block.into()),
        source_offset: 300 + u64::from(ordinal),
    };
    let references = [
        reference(0, "nx:om-data-blocks-2:block#12"),
        reference(1, "nx:om-data-blocks-2:block#13"),
    ];
    let point = crate::native::features::OffsetStoreNamedPoint {
        id: "nx:offset-store:named-point#2-10".into(),
        name: "Point1".into(),
        data_blocks: [
            "nx:om-data-blocks-2:block#10",
            "nx:om-data-blocks-2:block#11",
        ]
        .map(str::to_string)
        .to_vec(),
        values: [(1.0, 200), (2.0, 220)].map(|(value, source_offset)| {
            crate::native::features::FeatureBinary64ScalarToken {
                scalar: crate::om::scalar::ShiftedBinary64::try_from(
                    crate::test_support::test_bytes::shifted_f64_bytes(value),
                )
                .expect("shifted scalar"),
                source_offset,
            }
        }),
        source_offset: 190,
    };
    let points = [point];
    let admitted = crate::test_support::with_decode_context(|ctx| {
        feature_sketch_preceding_named_point_uses(ctx, &references, &points)
    })
    .expect("admitted preceding named-point route");
    assert!(!admitted.is_empty());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_sketch_preceding_named_point_uses(&ctx, &references, &points)
        .expect_err("preceding named-point resource limit")
}

#[test]
fn preceding_named_point_refuses_collection_limit() {
    let error = preceding_named_point_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn preceding_named_point_refuses_retained_limit() {
    let error = preceding_named_point_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn preceding_named_point_refuses_scoped_limit() {
    let error = preceding_named_point_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn preceding_named_point_refuses_work_limit() {
    let error = preceding_named_point_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
