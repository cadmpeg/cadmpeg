// SPDX-License-Identifier: Apache-2.0

use super::super::{external_reference_records, external_reference_tail_reference_pairs};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

fn external_file() -> Vec<u8> {
    crate::test_support::test_prt::prt_with_named_payloads(&[(
        "/Root/ExternalReferences",
        crate::test_support::test_streams::external_reference_stream(),
    )])
}

#[test]
fn native_external_handles_transfer_to_retained_output() {
    let file = external_file();
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .unwrap();
    let records =
        crate::test_support::with_decode_context(|ctx| external_reference_records(ctx, &container))
            .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, 6);
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "nx external reference handles",
        |ctx| external_reference_records(ctx, &container),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "nx external reference handles" && limit.additional > 0));
}

#[test]
fn native_external_tail_pair_scratch_is_released_after_projection() {
    let file = external_file();
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .unwrap();
    let records =
        crate::test_support::with_decode_context(|ctx| external_reference_records(ctx, &container))
            .unwrap();
    let pairs = crate::test_support::with_decode_context(|ctx| {
        external_reference_tail_reference_pairs(ctx, &container, &records)
    })
    .unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].persistent_handle, 5);
    let expected = pairs.capacity()
        * std::mem::size_of::<super::super::ExternalReferenceTailReferencePair>()
        + pairs
            .iter()
            .map(|pair| pair.id.len() + pair.handle_set_record.len())
            .sum::<usize>();
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "test native tail retained checkpoint",
        |ctx| {
            let pairs = external_reference_tail_reference_pairs(ctx, &container, &records)?;
            ctx.charge_retained(1, "test native tail retained checkpoint")?;
            Ok(pairs)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("retained checkpoint must refuse")
    };
    assert_eq!(limit.used, cadmpeg_core::decode::u64_from_index(expected));
    assert_eq!(limit.additional, 1);
}

#[test]
fn native_control_value_projection_visits_are_charged() {
    let error = super::control_value_route_refusal(
        ResourceDimension::WorkUnits,
        "project NX control values",
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "project NX control values" && limit.additional == 1));
}

#[test]
fn native_control_index_value_projection_visits_are_charged() {
    let error = super::control_index_value_route_refusal(
        ResourceDimension::WorkUnits,
        "project NX control index values",
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "project NX control index values" && limit.additional == 1));
}

#[test]
fn om_text_projection_copies_the_checked_value() {
    let file = crate::test_support::test_prt::prt_with_indexed_om_section();
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .unwrap();
    let rows = crate::test_support::with_decode_context(|ctx| {
        super::super::string_values(ctx, &container)
    })
    .unwrap();
    assert!(!rows.is_empty());
    let expected = rows[0].value.as_str().len();
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "NX string value text",
        |ctx| super::super::string_values(ctx, &container),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.additional == cadmpeg_core::decode::u64_from_index(expected))
    );
}
