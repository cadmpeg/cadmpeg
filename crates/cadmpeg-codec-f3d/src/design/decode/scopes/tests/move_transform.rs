// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::direct_face::{
    exact_modify_operation, exact_move_face_operation, exact_move_operation,
    marked_move_transform_loss, move_transform_references_loss,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

fn fixture(class: &str, length: usize) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(class.as_bytes());
    bytes[7..11].copy_from_slice(&71u32.to_le_bytes());
    let form: u32 = match length {
        253 | 254 => 5,
        283 | 284 => 0,
        _ => 1,
    };
    bytes[43..47].copy_from_slice(&form.to_le_bytes());
    let transform: [[f64; 4]; 4] = [
        [0., -1., 0., 7.],
        [1., 0., 0., 11.],
        [0., 0., 1., 13.],
        [0., 0., 0., 1.],
    ];
    for (cell, value) in transform.into_iter().flatten().enumerate() {
        let at = 48 + cell * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&71u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty("f3d:test:scope#17", DesignFeatureKind::Move, 17);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![71]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn move_transform_added_classes_decode_exact_matrix() {
    let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 2);
    let class_types = super::move_transform_types(
        &row,
        &[330, 273, 408, 372, 289, 394, 406, 282, 403, 391, 452, 500],
    );
    for (class, length) in [
        ("330", 273),
        ("273", 273),
        ("408", 273),
        ("372", 273),
        ("289", 273),
        ("394", 273),
        ("406", 273),
        ("282", 273),
        ("403", 273),
        ("391", 273),
        ("452", 253),
        ("500", 254),
        ("500", 274),
    ] {
        let (bytes, scope) = fixture(class, length);
        let decoded = exact_move_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &class_types,
        )
        .unwrap();
        assert_eq!(
            decoded.transform.rows(),
            [
                [0., -1., 0., 7.],
                [1., 0., 0., 11.],
                [0., 0., 1., 13.],
                [0., 0., 0., 1.],
            ]
        );
        assert_eq!(
            u32::from(decoded.form),
            if length == 253 || length == 254 { 5 } else { 1 }
        );
        assert_eq!(decoded.form_offset, 43);
        assert_eq!(decoded.transform_offset, 48);
        assert_eq!(decoded.transform_record_index, 71);
    }
}

