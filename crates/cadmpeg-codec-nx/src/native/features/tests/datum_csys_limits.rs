// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_datum_csys_constructions;
use crate::test_support::test_om::composed_feature_history_payload_over_sort_scratch;
use crate::test_support::test_prt::prt_with_named_payloads;

fn datum_csys_refusal(
    dimension: cadmpeg_core::decode::ResourceDimension,
) -> cadmpeg_core::CodecError {
    let mut frame = vec![
        0x13, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];
    for _ in 0..8 {
        frame.extend_from_slice(&[0xf0, 0x03]);
    }
    frame.extend_from_slice(&[0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    let store = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload = composed_feature_history_payload_over_sort_scratch(
        &[(&[3, 0xff, 0xff, 0xff], "DATUM_CSYS", frame)],
        &store,
    );
    let source_bytes_1 = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);

    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, &source_bytes_1)
    })
    .expect("synthetic datum CSYS container");
    crate::test_support::with_decode_context_over(
        container.data.as_ref(),
        |policy| {
            policy.limits.max_work_units = u64::MAX;
            policy.limits.max_collection_items = u64::MAX;
            policy.limits.max_retained_bytes = u64::MAX;
            policy.limits.max_materialized_bytes = u64::MAX;
        },
        |history_ctx| {
            let history = crate::native::features::FeatureHistory::new(history_ctx, &container)?;
            let inputs = crate::native::features::feature_input_blocks(history_ctx, &history)?;

            let admitted = crate::test_support::with_decode_context(|ctx| {
                let inputs = crate::native::features::feature_input_blocks(ctx, &history)?;
                feature_datum_csys_constructions(ctx, &history, &inputs)
            })
            .expect("admitted datum CSYS constructions");
            assert_eq!(admitted.len(), 1);
            assert_eq!(admitted[0].frame.members().len(), 8);

            let operation = match dimension {
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    "resolve NX datum CSYS input prefix"
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    "NX feature input operation index"
                }
                _ => "NX datum CSYS constructions",
            };
            Ok::<_, cadmpeg_core::CodecError>(crate::test_support::resource_refusal_at(
                container.data.as_ref(),
                dimension,
                operation,
                |ctx| feature_datum_csys_constructions(ctx, &history, &inputs),
            ))
        },
    )
    .expect("prepared feature history")
}

#[test]
fn datum_csys_construction_refuses_collection_limit() {
    let error = datum_csys_refusal(cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn datum_csys_construction_refuses_retained_limit() {
    let error = datum_csys_refusal(cadmpeg_core::decode::ResourceDimension::RetainedBytes);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn datum_csys_construction_refuses_scoped_limit() {
    let error = datum_csys_refusal(cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn datum_csys_construction_refuses_work_limit() {
    let error = datum_csys_refusal(cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
