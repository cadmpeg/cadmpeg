// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::align::align_prefix_fixture;

#[test]
fn align_prefix_decodes_both_forms_and_preserves_source_code() {
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        let (bytes, scope) = align_prefix_fixture(form);
        let envelope = decode_align_envelope(&bytes, &scope).unwrap();
        assert_eq!(envelope.form, form);
        assert_eq!(envelope.source_code.value, 7);
        assert_eq!(envelope.first_reference.value, 806);
        assert_eq!(envelope.second_reference.value, 802);
    }
}

#[test]
fn align_prefix_refuses_changed_constants_references_and_truncation() {
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        let (bytes, scope) = align_prefix_fixture(form);
        let shift = bytes.len() - compact::LEN;
        let mut offsets = vec![
            compact::HEADER_ZERO,
            compact::ZERO + shift,
            compact::FIRST_MARKER + shift,
            compact::FIRST_REFERENCE + shift,
            compact::FIRST_PADDING + shift,
            compact::SECOND_MARKER + shift,
            compact::SECOND_REFERENCE + shift,
            compact::SECOND_PADDING + shift,
            compact::IDENTITY_LENGTH + shift,
            compact::IDENTITY + shift,
            compact::TAIL_ZERO + shift,
        ];
        if form == DesignAlignForm::Extended {
            offsets.push(extended::EXTRA_CODE);
        }
        for offset in offsets {
            let mut changed = bytes.clone();
            changed[offset] ^= 2;
            assert!(
                decode_align_envelope(&changed, &scope).is_none(),
                "offset {offset}"
            );
        }
        for end in 0..bytes.len() {
            assert!(decode_align_envelope(&bytes[..end], &scope).is_none());
        }
    }
}

#[test]
fn align_native_paths_have_distinct_exact_losses() {
    let (bytes, scope) = align_prefix_fixture(DesignAlignForm::Compact);
    let mut envelope = decode_align_envelope(&bytes, &scope).unwrap();
    assert_eq!(
        align_native_loss(&scope, None).code.to_string(),
        "f3d/feature.align-envelope-unparsed"
    );
    assert_eq!(
        align_native_loss(&scope, Some(&envelope)).code.to_string(),
        "f3d/feature.align-construction-unparsed"
    );
    let bytes = crate::test_support::align::align_constructor_fixture(&scope);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    envelope.construction =
        construction::decode_align_construction(&bytes, &records, &scope, &envelope);
    assert!(envelope.construction.is_some());
    assert_eq!(
        align_native_loss(&scope, Some(&envelope)).code.to_string(),
        "f3d/feature.align-operands-unresolved"
    );
}

fn motion_type(guid: &str, version: u32) -> crate::records::entity_header::SegmentType {
    crate::records::entity_header::SegmentType::try_new(
        "f3d:Design/MetaStream.dat:design-type#0".into(),
        crate::design::test_support::design_type(guid, None, version, "Fusion", vec![]),
    )
    .unwrap()
}