/// Places the reference table, kind, and paired header of a `MoveFace` frame.
fn lay_out_move_face(
    scope: &mut DesignParameterScope,
    start: usize,
    count_offset: u64,
    references: Vec<u32>,
) {
    let start = u64::try_from(start).unwrap();
    let kind_offset = count_offset + 12 + 11 * u64::try_from(references.len()).unwrap();
    scope
        .try_edit(|draft| {
            draft.byte_offset = start;
            draft.reference_count_offset = start + count_offset;
            draft.kind_offset = start + kind_offset;
            draft.frame_length = kind_offset + 93;
            draft.paired_byte_offset = start + draft.frame_length;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
}

/// A class-433 transform record 71, then a `MoveFace` scope frame that names
/// group 50, member 60, and that transform.
fn move_face_fixture(marker_one: bool) -> (Vec<u8>, DesignParameterScope, usize) {
    move_face_fixture_with("433", 253, DesignFeatureKind::MoveFace, marker_one)
}

fn move_face_fixture_with(
    class: &str,
    length: usize,
    kind: DesignFeatureKind,
    marker_one: bool,
) -> (Vec<u8>, DesignParameterScope, usize) {
    move_face_over(fixture(class, length).0, kind, marker_one)
}

fn move_face_over(
    mut bytes: Vec<u8>,
    kind: DesignFeatureKind,
    marker_one: bool,
) -> (Vec<u8>, DesignParameterScope, usize) {
    let start = bytes.len();
    bytes.resize(start + 200, 0);
    let marker = if marker_one {
        bytes[start + 20] = 1;
        26
    } else {
        22
    };
    bytes[start + marker - 1] = 1;
    bytes[start + marker] = 1;
    bytes[start + marker + 1..start + marker + 5].copy_from_slice(&71u32.to_le_bytes());
    bytes[start + marker + 11..start + marker + 15].copy_from_slice(&1u32.to_le_bytes());
    bytes[start + marker + 15] = 1;
    bytes[start + marker + 16..start + marker + 20].copy_from_slice(&50u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty("f3d:test:scope#17", kind, 17);
    lay_out_move_face(
        &mut scope,
        start,
        if marker_one { 131 } else { 127 },
        vec![50, 60, 71],
    );
    (bytes, scope, marker)
}

fn decoded_move_face(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<crate::records::feature::direct_face::DesignMoveOperation> {
    exact_move_face_operation(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
        &std::collections::HashMap::new(),
        &mut Vec::new(),
    )
}

#[test]
fn move_face_decodes_both_prologues_and_refuses_broken_transform_markers() {
    for marker_one in [true, false] {
        let (bytes, scope, marker) = move_face_fixture(marker_one);
        let decoded = decoded_move_face(&bytes, &scope).unwrap();
        assert_eq!(decoded.transform_record_index, 71);
        assert_eq!(decoded.transform_offset, 48);
        let start = usize::try_from(scope.byte_offset()).unwrap();
        for at in [marker - 1, marker, marker + 1, marker + 11] {
            let mut changed = bytes.clone();
            changed[start + at] ^= 0x02;
            assert!(
                decoded_move_face(&changed, &scope).is_none(),
                "offset {at} accepted"
            );
        }
        let mut shifted = scope.clone();
        lay_out_move_face(
            &mut shifted,
            start,
            if marker_one { 132 } else { 128 },
            vec![50, 60, 71],
        );
        assert!(decoded_move_face(&bytes, &shifted).is_none());
    }

    let (mut bytes, scope, _) = move_face_fixture(true);
    let start = usize::try_from(scope.byte_offset()).unwrap();
    bytes[start + 21] = 1;
    assert!(decoded_move_face(&bytes, &scope).is_none());

    let (bytes, mut no_member, _) = move_face_fixture(true);
    let start = usize::try_from(no_member.byte_offset()).unwrap();
    lay_out_move_face(&mut no_member, start, 131, vec![50, 71]);
    assert!(decoded_move_face(&bytes, &no_member).is_some());
}

#[test]
fn move_face_transform_can_be_the_first_reference() {
    for marker_one in [true, false] {
        let (bytes, mut scope, _) = move_face_fixture(marker_one);
        let start = usize::try_from(scope.byte_offset()).unwrap();
        let count = if marker_one { 131 } else { 127 };
        lay_out_move_face(&mut scope, start, count, vec![71, 50, 60]);
        let decoded = decoded_move_face(&bytes, &scope).unwrap();
        assert_eq!(decoded.transform_record_index, 71);

        for references in [vec![71, 60, 50], vec![60, 50, 71], vec![60, 71, 50]] {
            lay_out_move_face(&mut scope, start, count, references);
            assert!(decoded_move_face(&bytes, &scope).is_some());
        }
    }
}

#[test]
fn move_face_transform_is_selected_by_its_reference_not_by_class_or_length() {
    let kind = DesignFeatureKind::MoveFace;
    for (class, length) in [("313", 256), ("396", 256), ("999", 300), ("433", 253)] {
        let (bytes, scope, _) = move_face_fixture_with(class, length, kind.clone(), false);
        let decoded = decoded_move_face(&bytes, &scope).unwrap();
        assert_eq!(decoded.transform_record_index, 71);
        assert_eq!(decoded.form_offset, 43);
        assert_eq!(decoded.transform_offset, 48);
        assert_eq!(decoded.transform.rows()[1], [1., 0., 0., 11.]);
    }
}

#[test]
fn move_face_transform_refuses_a_short_frame_reserved_bytes_and_a_broken_matrix() {
    let refused = |bytes: &[u8], scope: &DesignParameterScope| {
        assert!(decoded_move_face(bytes, scope).is_none());
    };
    let (bytes, scope, _) = move_face_fixture_with("313", 256, DesignFeatureKind::MoveFace, false);
    let (mut short, _) = fixture("313", 176);
    short.remove(175);
    let (short, short_scope, _) = move_face_over(short, DesignFeatureKind::MoveFace, false);
    refused(&short, &short_scope);
    for (at, value) in [(11, 1), (30, 1), (43, 2), (47, 1)] {
        let mut changed = bytes.clone();
        changed[at] = value;
        refused(&changed, &scope);
    }
    let mut coded = bytes.clone();
    coded[42] = 1;
    let operation = decoded_move_face(&coded, &scope).unwrap();
    assert_eq!(operation.prefix.as_ref().unwrap().codes, [0, 1 << 24]);
    let notes = crate::design::decode::scopes::direct_face::move_transform_prefix_losses(
        &scope, &operation,
    );
    assert_eq!(
        notes[0].code,
        crate::loss::F3dLossCode::FeatureMoveTransformPrefixAssumed.kind()
    );
    let mut changed = bytes.clone();
    changed[168..176].copy_from_slice(&2.0f64.to_le_bytes());
    refused(&changed, &scope);
}

#[test]
fn move_face_transform_refuses_a_record_with_two_admitted_frames() {
    let (first, _) = fixture("313", 256);
    let second_frame = first.len();
    let mut frames = first;
    frames.extend_from_slice(&fixture("313", 256).0);
    let (bytes, scope, _) = move_face_over(frames.clone(), DesignFeatureKind::MoveFace, false);
    assert!(decoded_move_face(&bytes, &scope).is_none());

    frames[second_frame + 168..second_frame + 176].copy_from_slice(&2.0f64.to_le_bytes());
    let (bytes, scope, _) = move_face_over(frames, DesignFeatureKind::MoveFace, false);
    assert_eq!(
        decoded_move_face(&bytes, &scope).unwrap().transform_offset,
        48
    );
}

#[test]
fn move_transform_refuses_wrong_length_controls_and_matrix() {
    let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 2);
    let class_types = super::move_transform_types(
        &row,
        &[330, 273, 408, 372, 289, 394, 406, 282, 403, 391, 452, 500],
    );
    let (bytes, scope) = fixture("330", 273);
    let mut variants = vec![fixture("330", 272).0];
    for (at, value) in [(11, 1), (43, 2), (47, 1)] {
        let mut changed = bytes.clone();
        changed[at] = value;
        variants.push(changed);
    }
    let mut changed = bytes.clone();
    changed[168..176].copy_from_slice(&2.0f64.to_le_bytes());
    variants.push(changed);
    let mut changed = bytes.clone();
    changed[43..47].copy_from_slice(&5u32.to_le_bytes());
    variants.push(changed);
    let mut changed = fixture("452", 253).0;
    changed[43..47].copy_from_slice(&1u32.to_le_bytes());
    variants.push(changed);
    for bytes in variants {
        assert!(exact_move_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &class_types,
        )
        .is_none());
    }
}

/// A transform record of `class` and `length`, then a `Move` scope frame
/// whose prologue marks that record and the body group 50.
fn marked_move_fixture(class: &str, length: usize) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = fixture(class, length).0;
    let start = bytes.len();
    bytes.resize(start + 200, 0);
    bytes[start + 21] = 1;
    bytes[start + 22..start + 26].copy_from_slice(&71u32.to_le_bytes());
    bytes[start + 32..start + 36].copy_from_slice(&1u32.to_le_bytes());
    bytes[start + 36] = 1;
    bytes[start + 37..start + 41].copy_from_slice(&50u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty("f3d:test:scope#17", DesignFeatureKind::Move, 17);
    lay_out_move_face(&mut scope, start, 126, vec![50, 60, 71]);
    (bytes, scope)
}

fn decoded_move(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<crate::records::feature::direct_face::DesignMoveOperation> {
    let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 2);
    let class_types = super::move_transform_types(
        &row,
        &[330, 273, 408, 372, 289, 394, 406, 282, 403, 391, 452, 500],
    );
    exact_move_operation(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
        &class_types,
    )
}

#[test]
fn move_transform_is_selected_by_the_marked_prologue_reference() {
    for (class, length) in [("332", 256), ("999", 300), ("452", 253)] {
        let (bytes, scope) = marked_move_fixture(class, length);
        let start = usize::try_from(scope.byte_offset()).unwrap();
        let decoded = decoded_move(&bytes, &scope).unwrap();
        assert_eq!(decoded.transform_record_index, 71);
        assert_eq!(decoded.transform_offset, 48);
        assert_eq!(decoded.transform.rows()[0], [0., -1., 0., 7.]);
        assert_eq!(
            decoded.transform_reference_offset,
            Some(u64::try_from(start + 21).unwrap())
        );
    }
}

#[test]
fn a_broken_marked_prologue_leaves_an_unlisted_transform_unselected() {
    let (bytes, scope) = marked_move_fixture("332", 256);
    let start = usize::try_from(scope.byte_offset()).unwrap();
    for at in [15, 21, 22, 32, 36, 37] {
        let mut changed = bytes.clone();
        changed[start + at] ^= 0x02;
        assert!(
            decoded_move(&changed, &scope).is_none(),
            "offset {at} accepted"
        );
    }
    let mut unmarked = scope.clone();
    lay_out_move_face(&mut unmarked, start, 126, vec![50, 60, 72]);
    assert!(decoded_move(&bytes, &unmarked).is_none());

    let (bytes, scope) = marked_move_fixture("452", 253);
    let mut changed = bytes.clone();
    changed[usize::try_from(scope.byte_offset()).unwrap() + 32] = 2;
    assert_eq!(
        decoded_move(&changed, &scope)
            .unwrap()
            .transform_reference_offset,
        None
    );
}

#[test]
fn move_transform_selects_type_and_length_form() {
    let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 2);
    let types = std::collections::HashMap::from([(500, &row)]);
    let (bytes, scope) = fixture("500", 253);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let decoded = exact_move_operation(&bytes, &records, &scope, &types).unwrap();
    assert_eq!(
        decoded.transform.rows(),
        [
            [0., -1., 0., 7.],
            [1., 0., 0., 11.],
            [0., 0., 1., 13.],
            [0., 0., 0., 1.]
        ]
    );
    assert_eq!(u32::from(decoded.form), 5);
    assert_eq!(decoded.transform_record_index, 71);
    let mut different_pair = bytes.clone();
    different_pair[257..260].copy_from_slice(b"258");
    let paired_records =
        crate::design::test_support::indexed_record_offsets_for_test(&different_pair);
    assert_eq!(
        exact_move_operation(&different_pair, &paired_records, &scope, &types)
            .unwrap()
            .transform_record_index,
        71
    );
    let wrong = super::move_transform_type("11111111-2222-3333-4444-555555555555", 2);
    assert!(exact_move_operation(
        &bytes,
        &records,
        &scope,
        &std::collections::HashMap::from([(500, &wrong)])
    )
    .is_none());
    let wrong_version = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 1);
    assert!(exact_move_operation(
        &bytes,
        &records,
        &scope,
        &std::collections::HashMap::from([(500, &wrong_version)])
    )
    .is_none());
    let (mut bytes, scope) = fixture("500", 273);
    bytes[43..47].copy_from_slice(&5u32.to_le_bytes());
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert!(exact_move_operation(&bytes, &records, &scope, &types).is_none());
}

