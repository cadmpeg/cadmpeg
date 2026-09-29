// SPDX-License-Identifier: Apache-2.0

#[test]
fn retained_scan_sections_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = super::build_prt("c", &[("VisibGeom", b"payload".to_vec())]);
    let service = crate::decode::with_test_decode_ctx(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service scan admitted");
    assert_eq!(service.framing.sections.len(), 1);

    let refusal_limit = (0..4096).find(|&limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("root image admitted");
        matches!(
            crate::container::scan_bytes(&ctx, bytes.clone()),
            Err(CodecError::ResourceLimit(resource))
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == "creo retained scan sections"
        )
    });
    assert!(refusal_limit.is_some(), "one retained section exceeds a collection cap");
}
