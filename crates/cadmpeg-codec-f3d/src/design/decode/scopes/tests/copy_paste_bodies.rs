// SPDX-License-Identifier: Apache-2.0

#[test]
fn generated_copy_paste_bodies_scope_matches_operation_layout() {
    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let headers = super::scope_candidate_headers(&bytes)
        .into_iter()
        .filter(|header| header.record_index == 1_400)
        .collect::<Vec<_>>();
    assert_eq!(headers.len(), 1);
    let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
        headers[0].record_index,
        &headers[0].class_tag,
        headers[0].byte_offset,
    )
    .unwrap()
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
fn copy_paste_bodies_refuses_operand_and_body_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let header = super::scope_candidate_headers(&bytes)
        .into_iter()
        .find(|header| header.record_index == 1_400)
        .unwrap();
    let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .unwrap();
    for (cap, operation) in [
        (0, "f3d CopyPasteBodies bodies"),
        (1, "index F3D copied body suffixes"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = cap;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result =
            crate::design::decode::scopes::copy_paste_bodies::exact_copy_paste_bodies_operation(
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
    use cadmpeg_core::decode::ResourceDimension;

    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let header = super::scope_candidate_headers(&bytes)
        .into_iter()
        .find(|header| header.record_index == 1_400)
        .unwrap();
    for operation in [
        "f3d Design scope reference members",
        "f3d Design scope located references",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            operation,
            0,
            |ctx| {
                crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
                    ctx,
                    &bytes,
                    &records,
                    header.record_index,
                    &header.class_tag,
                    header.byte_offset,
                )
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(failure)
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
fn copy_paste_body_scans_propagate_work_refusals() {
    use cadmpeg_core::decode::ResourceDimension;

    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let header = super::scope_candidate_headers(&bytes)
        .into_iter()
        .find(|header| header.record_index == 1_400)
        .unwrap();
    let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &records,
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .unwrap();

    let operation = "scan F3D CopyPasteBodies scope references";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            crate::design::decode::scopes::copy_paste_bodies::exact_copy_paste_bodies_operation(
                ctx, &bytes, &records, &scope,
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::WorkUnits
                && refusal.operation == operation
                && refusal.additional == 1
    ));
}

#[test]
fn design_scope_kind_scan_refuses_temporary_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let header = super::scope_candidate_headers(&bytes)
        .into_iter()
        .find(|header| header.record_index == 1_400)
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 0;
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
            if failure.dimension == ResourceDimension::MaterializedBytes
                && failure.operation == "f3d Design temporary UTF-16 text"
    ));
}

#[test]
fn decoded_parameter_scopes_refuse_identifier_and_output_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let archive =
        crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_copy_paste_bodies(
            &[],
        );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let decode = |policy: &DecodePolicy| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).unwrap();
            crate::design::decode::scopes::parameter_scope::decode_parameter_scopes(
                &ctx,
                scan,
                &crate::native::F3dNative::default(),
            )
        };
        assert!(!decode(&DecodePolicy::default()).unwrap().is_empty());
        for (dimension, operation) in [
            (
                ResourceDimension::CollectionItems,
                "f3d Design parameter scopes",
            ),
            (
                ResourceDimension::RetainedBytes,
                "f3d Design parameter scope ID",
            ),
        ] {
            let error =
                cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                    let mut policy = DecodePolicy::default();
                    match dimension {
                        ResourceDimension::CollectionItems => {
                            policy.limits.max_collection_items = cap;
                        }
                        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                        _ => unreachable!(),
                    }
                    // A fresh container cache preserves the request sequence.
                    crate::test_support::zip_test::with_scan(&archive, |scan| {
                        let arena = DecodeArena::new();
                        let (ctx, _) =
                            DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                        crate::design::decode::scopes::parameter_scope::decode_parameter_scopes(
                            &ctx,
                            scan,
                            &crate::native::F3dNative::default(),
                        )
                    })
                });
            let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
                panic!("resource refusal")
            };
            let mut policy = DecodePolicy::default();
            match dimension {
                ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = refusal.limit + 1;
                }
                ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = refusal.limit + 1;
                }
                _ => unreachable!(),
            }
            assert!(
                !matches!(decode(&policy), Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation),
                "{operation} must be admitted above its boundary"
            );
        }
    });
}
