// SPDX-License-Identifier: Apache-2.0
use super::{class_369_wrapper_two, class_412_path, path_locator};
use crate::test_support::{indexed_header, lp_utf16};
use cadmpeg_core::decode::ResourceDimension;

fn scope() -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignParameterScopeDraft,
    };
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:design-parameter-scope#1".into(),
            byte_offset: 0,
            class_tag: "291".to_owned().try_into().unwrap(),
            record_index: 1,
            frame_length: 329,
            kind_offset: 0,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 9,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![20],
                vec![0],
                "reference_members",
            )
            .unwrap(),
            payload: DesignFeatureKind::CPattern.try_into().unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: "258".to_owned().try_into().unwrap(),
            paired_byte_offset: 329,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

fn reference(bytes: &mut [u8], at: usize, index: u32) {
    bytes[at] = 1;
    bytes[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
}

fn path_fixture() -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"451", 10);
    bytes.resize(path_locator::LEN, 0);
    reference(&mut bytes, path_locator::NONZERO_RECORD_REFERENCE, 20);
    reference(&mut bytes, path_locator::SCOPE_BACKLINK, 1);
    reference(&mut bytes, path_locator::WRAPPER_REFERENCE, 13);
    bytes[path_locator::CONSTANT_TWO..path_locator::CONSTANT_TWO + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    for lane in [0, 5, 10, 15] {
        let at = path_locator::TRANSFORM + lane * 8;
        bytes[at..at + 8].copy_from_slice(&1f64.to_le_bytes());
    }
    for (index, guid) in [
        (11, "11111111-1111-1111-1111-111111111111"),
        (12, "22222222-2222-2222-2222-222222222222"),
    ] {
        let start = bytes.len();
        indexed_header(&mut bytes, *b"412", index);
        bytes.resize(start + class_412_path::OCCURRENCE_GUID, 0);
        bytes[start + class_412_path::PATH_MARKER] = 1;
        lp_utf16(&mut bytes, guid);
        lp_utf16(&mut bytes, guid);
        lp_utf16(&mut bytes, guid);
        bytes.extend_from_slice(&2u64.to_le_bytes());
        lp_utf16(&mut bytes, guid);
        lp_utf16(&mut bytes, guid);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        assert_eq!(bytes.len(), start + class_412_path::LEN);
    }
    let wrapper_at = bytes.len();
    indexed_header(&mut bytes, *b"369", 13);
    bytes.resize(wrapper_at + class_369_wrapper_two::LEN, 0);
    bytes[wrapper_at + class_369_wrapper_two::WRAPPER_MARKER] = 1;
    let at = wrapper_at + class_369_wrapper_two::PATH_COUNT;
    bytes[at..at + 4].copy_from_slice(&2u32.to_le_bytes());
    reference(
        &mut bytes,
        wrapper_at + class_369_wrapper_two::PATH_REFERENCES,
        11,
    );
    reference(
        &mut bytes,
        wrapper_at + class_369_wrapper_two::PATH_REFERENCES + 11,
        12,
    );
    indexed_header(&mut bytes, *b"999", 14);
    bytes
}

/// Validate the fixture's operand path and copy it into the output.
fn operand_path(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<
    Option<crate::records::feature::assembly::DesignAssemblyOperandPath>,
    cadmpeg_core::CodecError,
> {
    let Some(envelope) =
        super::exact_legacy_class_388_envelope(ctx, bytes, records, scope, (10, 1), 0)?
    else {
        return Ok(None);
    };
    super::legacy_class_388_operand_path(ctx, bytes, &envelope)
}

#[test]
fn legacy_path_occurrence_guids_refuse_collection_limit() {
    let bytes = path_fixture();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let scope = scope();
    let operation = "f3d legacy occurrence GUIDs";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| operand_path(ctx, &bytes, &records, &scope),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == operation)
    );
    crate::design::test_support::with_test_decode_context(|ctx| {
        let path = operand_path(ctx, &bytes, &records, &scope)
            .unwrap()
            .unwrap();
        assert_eq!(path.occurrence_guids().len(), 2);
        assert_eq!(
            path.occurrence_guids()[0].value.as_str(),
            "11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(
            path.occurrence_guids()[1].value.as_str(),
            "22222222-2222-2222-2222-222222222222"
        );
    });
}

#[test]
fn legacy_path_validation_copies_no_guid() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = path_fixture();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let scope = scope();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let envelope =
        super::exact_legacy_class_388_envelope(&ctx, &bytes, &records, &scope, (10, 1), 0)
            .unwrap()
            .expect("valid legacy operand path");
    assert_eq!(
        envelope.paths,
        [
            Some(path_locator::LEN),
            Some(path_locator::LEN + class_412_path::LEN)
        ]
    );
}

#[test]
fn legacy_path_header_check_reads_no_further_than_the_expected_header() {
    // No header closes the locator, and the stream runs on far past it. The
    // check pays for the bytes up to the expected header and no more.
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"451", 10);
    bytes.resize(4096, 0);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = u64::try_from(path_locator::LEN + 11).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(!super::next_header_is(ctx, &bytes, 1, path_locator::LEN).unwrap());
    });
}

#[test]
fn legacy_class_412_identity_guid_push_refuses_each_collection_item() {
    let bytes = path_fixture();
    let start = path_locator::LEN;
    assert!(super::legacy_class_412_path_layout(&bytes, start));
    let identity_guids = crate::design::test_support::with_test_decode_context(|ctx| {
        super::legacy_class_412_identity_guids(ctx, &bytes, start)
            .expect("valid legacy class-412 path")
    })
    .expect("four class-412 identity GUIDs");
    assert_eq!(identity_guids.len(), 4);

    for skip in 0..4 {
        let refusal = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "collect F3D legacy path identity GUIDs",
            skip,
            |ctx| super::legacy_class_412_identity_guids(ctx, &bytes, start).map(|_| ()),
        );
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "collect F3D legacy path identity GUIDs"
                    && limit.additional == 1
        ));
    }
}

#[test]
fn legacy_path_identity_guids_refuse_collection_limit() {
    let bytes = path_fixture();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let scope = scope();
    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::CollectionItems,
        "collect F3D legacy path identity GUIDs",
        0,
        |ctx| operand_path(ctx, &bytes, &records, &scope).map(|_| ()),
    );
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect F3D legacy path identity GUIDs"
                && limit.additional == 1
    ));
}