#[test]
fn align_rigid_constructor_decodes_nullable_typed_and_omitted_matrix_forms() {
    use crate::design::decode::scopes::direct_face::{
        exact_align_operation, move_operation_links_hold,
    };
    use crate::records::feature::direct_face::{DesignMoveForm, DesignMoveReference};
    use crate::records::feature::scope::DesignScopePayloadMut;
    use crate::test_support::align::{align_motion_fixture, ALIGN_MOTION_MATRIX};

    let row = motion_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 2);
    let types = std::collections::HashMap::from([(500, &row)]);
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        for (leading, remaining, scalar, codes, omitted, typed, relative_form) in [
            ([true; 2], [false; 4], 0., [2, 0], false, false, 63),
            ([false; 2], [true; 4], 0.625, [1, 0], false, false, 83),
            ([false; 2], [true; 4], -0., [0, 0], false, false, 83),
            (
                [true; 2],
                [false; 4],
                0.625,
                [u32::MAX; 2],
                false,
                false,
                63,
            ),
            ([true; 2], [false; 4], 0., [2, 0], false, true, 143),
            ([false; 2], [true; 4], 0., [0, 0], false, true, 243),
            ([true; 2], [false; 4], 0., [2, 0], true, true, 143),
            ([false; 2], [true; 4], 0., [0, 0], true, true, 243),
            ([true; 2], [false; 4], 0., [2, 0], true, false, 63),
            ([false; 2], [true; 4], 0., [0, 0], true, false, 83),
            ([false; 2], [false; 4], 0., [0, 0], false, false, 43),
        ] {
            let fixture =
                align_motion_fixture(form, leading, remaining, scalar, codes, omitted, typed);
            let records =
                crate::design::test_support::indexed_record_offsets_for_test(&fixture.bytes);
            let mut envelope = decode_align_envelope(&fixture.bytes, &fixture.scope).unwrap();
            let operation =
                exact_align_operation(&fixture.bytes, &records, &envelope, &types).unwrap();
            assert_eq!(operation.form, DesignMoveForm::Form4);
            assert_eq!(operation.relative_form_offset(), Some(relative_form));
            assert_eq!(
                operation.form_offset,
                u64::try_from(fixture.form_offset).unwrap()
            );
            assert_eq!(
                operation.transform_offset,
                u64::try_from(fixture.matrix_offset).unwrap()
            );
            assert_eq!(
                fixture.form_offset - fixture.constructor_offset,
                relative_form
            );
            let expected = if omitted {
                crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows()
            } else {
                ALIGN_MOTION_MATRIX
            };
            assert_eq!(operation.transform.rows(), expected);
            assert!(operation.prefix.is_some());
            if let Some(prefix) = &operation.prefix {
                assert_eq!(prefix.codes, codes);
                assert_eq!(prefix.matrix_omitted, omitted);
                assert!(prefix.leading.is_some());
                if let Some(fields) = &prefix.leading {
                    assert_eq!(fields.scalar.get().to_bits(), scalar.to_bits());
                }
                let actual_leading = prefix.leading.as_ref().map(|fields| &fields.references);
                for (ordinal, present) in leading.into_iter().chain(remaining).enumerate() {
                    let reference = if ordinal < 2 {
                        actual_leading.map(|values| &values[ordinal])
                    } else {
                        Some(&prefix.references[ordinal - 2])
                    };
                    let index = 810 + u32::try_from(ordinal).unwrap() * 2;
                    match (reference, present, typed) {
                        (None | Some(DesignMoveReference::Null), false, _) => {}
                        (Some(DesignMoveReference::Local { record_index }), true, false) => {
                            assert_eq!(*record_index, index);
                        }
                        (
                            Some(DesignMoveReference::Typed {
                                record_index,
                                type_guid,
                            }),
                            true,
                            true,
                        ) => {
                            assert_eq!(*record_index, index);
                            assert_eq!(type_guid, "84E6245F-1CDC-4FB8-8048-163A85FAD3F0");
                        }
                        _ => panic!("reference {ordinal}"),
                    }
                }
            } else {
                assert!(!omitted && leading == [false; 2] && remaining == [false; 4]);
            }
            let header = crate::records::decal::DesignRecordHeader {
                id: "f3d:Design/BulkStream.dat:design-record#806".into(),
                record_index: 806,
                byte_offset: u64::try_from(fixture.constructor_offset).unwrap(),
                class_tag: "500".to_owned().try_into().unwrap(),
            };
            envelope.motion = Some(operation);
            let mut scope = fixture.scope;
            let DesignScopePayloadMut::Align(slot) = scope.payload_mut() else {
                panic!("Align");
            };
            *slot = Some(envelope.clone());
            assert!(align_envelope_links_hold(&scope, &envelope));
            assert!(move_operation_links_hold(&scope, Some(&header)));
        }
    }
}

