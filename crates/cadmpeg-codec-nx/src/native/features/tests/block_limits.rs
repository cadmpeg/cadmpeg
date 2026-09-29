// SPDX-License-Identifier: Apache-2.0

fn block_construction_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let references = (0..19_u32).map(|ordinal| {
        crate::native::features::block_reference::FeatureBlockConstructionReference {
            id: format!("reference#{ordinal}"),
            operation_label: "operation".into(), control: 0x26,
            position: crate::native::features::block_reference::BlockReferencePosition::new(
                ordinal).expect("block reference position"),
            token: crate::om::reference_index::PayloadIndexToken::from_wire(
                ordinal + 100,
                &[0xf0, u8::try_from(ordinal + 100).expect("small object index")],
            ).expect("payload index token"),
            data_block: Some(format!("block#{ordinal}")),
            source_offset: u64::from(ordinal),
        }
    }).collect::<Vec<_>>();
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::native::features::feature_block_constructions(ctx, &references)
    };
    assert_eq!(crate::test_support::with_decode_context(|ctx| decode(ctx))
        .expect("admitted block construction").len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    decode(&ctx).expect_err("block construction resource limit")
}

fn block_payload_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let store = vec![b"A".as_slice(); 20];
    let part = crate::test_support::test_om::composed_feature_history_payload(&[], &store);
    let file = crate::test_support::test_prt::prt_with_named_payloads(
        &[("/Root/UG_PART/UG_PART", part)]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    }).expect("block payload container");
    let construction = crate::native::features::FeatureBlockConstruction {
        id: "block-construction#0".into(), operation_label: "operation".into(),
        control: 0x26,
        members: std::array::from_fn(|ordinal| {
            crate::native::features::FeatureConstructionMember {
                reference: format!("reference#{ordinal}"),
                data_block: format!("nx:om-data-blocks-0:block#{}", ordinal + 1),
            }
        }),
        terminal_reference: "reference#18".into(),
        terminal_data_block: "nx:om-data-blocks-0:block#19".into(),
    };
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::native::features::feature_block_construction_payloads(ctx, &container,
            std::slice::from_ref(&construction))
    };
    assert_eq!(crate::test_support::with_decode_context(|ctx| decode(ctx))
        .expect("admitted block payload").len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    decode(&ctx).expect_err("block payload resource limit")
}

#[test]
fn block_payload_refuses_collection_limit() {
    let error = block_payload_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn block_payload_refuses_retained_limit() {
    let error = block_payload_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn block_payload_refuses_scoped_limit() {
    let error = block_payload_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn block_payload_refuses_work_limit() {
    let error = block_payload_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn block_construction_refuses_collection_limit() {
    let error = block_construction_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn block_construction_refuses_retained_limit() {
    let error = block_construction_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn block_construction_refuses_scoped_limit() {
    let error = block_construction_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn block_construction_refuses_work_limit() {
    let error = block_construction_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
