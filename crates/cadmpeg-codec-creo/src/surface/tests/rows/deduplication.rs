// SPDX-License-Identifier: Apache-2.0

#[test]
fn surface_row_deduplication_refuses_work() {
    let payload = [7, 0x22, 4, 0x01, 0, 0, 0xe4, 0xe4, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3];
    let error = crate::test_support::last_refusal_at(
        &[], cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo rows with boundaries result deduplication", |ctx| crate::surface::rows_with_boundaries(ctx, &payload, &[]),
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo rows with boundaries result deduplication"));
}
