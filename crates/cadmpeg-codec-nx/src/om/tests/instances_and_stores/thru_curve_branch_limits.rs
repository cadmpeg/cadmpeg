// SPDX-License-Identifier: Apache-2.0
//! Resource refusals for counted `THRU_CURVE` branch groups.

fn branch_bytes() -> Vec<u8> {
    let mut bytes = b"\x13\x00\x00\x01\x00\xf1\x01\x21\xf1\x01\x22\xf1\x01\x23\x01\x08\x02\x03\x03\x04\x01\x01\x01\x01\x07\xf1\x01\x24\xf1\x01\x25\xf1\x01\x26\xf1\x01\x27\xf1\x01\x28\xf1\x01\x29\x04\x01\xa0\x5e\x38\x13\x01".to_vec();
    bytes.extend([2, 0x15, 1, 2, 0xf0, 0x31, 1, 2]);
    bytes.extend([0; 5]);
    bytes.extend([0xff, 1, 2, 0xf0, 0x32, 0, 0x81, 0x58]);
    bytes.extend([0, 0, 0, 0, 0, 0, 0xff, 0, 0xff, 1]);
    bytes
}

fn branch_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = branch_bytes();

    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            configure(policy);
        },
        |ctx| {
            let record =
                crate::om::operation_record::OperationPayload::new(&bytes, 0, "THRU_CURVE")
                    .unwrap();
            crate::om::thru_curve_branches::thru_curve_payload_branch_group(ctx, record)
                .map(|(value, _storage)| value)
                .unwrap_err()
        },
    )
}

#[test]
fn thru_curve_branch_limit_fixture_is_valid() {
    let bytes = branch_bytes();
    let group = super::thru_curve_payload_branch_group_for_test(
        crate::om::operation_record::OperationPayload::new(&bytes, 0, "THRU_CURVE").unwrap(),
    );
    assert_eq!(group.unwrap().branches().len(), 1);
}

#[test]
fn thru_curve_branches_refuse_collection_limit() {
    let error = branch_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn thru_curve_branches_refuse_scoped_limit() {
    let bytes = branch_bytes();
    let error = crate::test_support::resource_refusal_at(
        &bytes,
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "NX thru-curve branch members",
        |ctx| {
            let record =
                crate::om::operation_record::OperationPayload::new(&bytes, 0, "THRU_CURVE")
                    .unwrap();
            crate::om::thru_curve_branches::thru_curve_payload_branch_group(ctx, record)
                .map(|(value, _storage)| value)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "NX thru-curve branch members")
    );
}

#[test]
fn thru_curve_branches_refuse_work_limit() {
    let error = branch_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
