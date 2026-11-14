use crate::design::decode::scopes::direct_face::{
    exact_move_operation, move_operation_links_hold, move_transform_prefix_losses,
    move_transform_references_loss,
};
use crate::records::feature::direct_face::DesignMoveReference;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};

const TYPE: &str = "84E6245F-1CDC-4FB8-8048-163A85FAD3F0";
const MATRIX: [[f64; 4]; 4] = [
    [0., -1., 0., 2.5],
    [1., 0., 0., -4.25],
    [0., 0., 1., 6.75],
    [0., 0., 0., 1.],
];

fn reference(bytes: &mut Vec<u8>, target: u32, typed: bool) {
    bytes.push(1);
    bytes.extend_from_slice(&u64::from(target).to_le_bytes());
    if typed {
        bytes.extend_from_slice(&36u32.to_le_bytes());
        bytes.extend_from_slice(TYPE.as_bytes());
    }
    bytes.extend_from_slice(&[0, 0]);
}

fn header(bytes: &mut Vec<u8>, class: &[u8; 3], index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(class);
    bytes.extend_from_slice(&index.to_le_bytes());
}

fn fixture(
    typed: bool,
    references: [bool; 4],
    codes: [u32; 2],
    form: u32,
    marker_one: bool,
) -> (Vec<u8>, DesignParameterScope, usize, usize) {
    let mut bytes = vec![0; 17];
    header(&mut bytes, b"500", 71);
    bytes.extend_from_slice(&[0; 20]);
    for (ordinal, (present, target)) in references.into_iter().zip([80, 81, 82, 83]).enumerate() {
        if ordinal == 3 {
            bytes.extend_from_slice(&codes[0].to_le_bytes());
        }
        if present {
            reference(&mut bytes, target, typed);
        } else {
            bytes.push(0);
        }
    }
    bytes.extend_from_slice(&codes[1].to_le_bytes());
    let form_at = bytes.len();
    bytes.extend_from_slice(&form.to_le_bytes());
    bytes.push(0);
    let matrix_at = bytes.len();
    for value in MATRIX.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 78]);
    header(&mut bytes, b"501", 71);
    let start = bytes.len();
    header(&mut bytes, b"502", 17);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(u8::from(marker_one));
    if marker_one {
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    reference(&mut bytes, 71, typed);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 50, typed);
    let count_offset = if marker_one {
        130
    } else if typed {
        206
    } else {
        126
    };
    bytes.resize(start + count_offset + 300, 0);
    let mut scope = DesignParameterScope::empty("f3d:test:scope#17", DesignFeatureKind::Move, 17);
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64::try_from(start).unwrap();
            draft.reference_count_offset = u64::try_from(start + count_offset).unwrap();
            draft.frame_length = u64::try_from(count_offset + 150).unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 60, 71, 90]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, form_at, matrix_at)
}

#[test]
fn nullable_transform_slots_preserve_references_codes_matrix_and_source_links() {
    let row = super::move_transform_type(TYPE, 2);
    let types = super::move_transform_types(&row, &[500]);
    for (typed, references, codes, form, marker) in [
        (true, [false; 4], [0, 0], 5, false),
        (true, [true; 4], [0, 0], 2, false),
        (false, [true; 4], [0, 0], 2, true),
        (false, [true, false, false, false], [0, 0], 6, false),
        (false, [false; 4], [0, 1], 0, false),
        (false, [false; 4], [0, 2], 0, false),
        (false, [false; 4], [0, 0], 6, false),
        (false, [false; 4], [0, 0], 4, false),
    ] {
        let (bytes, mut scope, form_at, matrix_at) =
            fixture(typed, references, codes, form, marker);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let operation = exact_move_operation(&bytes, &records, &scope, &types).unwrap();
        assert_eq!(operation.form_offset, u64::try_from(form_at).unwrap());
        assert_eq!(
            operation.transform_offset,
            u64::try_from(matrix_at).unwrap()
        );
        assert_eq!(u32::from(operation.form), form);
        for (actual, expected) in operation
            .transform
            .rows()
            .into_iter()
            .flatten()
            .zip(MATRIX.into_iter().flatten())
        {
            assert!((actual - expected).abs() < 1e-12);
        }
        if let Some(prefix) = &operation.prefix {
            assert_eq!(prefix.codes, codes);
            for (ordinal, (reference, present)) in
                prefix.references.iter().zip(references).enumerate()
            {
                let index = 80 + u32::try_from(ordinal).unwrap();
                match (reference, present, typed) {
                    (DesignMoveReference::Null, false, _) => {}
                    (DesignMoveReference::Local { record_index }, true, false) => {
                        assert_eq!(*record_index, index);
                    }
                    (
                        DesignMoveReference::Typed {
                            record_index,
                            type_guid,
                        },
                        true,
                        true,
                    ) => {
                        assert_eq!(*record_index, index);
                        assert_eq!(type_guid, TYPE);
                    }
                    _ => panic!("prefix reference"),
                }
            }
        } else {
            assert!(!references.into_iter().any(|value| value) && codes == [0, 0]);
        }
        assert_eq!(
            move_transform_references_loss(&scope, &operation).is_some(),
            form == 2
        );
        let losses = move_transform_prefix_losses(&scope, &operation);
        assert_eq!(
            losses.iter().any(|note| note.code
                == crate::loss::F3dLossCode::FeatureMoveTransformModeAssumed.kind()),
            form == 4 || form == 6
        );
        assert_eq!(
            losses.iter().any(|note| note.code
                == crate::loss::F3dLossCode::FeatureMoveTransformPrefixAssumed.kind()),
            codes != [0, 0] || references != [false; 4] && form != 2
        );
        let DesignScopePayloadMut::Move(slot) = scope.payload_mut() else {
            panic!("Move");
        };
        *slot = Some(operation);
        let header = crate::records::decal::DesignRecordHeader {
            id: "f3d:test:header#17".into(),
            byte_offset: 17,
            class_tag: "500".to_owned().try_into().unwrap(),
            record_index: 71,
        };
        assert!(move_operation_links_hold(&scope, Some(&header)));
    }
}

