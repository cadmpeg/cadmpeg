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
        .unwrap()
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
    let operation =
        crate::design::decode::scopes::copy_paste_bodies::exact_copy_paste_bodies_operation(
            &bytes, &records, &scope,
        )
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
fn generated_copy_paste_bodies_decode_parameter_scopes() {
    let archive =
        crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_copy_paste_bodies(
            &[],
        );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        assert!(
            !crate::design::decode::scopes::parameter_scope::decode_parameter_scopes(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                &crate::design::decode::scopes::parameter_scope::ParameterScopeSources::default(),
            )
            .unwrap()
            .scopes
            .is_empty()
        );
    });
}

#[test]
fn a_prologue_zero_scope_reads_its_references_after_the_marker_word() {
    let (mut bytes, _) =
        crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let header = crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(
        &bytes, &records,
    )
    .unwrap()
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
    .unwrap()
    .expect("scope");
    let start = usize::try_from(header.byte_offset).unwrap();
    let references = bytes[start + 29..start + 51].to_vec();
    bytes[start + 20] = 0;
    bytes[start + 21..start + 25].copy_from_slice(&1_u32.to_le_bytes());
    bytes[start + 25..start + 47].copy_from_slice(&references);
    bytes[start + 47..start + 51].fill(0);
    let decode = |bytes: &[u8]| {
        crate::design::decode::scopes::copy_paste_bodies::exact_copy_paste_bodies_operation(
            bytes, &records, &scope,
        )
    };
    let operation = decode(&bytes).expect("prologue-zero CopyPasteBodies operation");
    assert_eq!(operation.body_group_record_index, 1_500);
    assert_eq!(operation.relation_record_index, 1_700);
    for (at, value) in [(start + 21, 2), (start + 15, 7), (start + 20, 1)] {
        let mut other = bytes.clone();
        other[at] = value;
        assert!(decode(&other).is_none(), "byte {at} = {value}");
    }
}
