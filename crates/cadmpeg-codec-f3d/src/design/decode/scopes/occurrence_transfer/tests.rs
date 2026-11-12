// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::records::feature::assembly_features::OCCURRENCE_PLACEMENT_TYPE_GUID;
use crate::records::feature::occurrence_transfer::DesignOccurrenceTransferMode;
use crate::test_support::lp_utf16;

fn header(out: &mut Vec<u8>, index: u32) {
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"333");
    out.extend_from_slice(&index.to_le_bytes());
}
fn reference(out: &mut Vec<u8>, index: u32) {
    out.push(1);
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}
fn matrix(out: &mut Vec<u8>, translation: f64) {
    let mut rows: [[f64; 4]; 4] = SketchPlacementMatrix::IDENTITY.into();
    rows[0][3] = translation;
    for value in rows.into_iter().flatten() {
        out.extend_from_slice(&value.to_le_bytes());
    }
}
fn relation(out: &mut Vec<u8>, index: u32, scope: u32, owner: u32, marked_gap: bool) {
    header(out, index);
    out.extend_from_slice(&[0; 10]);
    reference(out, 10);
    if marked_gap {
        out.push(1);
    }
    out.extend_from_slice(&[0; 2]);
    reference(out, 99);
    out.push(0);
    reference(out, scope);
    header(out, index);
    out.extend_from_slice(&[0; 8]);
    reference(out, owner);
}
fn types() -> std::collections::HashMap<u64, (&'static str, u32)> {
    [
        (10, (OCCURRENCE_PLACEMENT_TYPE_GUID, 0)),
        (11, (COMPONENT_TYPE, 0)),
        (2, (COMPONENT_TYPE, 0)),
        (3, (COMPONENT_TYPE, 0)),
        (5, (CONTEXT_TYPE, 0)),
        (6, (CONTEXT_TYPE, 0)),
        (7, (OWNER_TYPE, 0)),
        (8, (OWNER_TYPE, 0)),
    ]
    .into_iter()
    .collect()
}
fn context(out: &mut Vec<u8>, component: u32, context: u32, collection: u32) {
    header(out, component);
    header(out, context);
    reference(out, collection);
    reference(out, 90);
    header(out, context);
    out.extend_from_slice(&[0; 8]);
    reference(out, component);
    header(out, collection);
}
/// Type GUID that `CopyPaste` and `CutPaste` scopes share.
const PASTE_TYPE: &str = "D57B1A22-9D5A-4D7D-A909-E08C579612CA";

