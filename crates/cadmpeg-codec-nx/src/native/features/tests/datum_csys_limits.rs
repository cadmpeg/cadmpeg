// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_datum_csys_constructions;
use crate::test_support::test_om::composed_feature_history_payload;
use crate::test_support::test_prt::prt_with_named_payloads;

fn datum_csys_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let mut frame = vec![
        0x13, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];
    for _ in 0..8 {
        frame.extend_from_slice(&[0xf0, 0x03]);
    }
    frame.extend_from_slice(&[0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    let store = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload =
        composed_feature_history_payload(&[(&[3, 0xff, 0xff, 0xff], "DATUM_CSYS", frame)], &store);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]),
        )
    })
    .expect("synthetic datum CSYS container");
    let admitted = crate::test_support::with_decode_context(|ctx| {
        feature_datum_csys_constructions(ctx, &container)
    })
    .expect("admitted datum CSYS constructions");
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].frame.members().len(), 8);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_datum_csys_constructions(&ctx, &container)
        .expect_err("datum CSYS construction resource limit")
}

#[test]
fn datum_csys_construction_refuses_collection_limit() {
    let error = datum_csys_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn datum_csys_construction_refuses_retained_limit() {
    let error = datum_csys_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn datum_csys_construction_refuses_scoped_limit() {
    let error = datum_csys_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn datum_csys_construction_refuses_work_limit() {
    let error = datum_csys_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
