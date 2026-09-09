// SPDX-License-Identifier: Apache-2.0

#[test]
fn generated_copy_paste_bodies_scope_matches_operation_layout() {
    let (bytes, _) = crate::test_support::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let headers =
        crate::design::decode::scopes::parameter_scope_candidate_headers(&bytes, &records)
            .into_iter()
            .filter(|header| header.record_index == 1_400)
            .collect::<Vec<_>>();
    assert_eq!(headers.len(), 1);
    let scope = crate::design::decode::scopes::parse_parameter_scope(
        &bytes,
        &records,
        headers[0].record_index,
        &headers[0].class_tag,
        headers[0].byte_offset,
    )
    .expect("scope");
    assert_eq!(
        scope.kind(),
        crate::records::feature::DesignFeatureKind::CopyPasteBodies
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
        crate::design::decode::scopes::exact_copy_paste_bodies_operation(&bytes, &records, &scope)
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