fn marked_move_loss(bytes: &[u8], scope: &DesignParameterScope) -> Option<String> {
    marked_move_loss_of(bytes, scope, 2)
}

/// The DR-306 loss message of a marked Move whose rigid-transform type has
/// `version`.
fn marked_move_loss_of(bytes: &[u8], scope: &DesignParameterScope, version: u32) -> Option<String> {
    let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", version);
    let class_types = super::move_transform_types(&row, &[452, 500]);
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    let operation = exact_move_operation(bytes, &records, scope, &class_types).unwrap();
    marked_move_transform_loss(bytes, &records, scope, &operation, &class_types).map(|note| {
        assert_eq!(
            note.code,
            crate::loss::F3dLossCode::FeatureMoveTransformAssumed.kind()
        );
        note.message
    })
}

#[test]
fn a_marked_transform_record_that_breaks_a_rigid_transform_rule_keeps_its_matrix_with_a_loss() {
    let (mut bytes, scope) = marked_move_fixture("452", 253);
    bytes[43..47].copy_from_slice(&1u32.to_le_bytes());
    let decoded = decoded_move(&bytes, &scope).unwrap();
    assert_eq!(u32::from(decoded.form), 1);
    assert_eq!(decoded.transform.rows()[0], [0., -1., 0., 7.]);
    assert_eq!(
        marked_move_loss(&bytes, &scope).as_deref(),
        Some(
            "Move scope f3d:test:scope#17 keeps the rigid matrix of its marked transform record 71, which has form word 1 in a 253-byte frame (DR-306)."
        )
    );

    let (bytes, scope) = marked_move_fixture("332", 256);
    let decoded = decoded_move(&bytes, &scope).unwrap();
    assert_eq!(u32::from(decoded.form), 1);
    assert_eq!(decoded.transform.rows()[2], [0., 0., 1., 13.]);
    assert_eq!(
        marked_move_loss(&bytes, &scope).as_deref(),
        Some("Move scope f3d:test:scope#17 keeps the rigid matrix of its marked transform record 71, which has a class type other than 84E6245F-1CDC-4FB8-8048-163A85FAD3F0 version 2 or 3 and a 256-byte frame (DR-306).")
    );
}

