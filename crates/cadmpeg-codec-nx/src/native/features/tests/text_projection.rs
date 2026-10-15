// SPDX-License-Identifier: Apache-2.0

use super::super::construction_records::feature_projected_curve_construction_strings;
use super::super::pattern::feature_pattern_construction_strings;
use super::super::payload_content::{FeaturePayloadBlock, FeaturePayloadContent};
use super::super::{
    FeatureConstructionOwner, FeatureConstructionPayload, FeatureHistory, FeatureProjectedCurveKind,
};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

fn fixture(operation_bytes: &[u8], store_bytes: &[u8]) -> Vec<u8> {
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "EXTRUDE", operation_bytes.to_vec())],
        &[store_bytes, b""],
    );
    crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)])
}

fn scan(file: &[u8]) -> crate::container::Container<'_> {
    crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file)).unwrap()
}

fn construction(container: &crate::container::Container<'_>) -> FeatureConstructionPayload {
    crate::test_support::with_decode_context(|ctx| {
        let blocks = super::super::offset_data_block_bytes(ctx, container)?;
        let id = blocks
            .keys()
            .find(|id| id.ends_with(":block#1"))
            .unwrap()
            .clone();
        let content: FeaturePayloadContent<Vec<FeaturePayloadBlock>> =
            FeaturePayloadContent::from_source(ctx, [id], blocks)?.unwrap();
        Ok::<_, CodecError>(FeatureConstructionPayload {
            id: "nx:feature-history:construction-payload#0-0000000000".into(),
            operation_label: "operation".into(),
            owner: FeatureConstructionOwner::ProjectedCurve {
                operation_kind: FeatureProjectedCurveKind::Projected,
                construction_references: Vec::new(),
            },
            content,
        })
    })
    .unwrap()
}

#[test]
fn feature_payload_text_projection_copies_the_checked_value() {
    let file = fixture(b"\x04\x07BLOCK\0", b"A");
    let container = scan(&file);
    let rows = crate::test_support::with_decode_context(|ctx| {
        super::super::feature_payload_strings(ctx, &FeatureHistory::new(ctx, &container)?)
    })
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].value.as_str(), "BLOCK");
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "NX feature payload string text",
        |ctx| super::super::feature_payload_strings(ctx, &FeatureHistory::new(ctx, &container)?),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 5));
}

#[test]
fn projected_curve_text_projection_copies_the_checked_value() {
    let file = fixture(b"", b"\x66\x32\x03\x05ABC\0");
    let container = scan(&file);
    let payload = construction(&container);
    let rows = crate::test_support::with_decode_context(|ctx| {
        feature_projected_curve_construction_strings(
            ctx,
            &container,
            std::slice::from_ref(&payload),
        )
    })
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].value.as_str(), "ABC");
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "NX projected curve string value",
        |ctx| {
            feature_projected_curve_construction_strings(
                ctx,
                &container,
                std::slice::from_ref(&payload),
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 3));
}

#[test]
fn pattern_text_projection_copies_the_checked_value() {
    let file = fixture(b"", b"\x66\x32\x03\x05ABC\0");
    let container = scan(&file);
    let payload = construction(&container);
    let rows = crate::test_support::with_decode_context(|ctx| {
        feature_pattern_construction_strings(ctx, &container, std::slice::from_ref(&payload))
    })
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(serde_json::to_value(&rows[0]).unwrap()["value"], "ABC");
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "NX pattern construction string value",
        |ctx| feature_pattern_construction_strings(ctx, &container, std::slice::from_ref(&payload)),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 3));
}