fn fixture(cut: bool, identity: bool, shift: usize) -> (Vec<u8>, DesignParameterScope) {
    fixture_with(cut, identity, shift, false)
}
/// A paste scope with a localized kind, an 88-byte tail, and for a copy the
/// 58-byte relation, typed through the shared paste GUID.
fn localized_fixture(cut: bool, shift: usize) -> (Vec<u8>, DesignParameterScope) {
    fixture_with(cut, false, shift, true)
}
fn copy_placement_fixture(
    source_identity: bool,
    target_identity: bool,
    localized: bool,
) -> (Vec<u8>, DesignParameterScope) {
    let (mut bytes, scope) = fixture_with(false, false, 0, localized);
    let start = usize::try_from(scope.byte_offset()).unwrap();
    if source_identity {
        bytes[start + 33] = 1;
        bytes.drain(start + 34..start + 162);
    }
    let target_flag = start + if source_identity { 61 } else { 189 };
    if target_identity {
        bytes[target_flag] = 1;
        bytes.drain(target_flag + 1..target_flag + 129);
    }
    let scope = parse(&bytes, u64::try_from(start).unwrap(), localized).unwrap();
    (bytes, scope)
}
fn cut_88_byte_tail_fixture() -> (Vec<u8>, DesignParameterScope) {
    let (bytes, scope) = fixture(true, false, 4);
    with_extended_scope_tail(bytes, &scope)
}
fn with_extended_scope_tail(
    mut bytes: Vec<u8>,
    scope: &DesignParameterScope,
) -> (Vec<u8>, DesignParameterScope) {
    let start = usize::try_from(scope.byte_offset()).unwrap();
    let kind_at = usize::try_from(scope.reference_count_offset()).unwrap() + 4 + 11 + 4;
    let units = usize::try_from(View::u32_le_at(&bytes, kind_at).unwrap()).unwrap();
    let kind_end = kind_at + 4 + units * 2;
    bytes.insert(kind_end, 0);
    let scope = parse(&bytes, u64::try_from(start).unwrap(), false).unwrap();
    (bytes, scope)
}
fn parse(bytes: &[u8], start: u64, localized: bool) -> Option<DesignParameterScope> {
    let records = IndexedRecordOffsets::build(bytes);
    crate::design::decode::scopes::parameter_scope::parse_parameter_scope_with_type_guid(
        bytes,
        &records,
        21,
        &"333".to_owned().try_into().unwrap(),
        start,
        localized.then_some(PASTE_TYPE),
    )
    .unwrap()
}
fn fixture_with(
    cut: bool,
    identity: bool,
    shift: usize,
    localized: bool,
) -> (Vec<u8>, DesignParameterScope) {
    let key = "11111111-2222-3333-4444-555555555555";
    let mut bytes = Vec::new();
    context(&mut bytes, 2, 5, 7);
    context(&mut bytes, 3, 6, 8);
    header(&mut bytes, 10);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 11);
    header(&mut bytes, 11);
    relation(&mut bytes, 12, 20, 7, false);
    relation(&mut bytes, 14, 21, 8, localized && !cut);
    header(&mut bytes, 13);
    if cut {
        bytes.extend_from_slice(&[0; 22]);
        reference(&mut bytes, 12);
        lp_utf16(&mut bytes, key);
        bytes.extend_from_slice(&[0; 7]);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        lp_utf16(&mut bytes, "RemoveInstance");
        header(&mut bytes, 13);
        bytes.extend_from_slice(&[0; 8]);
        reference(&mut bytes, 7);
    }
    let start = bytes.len();
    header(&mut bytes, 21);
    if cut {
        if shift == 4 {
            bytes.extend_from_slice(&[0; 9]);
            bytes.push(1);
            bytes.extend_from_slice(&[0; 4]);
        } else {
            bytes.extend_from_slice(&[0; 10]);
        }
        reference(&mut bytes, 13);
        reference(&mut bytes, 22);
        bytes.push(1);
        reference(&mut bytes, 23);
        bytes.extend_from_slice(&[0; 4]);
        reference(&mut bytes, 14);
        bytes.push(1);
        bytes.push(u8::from(identity));
        if !identity {
            matrix(&mut bytes, 3.0);
        }
        lp_utf16(&mut bytes, key);
    } else {
        bytes.resize(bytes.len() + 11 + shift, 0);
        reference(&mut bytes, 22);
        bytes.push(0);
        matrix(&mut bytes, 2.0);
        reference(&mut bytes, 23);
        bytes.extend_from_slice(&[0; 4]);
        reference(&mut bytes, 14);
        bytes.extend_from_slice(&[1, 0]);
        matrix(&mut bytes, 3.0);
        lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    }
    bytes.extend_from_slice(&[0; 3]);
    let count_at = bytes.len();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 14);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    let kind = match (cut, localized) {
        (true, false) => "CutPaste",
        (false, false) => "CopyPaste",
        (true, true) => "CouperColler",
        (false, true) => "CopierColler",
    };
    lp_utf16(&mut bytes, kind);
    let ordinal_at = bytes.len();
    bytes.resize(ordinal_at + if localized { 88 } else { 87 }, 0);
    bytes[ordinal_at..ordinal_at + 4].copy_from_slice(&1u32.to_le_bytes());
    let paired = bytes.len();
    header(&mut bytes, 21);
    let scope = parse(&bytes, u64::try_from(start).unwrap(), localized).unwrap();
    assert_eq!(scope.paired_byte_offset(), u64::try_from(paired).unwrap());
    assert_eq!(
        scope.reference_count_offset(),
        u64::try_from(count_at).unwrap()
    );
    (bytes, scope)
}