#[test]
fn a_marked_transform_record_within_the_rigid_transform_rules_has_no_loss() {
    for (class, length) in [
        ("452", 253),
        ("500", 254),
        ("500", 273),
        ("452", 274),
        ("452", 283),
        ("500", 284),
    ] {
        let (bytes, scope) = marked_move_fixture(class, length);
        assert_eq!(marked_move_loss(&bytes, &scope), None, "{class} {length}");
    }
    let (bytes, scope) = fixture("452", 253);
    assert_eq!(
        decoded_move(&bytes, &scope)
            .unwrap()
            .transform_reference_offset,
        None
    );
    assert_eq!(marked_move_loss(&bytes, &scope), None);
}

fn modify_fixture(marker_one: bool) -> (Vec<u8>, DesignParameterScope, usize) {
    let (mut bytes, _) = fixture("433", 284);
    let start = bytes.len();
    bytes.resize(start + 240, 0);
    let (transform_at, count_at) = if marker_one {
        bytes[start + 20] = 1;
        (25, 36)
    } else {
        (21, 32)
    };
    bytes[start + transform_at] = 1;
    bytes[start + transform_at + 1..start + transform_at + 5].copy_from_slice(&71u32.to_le_bytes());
    bytes[start + count_at..start + count_at + 4].copy_from_slice(&2u32.to_le_bytes());
    for (ordinal, group) in [54u32, 56].into_iter().enumerate() {
        let at = start + count_at + 4 + 11 * ordinal;
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&group.to_le_bytes());
    }
    let mut scope = DesignParameterScope::empty("f3d:test:scope#17", DesignFeatureKind::Modify, 17);
    lay_out_move_face(
        &mut scope,
        start,
        160,
        vec![71, 50, 51, 52, 53, 54, 55, 56, 57, 90],
    );
    (bytes, scope, start)
}

