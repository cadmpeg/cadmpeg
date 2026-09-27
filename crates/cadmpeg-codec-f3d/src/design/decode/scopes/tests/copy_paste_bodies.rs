// SPDX-License-Identifier: Apache-2.0

#[test]
fn generated_copy_paste_bodies_scope_matches_operation_layout() {
    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let headers =
        crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(
            &bytes, &records,
        )
        .into_iter()
        .filter(|header| header.record_index == 1_400)
        .collect::<Vec<_>>();
    assert_eq!(headers.len(), 1);
    let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &bytes,
        &records,
        headers[0].record_index,
        &headers[0].class_tag,
        headers[0].byte_offset,
    )
    .expect("scope");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::CopyPasteBodies
    );
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [1_500, 1_600]
    );
    assert_eq!(scope.frame_length(), 225);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("operation decode context")
        .0;
    let operation =
        crate::design::decode::scopes::copy_paste_bodies::exact_copy_paste_bodies_operation(
            &ctx, &bytes, &records, &scope,
        )
        .expect("operation decode resources")
        .expect("CopyPasteBodies operation");
    assert_eq!(operation.body_group_record_index, 1_500);
    assert_eq!(operation.relation_record_index, 1_700);
    assert_eq!(
        operation
            .bodies()
            .iter()
            .map(|body| body.source.value)
            .collect::<Vec<_>>(),
        [985]
    );
    assert_eq!(
        operation
            .bodies()
            .iter()
            .map(|body| body.copied.value)
            .collect::<Vec<_>>(),
        [8_422]
    );
}

#[test]
fn copy_paste_bodies_decode_allocations_refuse_collection_limit() {
    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let header = crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(
        &bytes, &records,
    )
    .into_iter()
    .find(|header| header.record_index == 1_400)
    .expect("operation header");
    let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &bytes,
        &records,
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .expect("operation scope");
    for (limit, operation) in [
        (0, "parse F3D copied body operands"),
        (1, "parse F3D copied bodies"),
        (2, "index F3D copied body suffixes"),
    ] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let ctx = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("operation limit context")
            .0;
        let error =
            crate::design::decode::scopes::copy_paste_bodies::exact_copy_paste_bodies_operation(
                &ctx, &bytes, &records, &scope,
            )
            .expect_err("operation collection limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref refusal)
            if refusal.operation == operation));
    }
}