#[test]
fn occurrence_transfer_decodes_cut_owners_and_identity_or_matrix_placement() {
    for (identity, shift) in [(true, 0), (false, 0), (false, 4)] {
        let (bytes, scope) = fixture(true, identity, shift);
        let records = IndexedRecordOffsets::build(&bytes);
        let value = exact_occurrence_transfer(&bytes, &records, &scope, &types()).unwrap();
        assert_eq!(value.target_owner.component, 3);
        assert_eq!(value.target_owner.context, 6);
        assert_eq!(value.target_owner.collection, 8);
        assert_eq!(value.component_record_index, 11);
        assert_eq!(value.target_transform_offset.is_none(), identity);
        let DesignOccurrenceTransferSource::Cut {
            source_owner,
            removal_record_index,
            ..
        } = value.source
        else {
            panic!("cut source");
        };
        assert_eq!(source_owner.component, 2);
        assert_eq!(source_owner.context, 5);
        assert_eq!(source_owner.collection, 7);
        assert_eq!(removal_record_index, 13);
    }
}

#[test]
fn occurrence_transfer_decodes_copy_placements_without_guessing_source_occurrence() {
    for shift in [0, 4] {
        let (bytes, scope) = fixture(false, false, shift);
        let records = IndexedRecordOffsets::build(&bytes);
        let value = exact_occurrence_transfer(&bytes, &records, &scope, &types()).unwrap();
        let DesignOccurrenceTransferSource::Copy {
            source_transform, ..
        } = value.source
        else {
            panic!("copy source");
        };
        assert_eq!(source_transform[0][3], 2.0);
        assert_eq!(value.target_transform[0][3], 3.0);
    }
}

#[test]
fn occurrence_transfer_intent_keeps_copy_placement_when_owner_join_fails() {
    let (bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let mut incomplete_types = types();
    incomplete_types.remove(&6);
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &incomplete_types).is_none());
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &incomplete_types).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Copy);
    assert_eq!(intent.relation_record_index, Some(14));
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(intent.component_record_index, Some(11));
    assert_eq!(intent.target_owner_component_record_index, None);
    assert_eq!(intent.source_transform.unwrap()[0][3], 2.0);
    assert_eq!(intent.target_transform.unwrap()[0][3], 3.0);
}

#[test]
fn occurrence_transfer_intent_keeps_cut_mode_and_target_after_source_owner_loss() {
    let (bytes, scope) = fixture(true, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let mut incomplete_types = types();
    incomplete_types.remove(&5);
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &incomplete_types).is_none());
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &incomplete_types).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Reparent);
    assert_eq!(intent.relation_record_index, Some(14));
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(intent.source_owner_component_record_index, None);
    assert_eq!(intent.target_owner_component_record_index, Some(3));
    assert_eq!(intent.target_transform.unwrap()[0][3], 3.0);
}

#[test]
fn identity_and_matrix_copy_prefixes_keep_both_placements() {
    for (source_identity, target_identity, localized, frame_length) in [
        (true, true, false, 269),
        (true, false, true, 404),
        (false, false, true, 532),
    ] {
        let (bytes, scope) = copy_placement_fixture(source_identity, target_identity, localized);
        assert_eq!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::CopyPaste
        );
        assert_eq!(scope.frame_length(), frame_length);
        let records = IndexedRecordOffsets::build(&bytes);
        let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
        assert_eq!(intent.mode, DesignOccurrenceTransferMode::Copy);
        assert_eq!(intent.relation_record_index, Some(14));
        assert_eq!(intent.occurrence_record_index, Some(10));
        assert_eq!(intent.component_record_index, Some(11));
        assert_eq!(
            intent.source_transform.unwrap()[0][3],
            if source_identity { 0.0 } else { 2.0 }
        );
        assert_eq!(
            intent.target_transform.unwrap()[0][3],
            if target_identity { 0.0 } else { 3.0 }
        );
    }
}