fn decoded_modify(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<crate::records::feature::direct_face::DesignModifyOperation> {
    exact_modify_operation(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
    )
}

#[test]
fn modify_decodes_both_prologues_with_a_form_zero_transform() {
    for (marker_one, transform_at, count_at) in [(false, 21, 32), (true, 25, 36)] {
        let (bytes, scope, start) = modify_fixture(marker_one);
        let decoded = decoded_modify(&bytes, &scope).unwrap();
        assert_eq!(decoded.body_group_record_indices, vec![54, 56]);
        assert_eq!(
            decoded.body_count_offset,
            u64::try_from(start + count_at).unwrap()
        );
        assert_eq!(decoded.motion.transform_record_index, 71);
        assert_eq!(u32::from(decoded.motion.form), 0);
        assert_eq!(
            decoded.motion.transform_reference_offset,
            Some(u64::try_from(start + transform_at).unwrap())
        );
        assert_eq!(decoded.motion.transform.rows()[1], [1., 0., 0., 11.]);
    }
}

#[test]
fn modify_refuses_broken_prologues_and_tables() {
    for marker_one in [false, true] {
        let (bytes, scope, start) = modify_fixture(marker_one);
        let count_at = if marker_one { 36 } else { 32 };
        let mut changes = vec![15, 20, count_at, count_at + 4, count_at + 15];
        if marker_one {
            changes.push(21);
        }
        for at in changes {
            let mut changed = bytes.clone();
            changed[start + at] ^= 0x04;
            assert!(
                decoded_modify(&changed, &scope).is_none(),
                "offset {at} accepted"
            );
        }
        let mut zero = bytes.clone();
        zero[start + count_at..start + count_at + 4].copy_from_slice(&0u32.to_le_bytes());
        assert!(decoded_modify(&zero, &scope).is_none());
        for table in [
            vec![50, 71, 51, 52, 53, 54, 55, 56, 57],
            vec![71, 54, 55, 56, 57, 50, 51, 52, 53],
            vec![71, 50, 51, 54, 55, 56, 57],
        ] {
            let mut shifted = scope.clone();
            lay_out_move_face(&mut shifted, start, 160, table);
            assert!(decoded_modify(&bytes, &shifted).is_none());
        }
        let mut move_scope =
            DesignParameterScope::empty("f3d:test:scope#17", DesignFeatureKind::Move, 17);
        lay_out_move_face(
            &mut move_scope,
            start,
            160,
            vec![71, 50, 51, 52, 53, 54, 55, 56, 57],
        );
        assert!(decoded_modify(&bytes, &move_scope).is_none());
    }
    let (mut bytes, scope, _) = modify_fixture(false);
    bytes[43..47].copy_from_slice(&2u32.to_le_bytes());
    assert!(decoded_modify(&bytes, &scope).is_none());
}

#[test]
fn a_284_byte_transform_record_stores_form_zero() {
    let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 2);
    let class_types = super::move_transform_types(&row, &[433]);
    let (bytes, scope, _) = modify_fixture(false);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let operation = decoded_modify(&bytes, &scope).unwrap();
    assert!(
        marked_move_transform_loss(&bytes, &records, &scope, &operation.motion, &class_types)
            .is_none()
    );
    let (mut bytes, scope, _) = modify_fixture(true);
    bytes[43..47].copy_from_slice(&1u32.to_le_bytes());
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let operation = decoded_modify(&bytes, &scope).unwrap();
    assert_eq!(
        marked_move_transform_loss(&bytes, &records, &scope, &operation.motion, &class_types)
            .map(|note| note.message)
            .as_deref(),
        Some(
            "Modify scope f3d:test:scope#17 keeps the rigid matrix of its marked transform record 71, which has form word 1 in a 284-byte frame (DR-306)."
        )
    );
}

