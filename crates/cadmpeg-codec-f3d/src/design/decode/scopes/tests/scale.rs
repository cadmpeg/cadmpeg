// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::direct_face::exact_scale_operation;
use crate::records::feature::scope::DesignParameterScope;
use std::collections::HashMap;

const EPS_SCALE_VALUE: f64 = 1.0e-12;

#[test]
fn legacy_scale_resolves_explicit_point_data_center() {
    for extra_reference in [false, true] {
        let (bytes, scope, position_at) = legacy_scale_fixture(extra_reference);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let operation =
            exact_scale_operation(&bytes, &records, &scope, &HashMap::new(), &mut Vec::new())
                .unwrap()
                .expect("legacy Scale operation");

        assert_eq!(operation.body_group_record_index, 102);
        assert_eq!(operation.center_record_index, 105);
        assert_eq!(
            operation.center_position.map(|center| center.offset),
            Some(u64_from_index(position_at))
        );
        assert_eq!(operation.uniform().unwrap().1, 21);
        assert!((operation.uniform().unwrap().0.get() - 2.5).abs() < EPS_SCALE_VALUE);

        let position = operation.center_position.expect("point-data center").value;
        for (actual, expected) in position.into_iter().zip([1.25, -2.5, 3.75]) {
            assert!((actual.get() - expected).abs() < EPS_SCALE_VALUE);
        }
    }
}

#[test]
fn modern_localized_scale_resolves_explicit_point_data_center() {
    let (bytes, scope, position_at) = modern_scale_fixture();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let operation =
        exact_scale_operation(&bytes, &records, &scope, &HashMap::new(), &mut Vec::new())
            .unwrap()
            .expect("modern localized Scale operation");

    assert_eq!(operation.body_group_record_index, 102);
    assert_eq!(operation.center_record_index, 105);
    assert_eq!(
        operation.center_position.map(|center| center.offset),
        Some(u64_from_index(position_at))
    );
    assert_eq!(operation.uniform().unwrap().1, 25);
    assert!((operation.uniform().unwrap().0.get() - 2.5).abs() < EPS_SCALE_VALUE);

    let position = operation.center_position.expect("point-data center").value;
    for (actual, expected) in position.into_iter().zip([1.25, -2.5, 3.75]) {
        assert!((actual.get() - expected).abs() < EPS_SCALE_VALUE);
    }
}