#[test]
fn localized_cut_prefix_selects_cutpaste_from_the_shared_type() {
    let (bytes, scope) = localized_fixture(true, 4);
    assert_eq!(scope.kind_name(), "CutPaste");
    assert_eq!(scope.source_kind_name(), "CouperColler");
    assert_eq!(scope.frame_length(), 418);
    let records = IndexedRecordOffsets::build(&bytes);
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Reparent);
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(intent.source_owner_component_record_index, Some(2));
    assert_eq!(intent.target_owner_component_record_index, Some(3));
    assert_eq!(intent.target_transform.unwrap()[0][3], 3.0);
}

#[test]
fn cutpaste_with_one_more_scope_tail_byte_keeps_the_shifted_prefix() {
    let (bytes, scope) = cut_88_byte_tail_fixture();
    assert_eq!(scope.frame_length(), 410);
    let records = IndexedRecordOffsets::build(&bytes);
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_none());
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Reparent);
    assert_eq!(intent.relation_record_index, Some(14));
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(intent.source_owner_component_record_index, Some(2));
    assert_eq!(intent.target_owner_component_record_index, Some(3));
    assert_eq!(intent.target_transform.unwrap()[0][3], 3.0);
}

#[test]
fn english_copy_prefixes_keep_identity_source_and_extended_tail_lengths() {
    for (source_identity, frame_length) in [(true, 398), (false, 526)] {
        let (bytes, scope) = copy_placement_fixture(source_identity, false, false);
        let (bytes, scope) = with_extended_scope_tail(bytes, &scope);
        assert_eq!(scope.frame_length(), frame_length);
        let records = IndexedRecordOffsets::build(&bytes);
        let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
        assert_eq!(intent.mode, DesignOccurrenceTransferMode::Copy);
        assert_eq!(intent.occurrence_record_index, Some(10));
        assert_eq!(intent.target_transform.unwrap()[0][3], 3.0);
        assert_eq!(
            intent.source_transform.unwrap()[0][3],
            if source_identity { 0.0 } else { 2.0 }
        );
    }
}

#[test]
fn invalid_copy_matrices_do_not_drop_the_mode_or_other_placement() {
    for (offset, source_unresolved) in [(34, true), (190, false)] {
        let (mut bytes, scope) = fixture(false, false, 0);
        let start = usize::try_from(scope.byte_offset()).unwrap();
        bytes[start + offset..start + offset + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        let records = IndexedRecordOffsets::build(&bytes);
        let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
        assert_eq!(intent.mode, DesignOccurrenceTransferMode::Copy);
        assert_eq!(intent.occurrence_record_index, Some(10));
        assert_eq!(intent.source_transform.is_none(), source_unresolved);
        assert_eq!(intent.target_transform.is_none(), !source_unresolved);
    }
}

#[test]
fn a_bounded_invalid_copy_reference_keeps_its_placement_fields() {
    let (mut bytes, scope) = fixture(false, false, 0);
    let start = usize::try_from(scope.byte_offset()).unwrap();
    bytes[start + 22] = 0;
    let records = IndexedRecordOffsets::build(&bytes);
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Copy);
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(intent.source_transform.unwrap()[0][3], 2.0);
    assert_eq!(intent.target_transform.unwrap()[0][3], 3.0);
}

#[test]
fn a_matrix_selector_cannot_read_from_the_scope_table_or_footer() {
    let (mut bytes, scope) = copy_placement_fixture(true, true, false);
    let start = usize::try_from(scope.byte_offset()).unwrap();
    bytes[start + 61] = 0;
    let records = IndexedRecordOffsets::build(&bytes);
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Copy);
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(
        intent.source_transform.unwrap(),
        SketchPlacementMatrix::IDENTITY
    );
    assert_eq!(intent.target_transform, None);

    let (mut bytes, scope) = copy_placement_fixture(true, true, false);
    let start = usize::try_from(scope.byte_offset()).unwrap();
    bytes.truncate(start + 61);
    let records = IndexedRecordOffsets::build(&bytes);
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Copy);
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(
        intent.source_transform.unwrap(),
        SketchPlacementMatrix::IDENTITY
    );
    assert_eq!(intent.target_transform, None);
}