fn form_two_fixture() -> (Vec<u8>, DesignParameterScope) {
    form_two_fixture_of(293)
}

fn form_two_fixture_of(length: usize) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"424");
    bytes[7..11].copy_from_slice(&71u32.to_le_bytes());
    for (at, target) in [(31, 96u32), (42, 93), (53, 115), (68, 118)] {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
    }
    bytes[83..87].copy_from_slice(&2u32.to_le_bytes());
    let transform: [[f64; 4]; 4] = [
        [1., 0., 0., 0.4],
        [0., 1., 0., -1.4],
        [0., 0., 1., 10.7],
        [0., 0., 0., 1.],
    ];
    for (cell, value) in transform.into_iter().flatten().enumerate() {
        let at = 88 + cell * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&71u32.to_le_bytes());
    let start = bytes.len();
    bytes.resize(start + 200, 0);
    bytes[start + 21] = 1;
    bytes[start + 22..start + 26].copy_from_slice(&71u32.to_le_bytes());
    bytes[start + 32..start + 36].copy_from_slice(&1u32.to_le_bytes());
    bytes[start + 36] = 1;
    bytes[start + 37..start + 41].copy_from_slice(&50u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty("f3d:test:scope#17", DesignFeatureKind::Move, 17);
    lay_out_move_face(&mut scope, start, 126, vec![50, 60, 71]);
    (bytes, scope)
}

#[test]
fn a_form_two_transform_record_stores_its_matrix_after_four_references() {
    let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 2);
    let class_types = super::move_transform_types(&row, &[424]);
    let (bytes, scope) = form_two_fixture();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let decoded = exact_move_operation(&bytes, &records, &scope, &class_types).unwrap();
    assert_eq!(u32::from(decoded.form), 2);
    assert_eq!(decoded.form_offset, 83);
    assert_eq!(decoded.transform_offset, 88);
    assert_eq!(decoded.transform.rows()[2], [0., 0., 1., 10.7]);
    assert!(marked_move_transform_loss(&bytes, &records, &scope, &decoded, &class_types).is_none());
    let note = move_transform_references_loss(&scope, &decoded).unwrap();
    assert_eq!(
        note.code,
        crate::loss::F3dLossCode::FeatureMoveTransformReferencesAssumed.kind()
    );
    assert_eq!(
        note.message,
        "Move scope f3d:test:scope#17 takes the rigid matrix of its form-2 transform record 71; the four references of the record are assumed to leave that motion unchanged (DR-375)."
    );
    let (bytes, scope) = fixture("424", 273);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let standard = exact_move_operation(&bytes, &records, &scope, &class_types).unwrap();
    assert!(move_transform_references_loss(&scope, &standard).is_none());
}