#[test]
fn transform_rejects_bad_references_nonfinite_values_and_truncation() {
    for typed in [false, true] {
        let (bytes, scope, form_at, matrix_at) = fixture(typed, [true; 4], [0, 0], 2, false);
        let mut invalid = Vec::new();
        for offset in [17 + 20, 17 + 31, matrix_at - 1] {
            let mut changed = bytes.clone();
            changed[offset] ^= 4;
            invalid.push(changed);
        }
        let mut changed = bytes.clone();
        changed[form_at] = 3;
        invalid.push(changed);
        let mut changed = bytes.clone();
        changed[17 + 31 + 5] = 1;
        invalid.push(changed);
        if typed {
            let mut changed = bytes.clone();
            changed[17 + 31 + 13] = b'G';
            invalid.push(changed);
        }
        let mut changed = bytes.clone();
        changed[matrix_at..matrix_at + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        invalid.push(changed);
        invalid.push(bytes[..matrix_at + 127].to_vec());
        for changed in invalid {
            let records = crate::design::test_support::indexed_record_offsets_for_test(&changed);
            assert!(exact_move_operation(
                &changed,
                &records,
                &scope,
                &std::collections::HashMap::new()
            )
            .is_none());
        }
    }
}

#[test]
fn unsigned_prefix_codes_survive_with_a_named_motion_assumption() {
    let (bytes, scope, _, _) = fixture(false, [true; 4], [7, 11], 2, false);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let operation =
        exact_move_operation(&bytes, &records, &scope, &std::collections::HashMap::new()).unwrap();
    assert_eq!(operation.prefix.as_ref().unwrap().codes, [7, 11]);
    assert_eq!(
        move_transform_prefix_losses(&scope, &operation)[0].code,
        crate::loss::F3dLossCode::FeatureMoveTransformPrefixAssumed.kind()
    );
}

#[test]
fn omitted_matrix_projects_assumed_identity_and_keeps_the_omission_flag() {
    let (mut bytes, mut scope, _, matrix_at) = fixture(false, [false; 4], [0, 0], 5, true);
    bytes[matrix_at - 1] = 1;
    bytes.drain(matrix_at..matrix_at + 128);
    scope
        .try_edit(|draft| {
            draft.byte_offset -= 128;
            draft.reference_count_offset -= 128;
            draft.paired_byte_offset -= 128;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let operation =
        exact_move_operation(&bytes, &records, &scope, &std::collections::HashMap::new()).unwrap();
    assert!(operation.prefix.as_ref().unwrap().matrix_omitted);
    for (actual, expected) in operation.transform.rows().into_iter().flatten().zip([
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]) {
        assert!((actual - expected).abs() < 1e-12);
    }
    assert_eq!(
        move_transform_prefix_losses(&scope, &operation)[0].code,
        crate::loss::F3dLossCode::FeatureMoveTransformIdentityAssumed.kind()
    );
}