#[test]
fn invalid_cut_matrix_keeps_mode_and_the_typed_owner_joins() {
    let (mut bytes, scope) = fixture(true, false, 4);
    let start = usize::try_from(scope.byte_offset()).unwrap();
    let matrix_at = start + 76;
    bytes[matrix_at..matrix_at + 8].copy_from_slice(&f64::NAN.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Reparent);
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(intent.source_owner_component_record_index, Some(2));
    assert_eq!(intent.target_owner_component_record_index, Some(3));
    assert_eq!(intent.target_transform, None);
}

#[test]
fn a_bounded_invalid_cut_removal_reference_keeps_its_target_fields() {
    let (mut bytes, scope) = fixture(true, false, 4);
    let start = usize::try_from(scope.byte_offset()).unwrap();
    bytes[start + 25] = 0;
    let records = IndexedRecordOffsets::build(&bytes);
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Reparent);
    assert_eq!(intent.relation_record_index, Some(14));
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(intent.source_owner_component_record_index, None);
    assert_eq!(intent.target_owner_component_record_index, Some(3));
    assert_eq!(intent.target_transform.unwrap()[0][3], 3.0);
}

#[test]
fn relation_pair_and_carrier_types_gate_only_their_own_links() {
    let (mut bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let relation_at = records.first_at_or_after(0, 14).unwrap();
    let paired_at = next_indexed_record_offset(&bytes, relation_at + 1).unwrap();
    bytes[paired_at + 7..paired_at + 11].copy_from_slice(&99u32.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Copy);
    assert_eq!(intent.occurrence_record_index, None);
    assert_eq!(intent.target_owner_component_record_index, None);

    let (bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let mut wrong_carrier_type = types();
    wrong_carrier_type.insert(10, (COMPONENT_TYPE, 0));
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &wrong_carrier_type).unwrap();
    assert_eq!(intent.occurrence_record_index, None);
    assert_eq!(intent.component_record_index, None);
    assert_eq!(intent.target_owner_component_record_index, Some(3));

    let mut wrong_component_type = types();
    wrong_component_type.insert(11, (OWNER_TYPE, 0));
    let intent =
        occurrence_transfer_intent(&bytes, &records, &scope, &wrong_component_type).unwrap();
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(intent.component_record_index, None);
}

#[test]
fn a_component_after_its_relation_is_not_joined_to_the_occurrence() {
    let (mut bytes, scope) = fixture(false, false, 0);
    let carrier_at = IndexedRecordOffsets::build(&bytes)
        .first_at_or_after(0, 10)
        .unwrap();
    bytes[carrier_at + 25..carrier_at + 29].copy_from_slice(&14u32.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let mut types = types();
    types.insert(14, (COMPONENT_TYPE, 0));
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types).unwrap();
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(intent.component_record_index, None);
}

#[test]
fn a_non_removeinstance_source_keeps_the_cut_target_and_occurrence() {
    let (mut bytes, scope) = fixture(true, false, 0);
    let removal_at = IndexedRecordOffsets::build(&bytes)
        .first_at_or_after(0, 13)
        .unwrap();
    bytes[removal_at + 135..removal_at + 137].copy_from_slice(&u16::from(b'X').to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Reparent);
    assert_eq!(intent.occurrence_record_index, Some(10));
    assert_eq!(intent.source_owner_component_record_index, None);
    assert_eq!(intent.target_owner_component_record_index, Some(3));
    assert_eq!(intent.target_transform.unwrap()[0][3], 3.0);
}

#[test]
fn occurrence_transfer_rejects_changed_relation_marker_and_source_owner() {
    let (mut bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    bytes[usize::try_from(scope.byte_offset()).unwrap() + 177] = 0;
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_none());

    let (mut bytes, scope) = fixture(true, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let relation_at = records.first_at_or_after(0, 12).unwrap();
    bytes[relation_at + 77..relation_at + 81].copy_from_slice(&9u32.to_le_bytes());
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_none());
}

#[test]
fn occurrence_transfer_rejects_non_rigid_and_truncated_placement() {
    let (mut bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let at = usize::try_from(scope.byte_offset()).unwrap() + 190;
    bytes[at..at + 8].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_none());
    assert!(exact_occurrence_transfer(&bytes[..at + 8], &records, &scope, &types()).is_none());
}

#[test]
fn occurrence_transfer_requires_typed_context_and_collection_ownership() {
    let (bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let mut incomplete = types();
    incomplete.remove(&6);
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &incomplete).is_none());
    let mut wrong_type = types();
    wrong_type.insert(8, (COMPONENT_TYPE, 0));
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &wrong_type).is_none());
}

