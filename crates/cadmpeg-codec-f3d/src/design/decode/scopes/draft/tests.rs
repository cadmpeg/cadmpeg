// SPDX-License-Identifier: Apache-2.0

#[test]
fn consecutive_guid_pair_scan_propagates_candidate_and_guid_refusals() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut bytes = Vec::new();
    crate::test_support::lp_utf16(&mut bytes, "ABCDEF12-3456-7890-ABCD-EF1234567890");
    crate::test_support::lp_utf16(&mut bytes, "12345678-ABCD-EF01-2345-6789ABCDEF01");

    assert!(super::contains_consecutive_guid_pair(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
    )
    .unwrap());

    for (operation, additional) in [
        ("scan F3D consecutive GUID pair candidates", 152),
        ("validate F3D counted relaxed GUID", 72),
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| super::contains_consecutive_guid_pair(ctx, &bytes).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::WorkUnits
                    && refusal.operation == operation
                    && refusal.additional == additional
        ));
    }
}