fn modern_scale_fixture() -> (Vec<u8>, DesignParameterScope, usize) {
    let mut bytes = vec![0; 317];
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[25..33].copy_from_slice(&2.5f64.to_le_bytes());
    for (offset, record_index) in [(33, 105u32), (44, 101), (68, 102)] {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    bytes[55..59].copy_from_slice(&1u32.to_le_bytes());
    bytes[60..64].copy_from_slice(&1u32.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());

    let position_at = append_point_data(&mut bytes, 105, [104, 103]);
    let mut scope = DesignParameterScope::empty(
        "generated:scale#100",
        crate::records::feature::scope::DesignFeatureKind::Scale,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 317;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![101, 102, 103, 104, 105]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.source_kind = Some("Maßstab".to_owned());
    (bytes, scope, position_at)
}

fn legacy_scale_fixture(extra_reference: bool) -> (Vec<u8>, DesignParameterScope, usize) {
    let frame_length = if extra_reference { 318 } else { 307 };
    let reference_members = if extra_reference {
        vec![101, 102, 103, 104, 106, 105]
    } else {
        vec![101, 102, 103, 104, 105]
    };
    let mut bytes = vec![0; frame_length];
    bytes[21..29].copy_from_slice(&2.5f64.to_le_bytes());
    for (offset, record_index) in [(29, 105u32), (40, 101), (64, 102)] {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    bytes[51..55].copy_from_slice(&1u32.to_le_bytes());
    bytes[56..60].copy_from_slice(&1u32.to_le_bytes());
    bytes[60..64].copy_from_slice(&1u32.to_le_bytes());

    let position_at = append_point_data(&mut bytes, 105, [104, 103]);
    let mut scope = DesignParameterScope::empty(
        "generated:scale#100",
        crate::records::feature::scope::DesignFeatureKind::Scale,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(frame_length);
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(reference_members);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, position_at)
}

fn append_point_data(bytes: &mut Vec<u8>, record_index: u32, input_records: [u32; 2]) -> usize {
    let point_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 27]);
    let position_at = bytes.len();
    for value in [1.25f64, -2.5, 3.75] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&(-1.0f64).to_le_bytes());
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in input_records {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.resize(point_at + 208, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    position_at
}

#[test]
fn matrix_scale_decodes_uniform_and_axis_factors_with_center() {
    use crate::records::feature::body_ops::DesignScaleFactors;
    for marker in [0, 1] {
        for per_axis in [false, true] {
            let (bytes, scope, matrix_at) = matrix_scale_fixture(marker, per_axis);
            let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
            let mut losses = Vec::new();
            let operation =
                exact_scale_operation(&bytes, &records, &scope, &HashMap::new(), &mut losses)
                    .unwrap()
                    .unwrap();
            assert_eq!(operation.body_group_record_index, 102);
            assert_eq!(operation.center_record_index, 105);
            assert_eq!(operation.matrix_offset, Some(u64_from_index(matrix_at)));
            let center = operation.center_position.unwrap();
            assert_eq!(
                center.value.map(cadmpeg_ir::scalar::FiniteReal::get),
                [1.25, -2.5, 3.75]
            );
            match operation.factors {
                DesignScaleFactors::Uniform { factor, .. } => {
                    assert!(!per_axis);
                    assert_eq!(factor.get(), 2.0);
                }
                DesignScaleFactors::PerAxis { factors, .. } => {
                    assert!(per_axis);
                    assert_eq!(
                        factors.map(cadmpeg_ir::scalar::PositiveReal::get),
                        [1.0, 2.0, 3.0]
                    );
                }
            }
            assert!(losses.is_empty());
        }
    }
}

#[test]
fn matrix_scale_rejects_invalid_transform_and_guid() {
    for (relative, value) in [(0, -1.0f64), (8, 0.5), (24, 7.0), (120, 0.0)] {
        let (mut bytes, scope, at) = matrix_scale_fixture(0, false);
        bytes[at + relative..at + relative + 8].copy_from_slice(&value.to_le_bytes());
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        assert!(
            exact_scale_operation(&bytes, &records, &scope, &HashMap::new(), &mut Vec::new())
                .unwrap()
                .is_none()
        );
    }
    let (mut bytes, scope, at) = matrix_scale_fixture(0, true);
    bytes[at + 40..at + 48].copy_from_slice(&1.0f64.to_le_bytes());
    bytes[at + 56..at + 64].copy_from_slice(&0.0f64.to_le_bytes());
    bytes[at + 80..at + 88].copy_from_slice(&1.0e308f64.to_le_bytes());
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert!(
        exact_scale_operation(&bytes, &records, &scope, &HashMap::new(), &mut Vec::new())
            .unwrap()
            .is_none()
    );
    let (mut bytes, scope, at) = matrix_scale_fixture(0, false);
    bytes[at + 132] = b'1';
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert!(
        exact_scale_operation(&bytes, &records, &scope, &HashMap::new(), &mut Vec::new())
            .unwrap()
            .is_none()
    );
}

#[test]
fn matrix_scale_names_nonzero_prologue_word_assumption() {
    let (mut bytes, scope, _) = matrix_scale_fixture(1, false);
    bytes[25..29].copy_from_slice(&7u32.to_le_bytes());
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut losses = Vec::new();
    assert!(
        exact_scale_operation(&bytes, &records, &scope, &HashMap::new(), &mut losses)
            .unwrap()
            .is_some()
    );
    assert_eq!(losses, vec![crate::loss::F3dLossCode::FeatureScaleMatrixWordAssumed.note(
        "generated:scale#100 stores word 0x00000007 in its matrix Scale prologue; the Scale assumes that the word leaves the stored matrix unchanged (DR-463)."
    )]);
}

fn matrix_scale_fixture(marker: u8, per_axis: bool) -> (Vec<u8>, DesignParameterScope, usize) {
    let mut bytes = vec![0; 500];
    bytes[20] = marker;
    let count_at = if marker == 0 { 25 } else { 29 };
    bytes[count_at..count_at + 4].copy_from_slice(&1u32.to_le_bytes());
    let mut at = count_at + 4;
    for index in [102u32, 105] {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
        at += 11;
    }
    for index in if per_axis {
        [None, Some(101u32), Some(103), Some(104)]
    } else {
        [Some(101), None, None, None]
    } {
        if let Some(index) = index {
            bytes[at] = 1;
            bytes[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
            at += 11;
        } else {
            at += 1;
        }
    }
    at += 1;
    let matrix: [f64; 16] = if per_axis {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 2.5, 0.0, 0.0, 3.0, -7.5, 0.0, 0.0, 0.0, 1.0,
        ]
    } else {
        [
            2.0, 0.0, 0.0, -1.25, 0.0, 2.0, 0.0, 2.5, 0.0, 0.0, 2.0, -3.75, 0.0, 0.0, 0.0, 1.0,
        ]
    };
    for (cell, value) in matrix.into_iter().enumerate() {
        bytes[at + 8 * cell..at + 8 * cell + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[at + 128..at + 132].copy_from_slice(&36u32.to_le_bytes());
    for (i, code) in "00000000-0000-0000-0000-000000000000"
        .encode_utf16()
        .enumerate()
    {
        bytes[at + 132 + 2 * i..at + 134 + 2 * i].copy_from_slice(&code.to_le_bytes());
    }
    append_point_data(&mut bytes, 105, [104, 103]);
    let mut scope = DesignParameterScope::empty(
        "generated:scale#100",
        crate::records::feature::scope::DesignFeatureKind::Scale,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 500;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![101, 102, 103, 104, 105]);
            draft.paired_byte_offset = 500;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, at)
}

#[test]
fn modern_scale_accepts_thirty_body_members_and_rejects_mismatched_order() {
    let (mut bytes, mut scope, _) = modern_scale_fixture();
    let mut references = vec![101, 102];
    references.extend(200..230);
    references.extend([104, 105]);
    scope
        .try_edit(|draft| {
            draft.frame_length = 636;
            draft.paired_byte_offset = 636;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"123");
    bytes.extend_from_slice(&102u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&30u32.to_le_bytes());
    for index in 200u32..230 {
        bytes.push(1);
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"124");
    bytes.extend_from_slice(&102u32.to_le_bytes());
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let operation =
        exact_scale_operation(&bytes, &records, &scope, &HashMap::new(), &mut Vec::new())
            .unwrap()
            .unwrap();
    assert_eq!(operation.uniform().unwrap().0.get(), 2.5);
    scope
        .try_edit(|draft| {
            let mut references = vec![101, 102];
            references.extend(201..231);
            references.extend([104, 105]);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(
        exact_scale_operation(&bytes, &records, &scope, &HashMap::new(), &mut Vec::new())
            .unwrap()
            .is_none()
    );
}