#[test]
fn occurrence_transfer_rejects_other_types_with_an_occurrence_prefix() {
    let (bytes, scope) = fixture(false, false, 0);
    let records = IndexedRecordOffsets::build(&bytes);
    let mut wrong_type = types();
    wrong_type.insert(10, (COMPONENT_TYPE, 0));
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &wrong_type).is_none());
}

#[test]
fn a_localized_compact_copy_prefix_selects_copy_paste_and_decodes_its_transfer() {
    for shift in [0, 4] {
        let (bytes, scope) = localized_fixture(false, shift);
        assert_eq!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::CopyPaste
        );
        assert_eq!(scope.source_kind_name(), "CopierColler");
        let records = IndexedRecordOffsets::build(&bytes);
        let value = exact_occurrence_transfer(&bytes, &records, &scope, &types()).unwrap();
        assert_eq!(value.occurrence_record_index, 10);
        assert_eq!(value.target_owner.collection, 8);
        let DesignOccurrenceTransferSource::Copy {
            source_transform, ..
        } = value.source
        else {
            panic!("copy source");
        };
        assert_eq!(source_transform[0][3], 2.0);
        assert_eq!(value.target_transform[0][3], 3.0);
    }
}

#[test]
fn the_shared_paste_type_selects_complete_cut_and_keeps_changed_copy_native() {
    let native = |kind: &str| {
        crate::records::feature::scope::DesignFeatureKind::try_from(kind.to_owned()).unwrap()
    };
    let (bytes, scope) = localized_fixture(true, 0);
    assert_eq!(scope.kind(), native("CutPaste"));
    assert_eq!(scope.source_kind_name(), "CouperColler");
    let records = IndexedRecordOffsets::build(&bytes);
    assert!(exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_none());
    let intent = occurrence_transfer_intent(&bytes, &records, &scope, &types()).unwrap();
    assert_eq!(intent.mode, DesignOccurrenceTransferMode::Reparent);
    assert_eq!(intent.target_transform.unwrap()[0][3], 3.0);

    for marker in [20, 21] {
        let (mut bytes, scope) = localized_fixture(false, 0);
        bytes[usize::try_from(scope.byte_offset()).unwrap() + marker] = 1;
        let changed = parse(&bytes, scope.byte_offset(), true).unwrap();
        assert_eq!(changed.kind(), native("CopierColler"));
    }
}

#[test]
fn the_58_byte_relation_is_admitted_for_a_copy_and_refused_for_a_cut() {
    for cut in [false, true] {
        let (mut bytes, scope) = fixture(cut, false, 0);
        let relation_at = IndexedRecordOffsets::build(&bytes)
            .first_at_or_after(0, 14)
            .unwrap();
        bytes.insert(relation_at + 32, 1);
        let scope = parse(&bytes, scope.byte_offset() + 1, false).unwrap();
        let records = IndexedRecordOffsets::build(&bytes);
        assert_eq!(
            exact_occurrence_transfer(&bytes, &records, &scope, &types()).is_some(),
            !cut
        );
    }
}