#[test]
fn align_rigid_constructor_rejects_malformed_fields_types_and_truncation() {
    use crate::design::decode::scopes::direct_face::exact_align_operation;
    use crate::test_support::align::align_motion_fixture;

    let fixture = align_motion_fixture(
        DesignAlignForm::Compact,
        [true; 2],
        [false; 4],
        0.,
        [2, 0],
        false,
        false,
    );
    let envelope = decode_align_envelope(&fixture.bytes, &fixture.scope).unwrap();
    let row = motion_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 2);
    let types = std::collections::HashMap::from([(500, &row)]);
    for violation in 0..10 {
        let mut bytes = fixture.bytes.clone();
        match violation {
            0 => bytes[fixture.constructor_offset + 11] = 1,
            1 => bytes[fixture.constructor_offset + 21..fixture.constructor_offset + 29]
                .copy_from_slice(&f64::NAN.to_le_bytes()),
            2 => bytes[fixture.constructor_offset + 29] = 2,
            3 => bytes[fixture.constructor_offset + 38] = 1,
            4 => bytes[fixture.constructor_offset + 30..fixture.constructor_offset + 38]
                .copy_from_slice(&u64::MAX.to_le_bytes()),
            5 => bytes[fixture.form_offset..fixture.form_offset + 4]
                .copy_from_slice(&0_u32.to_le_bytes()),
            6 => bytes[fixture.form_offset + 4] = 2,
            7 => bytes[fixture.matrix_offset..fixture.matrix_offset + 8]
                .copy_from_slice(&f64::INFINITY.to_le_bytes()),
            8 => bytes[fixture.matrix_offset + 96..fixture.matrix_offset + 104]
                .copy_from_slice(&1_f64.to_le_bytes()),
            _ => bytes[fixture.matrix_offset..fixture.matrix_offset + 8]
                .copy_from_slice(&2_f64.to_le_bytes()),
        }
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        assert!(
            exact_align_operation(&bytes, &records, &envelope, &types).is_none(),
            "violation {violation}"
        );
    }
    let records = crate::design::test_support::indexed_record_offsets_for_test(&fixture.bytes);
    for (guid, version) in [
        ("11111111-2222-3333-4444-555555555555", 2),
        ("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 3),
    ] {
        let wrong = motion_type(guid, version);
        assert!(exact_align_operation(
            &fixture.bytes,
            &records,
            &envelope,
            &std::collections::HashMap::from([(500, &wrong)])
        )
        .is_none());
    }
    assert!(exact_align_operation(
        &fixture.bytes,
        &records,
        &envelope,
        &std::collections::HashMap::new()
    )
    .is_none());
    for end in 0..fixture.bytes.len() {
        let bytes = &fixture.bytes[..end];
        let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
        assert!(
            exact_align_operation(bytes, &records, &envelope, &types).is_none(),
            "end {end}"
        );
    }
}

#[test]
fn align_omitted_matrix_rejects_hidden_payload_and_changed_tail_length() {
    use crate::design::decode::scopes::direct_face::exact_align_operation;
    use crate::test_support::align::align_motion_fixture;

    let row = motion_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 2);
    let types = std::collections::HashMap::from([(500, &row)]);
    for omitted in [false, true] {
        let mut fixture = align_motion_fixture(
            DesignAlignForm::Extended,
            [true; 2],
            [false; 4],
            0.,
            [2, 0],
            omitted,
            false,
        );
        let envelope = decode_align_envelope(&fixture.bytes, &fixture.scope).unwrap();
        if omitted {
            let end = fixture.bytes.len() - 11;
            fixture.bytes.drain(end - 2..end);
        } else {
            fixture.bytes[fixture.form_offset + 4] = 1;
        }
        let records = crate::design::test_support::indexed_record_offsets_for_test(&fixture.bytes);
        assert!(exact_align_operation(&fixture.bytes, &records, &envelope, &types).is_none());
    }
}
