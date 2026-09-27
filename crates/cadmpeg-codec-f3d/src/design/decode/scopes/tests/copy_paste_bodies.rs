// SPDX-License-Identifier: Apache-2.0

#[test]
fn generated_copy_paste_bodies_scope_matches_operation_layout() {
    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let headers =
        crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(&cadmpeg_test_support::service_decode_context(),
            &bytes, &records,
        ).unwrap()
        .into_iter()
        .filter(|header| header.record_index == 1_400)
        .collect::<Vec<_>>();
    assert_eq!(headers.len(), 1);
    let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(&cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
        headers[0].record_index,
        &headers[0].class_tag,
        headers[0].byte_offset,
    ).unwrap()
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
    let header = crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(&cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
    ).unwrap()
    .into_iter()
    .find(|header| header.record_index == 1_400)
    .unwrap();
    let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(&cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    ).unwrap()
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

#[test]
fn design_scope_reference_vectors_refuse_each_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let header = crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(&cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
    ).unwrap()
    .into_iter()
    .find(|header| header.record_index == 1_400)
    .unwrap();
    for (cap, operation) in [
        (1, "f3d Design scope reference members"),
        (3, "f3d Design scope reference offsets"),
        (5, "f3d Design scope located references"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
            &ctx,
            &bytes,
            &records,
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        );
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation
        ));
    }
    assert!(
        crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &records,
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .is_some()
    );
}

#[test]
fn design_scope_kind_scan_refuses_temporary_and_retained_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let header = crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(&cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
    ).unwrap()
    .into_iter()
    .find(|header| header.record_index == 1_400)
    .unwrap();
    let kind_len = "CopyPasteBodies".len() as u64;
    for (materialized_cap, retained_cap, dimension, operation) in [
        (Some(0), None, ResourceDimension::MaterializedBytes, "f3d Design temporary UTF-16 text"),
        (None, Some(0), ResourceDimension::RetainedBytes, "f3d Design UTF-16 text"),
        (None, Some(kind_len), ResourceDimension::RetainedBytes, "f3d Design scope kind storage"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        if let Some(cap) = materialized_cap {
            policy.limits.max_materialized_bytes = cap;
        }
        if let Some(cap) = retained_cap {
            policy.limits.max_retained_bytes = cap;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
            &ctx,
            &bytes,
            &records,
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        );
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ));
    }
}

#[test]
fn design_scope_candidate_headers_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(
        &ctx, &bytes, &records,
    );
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d Design scope candidate headers"
    ));
    assert!(
        !crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &records,
        )
        .unwrap()
        .is_empty()
    );
}
