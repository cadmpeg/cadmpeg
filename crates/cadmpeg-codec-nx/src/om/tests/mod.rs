// SPDX-License-Identifier: Apache-2.0
//! Unit and fixture tests for OM wire parsers owned by `om`.

#![allow(clippy::unwrap_used)]

#[test]
fn unique_candidate_stops_after_second_hit() {
    let mut yielded = 0;
    let result = super::unique_candidate((0..).inspect(|_| {
        yielded += 1;
    }));

    assert_eq!(result, None);
    assert_eq!(yielded, 2);
}

pub(super) fn message_bytes(text: &[u8], value: &[u8], count_or_severity: [u8; 2]) -> Vec<u8> {
    let declared_length = u8::try_from(text.len() + 2).expect("short synthesized message");
    let mut bytes = vec![0x03, declared_length];
    bytes.extend_from_slice(text);
    bytes.extend([0, 0, 0, 0, 0]);
    bytes.extend_from_slice(value);
    bytes.extend(count_or_severity);
    bytes
}

mod control_lanes;
mod index_and_lanes;
mod instances_and_stores;
mod operation_records;
mod operation_state;
mod pattern_lanes;
mod sketch_payload;

fn surface_feature_payload_references_test(record: crate::om::operation_record::OperationPayload<'_>) -> Option<crate::om::surface_envelope::SurfaceFeaturePayloadReferenceField> {
    crate::test_support::with_decode_context(|ctx| crate::om::surface_envelope::surface_feature_payload_references(ctx, record)).unwrap()
}

#[test]
fn expression_unit_equality_cost_counts_variant_and_label() {
    use cadmpeg_core::decode::cost::DecodeCost;
    for (unit, expected) in [(super::ExpressionUnit::Millimeter, 1), (super::ExpressionUnit::Inch, 1), (super::ExpressionUnit::Degree, 1), (super::ExpressionUnit::Native("μm".into()), 1 + 3)] {
        // The cost is one variant byte plus the UTF-8 bytes of a native label.
        crate::test_support::with_decode_context(|ctx| assert_eq!(unit.decode_cost(ctx, "NX expression unit equality").unwrap(), expected));
        let error = crate::test_support::resource_refusal_at(&[], cadmpeg_core::decode::ResourceDimension::WorkUnits, "NX expression unit equality", |ctx| ctx.equal(&unit, &unit, "NX expression unit equality"));
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.additional == expected));
    }
}
