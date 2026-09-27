// SPDX-License-Identifier: Apache-2.0

#[test]
fn generated_copy_paste_bodies_scope_matches_operation_layout() {
    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
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
    let operation =
        crate::design::decode::scopes::copy_paste_bodies::exact_copy_paste_bodies_operation(
            &cadmpeg_test_support::service_decode_context(),
            &bytes, &records, &scope,
        )
        .unwrap()
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
fn copy_paste_bodies_refuses_operand_and_body_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let header = crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(
        &bytes,
        &records,
    )
    .into_iter()
    .find(|header| header.record_index == 1_400)
    .unwrap();
    let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &bytes,
        &records,
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap();
    for (cap, operation) in [
        (0, "f3d CopyPasteBodies operands"),
        (1, "f3d CopyPasteBodies bodies"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = crate::design::decode::scopes::copy_paste_bodies::exact_copy_paste_bodies_operation(
            &ctx, &bytes, &records, &scope,
        );
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation
        ));
    }
}