#[test]
fn a_form_two_transform_record_with_a_broken_field_is_refused() {
    let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 2);
    let class_types = super::move_transform_types(&row, &[424]);
    for at in [20, 31, 42, 53, 68, 83, 87] {
        let (mut bytes, scope) = form_two_fixture();
        bytes[at] = if at == 83 { 3 } else { bytes[at] ^ 0x04 };
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        assert!(
            exact_move_operation(&bytes, &records, &scope, &class_types).is_none(),
            "offset {at} accepted"
        );
    }
    let (mut bytes, scope) = fixture("424", 273);
    bytes[43..47].copy_from_slice(&2u32.to_le_bytes());
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert!(exact_move_operation(&bytes, &records, &scope, &class_types).is_none());
}

#[test]
fn a_version_three_transform_record_is_three_bytes_longer_than_version_two() {
    for (length, form) in [(256, 5u32), (276, 1), (286, 0)] {
        let (mut bytes, scope) = marked_move_fixture("452", length);
        bytes[43..47].copy_from_slice(&form.to_le_bytes());
        assert_eq!(marked_move_loss_of(&bytes, &scope, 3), None, "{length}");
        assert_eq!(
            marked_move_loss_of(&bytes, &scope, 2).as_deref(),
            Some(format!("Move scope f3d:test:scope#17 keeps the rigid matrix of its marked transform record 71, which has a {length}-byte frame (DR-306).").as_str())
        );
    }
    let (bytes, scope) = marked_move_fixture("452", 253);
    assert_eq!(
        marked_move_loss_of(&bytes, &scope, 3).as_deref(),
        Some("Move scope f3d:test:scope#17 keeps the rigid matrix of its marked transform record 71, which has a 253-byte frame (DR-306).")
    );
    let (mut bytes, scope) = marked_move_fixture("452", 256);
    bytes[43..47].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        marked_move_loss_of(&bytes, &scope, 3).as_deref(),
        Some("Move scope f3d:test:scope#17 keeps the rigid matrix of its marked transform record 71, which has form word 1 in a 256-byte frame (DR-306).")
    );

    for (version, loss) in [(3, false), (2, true)] {
        let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", version);
        let class_types = super::move_transform_types(&row, &[424]);
        let (bytes, scope) = form_two_fixture_of(296);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let decoded = exact_move_operation(&bytes, &records, &scope, &class_types).unwrap();
        assert_eq!(u32::from(decoded.form), 2);
        assert_eq!(
            marked_move_transform_loss(&bytes, &records, &scope, &decoded, &class_types).is_some(),
            loss
        );
    }
}

#[test]
fn a_version_three_record_at_a_version_two_length_reports_its_frame() {
    for length in [273, 283] {
        let (bytes, scope) = marked_move_fixture("452", length);
        assert_eq!(
            marked_move_loss_of(&bytes, &scope, 3).as_deref(),
            Some(format!("Move scope f3d:test:scope#17 keeps the rigid matrix of its marked transform record 71, which has a {length}-byte frame (DR-306).").as_str())
        );
    }
    let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", 3);
    let class_types = super::move_transform_types(&row, &[424]);
    let (bytes, scope) = form_two_fixture_of(293);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let decoded = exact_move_operation(&bytes, &records, &scope, &class_types).unwrap();
    assert!(marked_move_transform_loss(&bytes, &records, &scope, &decoded, &class_types).is_some());
}

#[test]
fn a_move_without_a_prologue_selects_only_a_version_two_record() {
    for (version, length, form, selected) in
        [(2, 253, 5u32, true), (3, 256, 5, false), (3, 276, 1, false)]
    {
        let row = super::move_transform_type("84E6245F-1CDC-4FB8-8048-163A85FAD3F0", version);
        let class_types = super::move_transform_types(&row, &[452]);
        let (mut bytes, scope) = fixture("452", length);
        bytes[43..47].copy_from_slice(&form.to_le_bytes());
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        assert_eq!(
            exact_move_operation(&bytes, &records, &scope, &class_types).is_some(),
            selected,
            "{version} {length}"
        );
    }
}
