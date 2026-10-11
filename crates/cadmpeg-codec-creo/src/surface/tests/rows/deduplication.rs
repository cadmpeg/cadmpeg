// SPDX-License-Identifier: Apache-2.0

#[test]
fn surface_row_deduplication_refuses_work() {
    let row = [
        7, 0x22, 4, 0x01, 0, 0, 0xe4, 0xe4, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3,
    ];
    let mut second = row;
    second[0] = 8;
    let payload = [row.as_slice(), second.as_slice()].concat();
    let admitted = crate::decode::with_test_decode_ctx(|ctx|
        crate::surface::rows(ctx, &payload))
        .expect("two surface rows");
    assert_eq!(admitted.len(), 2);
    assert_eq!(admitted[0].offset, 0);
    assert_eq!(admitted[1].offset, row.len());
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo rows with boundaries result deduplication",
        |ctx| crate::surface::rows(ctx, &payload),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo rows with boundaries result deduplication")
    );
}

#[test]
fn singleton_surface_row_does_not_enter_deduplication() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let payload = [
        7, 0x22, 4, 0x01, 0, 0, 0xe4, 0xe4, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3,
    ];
    let work = crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = crate::surface::rows(&ctx, &payload);
        if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result {
            assert_ne!(refusal.operation, "creo rows with boundaries result deduplication");
        }
        result
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let rows = crate::surface::rows(&ctx, &payload).expect("singleton row");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 7);
    assert_eq!(rows[0].offset, 0);
}
