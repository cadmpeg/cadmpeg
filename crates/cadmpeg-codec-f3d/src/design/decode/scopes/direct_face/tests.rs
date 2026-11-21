// SPDX-License-Identifier: Apache-2.0

use super::{exact_axis_owner_scale, exact_scale_operation};
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::decode::scopes::parameter_scope::{
    parse_parameter_scope, parse_parameter_scope_with_type_guid,
};
use crate::design::test_support::{indexed_record_offsets_for_test, parameter_record};
use crate::records::feature::body_ops::{DesignScaleFactors, DesignScaleOperation};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
use crate::test_support::lp_utf16;
use cadmpeg_core::decode::u64_from_index;
use std::collections::HashMap;

const EPS_SCALE_VALUE: f64 = 1.0e-12;
const SCALE_TYPE_GUID: &str = "D45FD6C4-161B-4240-BFAC-83A7C2C97A0C";

#[test]
fn legacy_scale_with_short_or_extended_tail_resolves_center_and_history_states() {
    for (start, tail) in [(0, 77), (13, 78)] {
        let (bytes, position_at) = scale_fixture(5, tail, start);
        let scope = parse_scope(&bytes, start).expect("synthetic Scale scope");
        let offset = u64_from_index(start);
        assert_eq!(scope.frame_length(), 231 + u64_from_index(tail));
        assert_eq!(scope.reference_count_offset(), offset + 154);
        assert_eq!(scope.kind_offset(), offset + 221);
        assert_eq!(scope.feature_ordinal_offset(), offset + 231);
        assert_eq!(scope.previous_history_state_id_offset(), Some(offset + 262));
        assert_eq!(scope.history_state_id(), Some(19));
        assert_eq!(scope.previous_history_state_id(), Some(17));

        let operation =
            decode_scale(&bytes, start, &HashMap::new()).expect("uniform Scale with fixed tail");
        assert_eq!(operation.body_group_record_index, 102);
        assert_eq!(operation.center_record_index, 105);
        assert_eq!(operation.uniform().unwrap().1, offset + 21);
        assert!((operation.uniform().unwrap().0.get() - 2.5).abs() < EPS_SCALE_VALUE);
        let center = operation.center_position.expect("point-data center");
        assert_eq!(center.offset, u64_from_index(position_at));
        for (actual, expected) in center.value.into_iter().zip([1.25, -2.5, 3.75]) {
            assert!((actual.get() - expected).abs() < EPS_SCALE_VALUE);
        }
    }
}

#[test]
fn legacy_scale_with_extended_tail_refuses_invalid_factors() {
    let (bytes, _) = scale_fixture(5, 78, 0);
    assert!(decode_scale(&bytes, 0, &HashMap::new()).is_some());
    for factor in [
        0.0f64,
        -0.0,
        -1.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let mut invalid = bytes.clone();
        invalid[21..29].copy_from_slice(&factor.to_le_bytes());
        assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
    }
}

#[test]
fn legacy_scale_with_extended_tail_requires_complete_matching_references() {
    let (bytes, _) = scale_fixture(5, 78, 0);
    assert!(decode_scale(&bytes, 0, &HashMap::new()).is_some());
    for marker in [29, 40, 64] {
        for delta in 0..11 {
            let mut invalid = bytes.clone();
            invalid[marker + delta] ^= 1;
            assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
        }
    }
    for offset in (16..21).chain(51..64) {
        let mut invalid = bytes.clone();
        invalid[offset] ^= 1;
        assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
    }
}

#[test]
fn legacy_scale_with_extended_tail_requires_null_guid_and_zero_separator() {
    let (bytes, _) = scale_fixture(5, 78, 0);
    assert!(decode_scale(&bytes, 0, &HashMap::new()).is_some());
    for offset in 75..154 {
        let mut invalid = bytes.clone();
        invalid[offset] ^= 1;
        assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
    }
}

#[test]
fn legacy_scale_with_extended_tail_requires_finite_point_data_of_the_correct_type() {
    let (bytes, position_at) = scale_fixture(5, 78, 0);
    let point_type = HashMap::from([(105, ("69EE2FA7-BCC7-449E-9CA9-976CEFDFED44", 3))]);
    assert!(decode_scale(&bytes, 0, &point_type).is_some());
    for axis in 0..3 {
        for coordinate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut invalid = bytes.clone();
            let at = position_at + axis * 8;
            invalid[at..at + 8].copy_from_slice(&coordinate.to_le_bytes());
            assert!(decode_scale(&invalid, 0, &point_type).is_none());
        }
    }
    let other_type = HashMap::from([(105, ("00000000-0000-0000-0000-000000000001", 3))]);
    assert!(decode_scale(&bytes, 0, &other_type).is_none());
}

#[test]
fn legacy_scale_requires_an_admitted_frame_length_and_reference_count_pair() {
    for (count, tail_length) in [(5, 76), (5, 77), (5, 78), (6, 76)] {
        let (bytes, _) = scale_fixture(count, tail_length, 0);
        assert!(decode_scale(&bytes, 0, &HashMap::new()).is_some());
    }
    for (count, tail_length, frame_length) in
        [(5, 87, 318), (6, 72, 314), (6, 77, 319), (6, 78, 320)]
    {
        let (bytes, _) = scale_fixture(count, tail_length, 0);
        let scope = parse_scope(&bytes, 0).expect("synthetic scope envelope");
        assert_eq!(scope.frame_length(), frame_length);
        assert_eq!(scope.reference_members().len(), count);
        assert!(decode_scale(&bytes, 0, &HashMap::new()).is_none());
    }
}

#[test]
fn a_legacy_scale_form_is_selected_by_payload_length_under_any_kind_spelling() {
    for (count, tail_length) in [(5, 76), (5, 77), (5, 78), (6, 76)] {
        let (english, _) = named_scale_fixture("Scale", count, tail_length, 0);
        let (french, _) = named_scale_fixture("Echelle", count, tail_length, 0);
        let english = decode_typed_scale(&english).expect("English legacy Scale");
        let french = decode_typed_scale(&french).expect("French legacy Scale");
        assert_eq!(
            french.body_group_record_index,
            english.body_group_record_index
        );
        assert_eq!(french.center_record_index, english.center_record_index);
        assert_eq!(french.uniform().unwrap().1, english.uniform().unwrap().1);
        assert!((french.uniform().unwrap().0.get() - 2.5).abs() < EPS_SCALE_VALUE);
    }
    let (french, _) = named_scale_fixture("Echelle", 6, 78, 0);
    assert!(decode_typed_scale(&french).is_none());
}

#[test]
fn modern_scale_projects_the_short_fixed_tail() {
    let (mut bytes, _) = scale_fixture(5, 77, 0);
    let prefix = legacy_uniform_prefix();
    let modern = [&[1, 0, 0, 0][..], &prefix[20..]].concat();
    bytes.splice(20..prefix.len(), modern);
    let operation = decode_scale(&bytes, 0, &HashMap::new()).expect("short modern Scale");
    assert_eq!(operation.body_group_record_index, 102);
    assert_eq!(operation.center_record_index, 105);
    let (factor, offset) = operation.uniform().unwrap();
    assert_eq!(offset, 25);
    assert!((factor.get() - 2.5).abs() < EPS_SCALE_VALUE);
    bytes[crate::layout::scale_modern_operation_prefix::LEN] = 0;
    assert!(decode_scale(&bytes, 0, &HashMap::new()).is_none());
}

#[test]
fn named_legacy_scale_projects_one_body_group_member() {
    let prefix = legacy_uniform_prefix();
    let bytes = labeled_scale_fixture(prefix, &[101, 102, 103, 104, 105]);
    let scope = parse_scope(&bytes, 0).unwrap();
    assert_eq!(scope.label.as_ref().unwrap().value, "Example");
    assert_eq!(scope.previous_history_state_id(), Some(17));
    let operation = decode_scale(&bytes, 0, &HashMap::new()).expect("named uniform Scale");
    assert_eq!(operation.body_group_record_index, 102);
    assert_eq!(operation.center_record_index, 105);
    assert!((operation.uniform().unwrap().0.get() - 2.5).abs() < EPS_SCALE_VALUE);
}

#[test]
fn named_legacy_scale_matches_both_members_to_the_body_group() {
    for (members, matches) in [([103, 104], true), ([104, 103], false)] {
        let prefix = legacy_uniform_prefix();
        let mut bytes = labeled_scale_fixture(prefix, &[101, 102, 103, 104, 106, 105]);
        append_scale_fixture_header(&mut bytes, *b"351", 102);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        for member in members {
            append_marked_reference(&mut bytes, member);
        }
        append_scale_fixture_header(&mut bytes, *b"263", 102);
        let result = decode_scale(&bytes, 0, &HashMap::new());
        assert_eq!(
            result.as_ref().map(|op| op.body_group_record_index),
            matches.then_some(102)
        );
    }
}

#[test]
fn counted_group_scale_projects_the_ordered_groups() {
    let prefix = legacy_uniform_prefix();
    for groups in [&[102_u32][..], &[102, 103][..]] {
        let mut prefix = prefix[..51].to_vec();
        prefix.push(1);
        prefix.extend_from_slice(&u32::try_from(groups.len()).unwrap().to_le_bytes());
        for &group in groups {
            append_marked_reference(&mut prefix, group);
        }
        prefix.extend_from_slice(&1_u32.to_le_bytes());
        prefix.extend_from_slice(&0_u32.to_le_bytes());
        let refs = [&[101][..], groups, &[104, 105][..]].concat();
        let bytes = labeled_scale_fixture(prefix.clone(), &refs);
        let operation = decode_scale(&bytes, 0, &HashMap::new()).expect("counted-group Scale");
        assert_eq!(operation.body_group_record_index, groups[0]);
        assert_eq!(operation.further_body_group_record_indices, groups[1..]);
        assert_eq!(operation.center_record_index, 105);
        assert!((operation.uniform().unwrap().0.get() - 2.5).abs() < EPS_SCALE_VALUE);

        let mut wrong_order = refs;
        wrong_order[1] = 200;
        let bytes = labeled_scale_fixture(prefix, &wrong_order);
        assert!(decode_scale(&bytes, 0, &HashMap::new()).is_none());
    }
}

#[test]
fn axis_owner_scale_resolves_only_its_linked_positive_axis_factors() {
    const STREAM: &str = "f3d:synthetic/BulkStream.dat";
    let mut prefix = legacy_uniform_prefix()[..40].to_vec();
    prefix[21..29].fill(0);
    prefix.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0]);
    let axis_records = [201_u32, 205, 209];
    for record in axis_records {
        append_marked_reference(&mut prefix, record);
    }
    prefix.extend_from_slice(&1_u32.to_le_bytes());
    append_marked_reference(&mut prefix, 102);
    let bytes = labeled_scale_fixture(prefix, &[201, 205, 209, 102, 103, 104, 105]);
    let mut scope = parse_scope(&bytes, 0).unwrap();
    scope.id = format!("{STREAM}:design-parameter-scope#0");
    let records = indexed_record_offsets_for_test(&bytes);
    let values = [1.5_f64, 0.75, 2.0];
    let mut parameters = Vec::new();
    let mut owners = Vec::new();
    for (ordinal, ((record, kind), value)) in axis_records
        .into_iter()
        .zip(["ScaleX", "ScaleY", "ScaleZ"])
        .zip(values)
        .enumerate()
    {
        let parameter_index = record - 1;
        let mut bytes = parameter_record(Some(record), &value.to_string(), kind, None, kind, value);
        bytes[7..11].copy_from_slice(&parameter_index.to_le_bytes());
        let mut parameter = parse_design_parameter_record(&bytes).unwrap();
        parameter.id = format!("{STREAM}:design-parameter#{parameter_index}");
        parameters.push(parameter);
        let offset = u64::from(record) * 150;
        owners.push(
            DesignParameterOwner::try_from(DesignParameterOwnerWire {
                id: format!("{STREAM}:design-parameter-owner#{record}"),
                byte_offset: offset,
                frame_length: 104,
                class_tag: "296".to_owned().try_into().unwrap(),
                record_index: record,
                scope_record_index: scope.record_index,
                local_ordinal: u32::try_from(ordinal).unwrap(),
                evaluated_value: value,
                evaluated_value_offset: offset + 40,
                parameter_record_index: parameter_index,
                owned_ordinal: 1,
                variant: Some(0),
                companion_record_index: record + 1,
            })
            .unwrap(),
        );
    }
    let decode = |parameters: &[_], owners: &[_]| {
        exact_axis_owner_scale(
            &bytes,
            &records,
            &scope,
            &HashMap::new(),
            parameters,
            owners,
        )
    };
    let operation = decode(&parameters, &owners).expect("owner-backed Scale");
    let DesignScaleFactors::PerAxis { factors, offsets } = operation.factors else {
        panic!("per-axis factors");
    };
    for (actual, expected) in factors.into_iter().zip(values) {
        assert!((actual.get() - expected).abs() < EPS_SCALE_VALUE);
    }
    assert_eq!(
        offsets,
        std::array::from_fn(|i| owners[i].evaluated_value_offset())
    );
    assert!(decode(&parameters, &owners[..2]).is_none());
    owners.push(owners[1].clone());
    assert!(decode(&parameters, &owners).is_none());
    owners.pop();
    parameters[1].try_set_evaluated_value(2.0).unwrap();
    assert!(decode(&parameters, &owners).is_none());
}

#[test]
fn an_english_scale_frame_length_is_read_with_a_loss_under_another_spelling() {
    let assumed = crate::loss::F3dLossCode::FeatureScaleFrameLengthFormAssumed.kind();
    let (french, _) = named_scale_fixture("Echelle", 5, 72, 0);
    let scope = parse_scope(&french, 0).expect("synthetic scope envelope");
    assert_eq!(scope.frame_length(), 307);
    let (operation, losses) = decode_typed_scale_and_losses(&french);
    assert!(operation.is_some());
    assert_eq!(losses, [assumed]);
    let (unplaced_tail, _) = named_scale_fixture("Escalade", 5, 72, 0);
    let scope = parse_scope(&unplaced_tail, 0).expect("synthetic scope envelope");
    assert_eq!(scope.frame_length(), 309);
    assert_eq!(
        decode_typed_scale_and_losses(&unplaced_tail),
        (None, Vec::new())
    );
    for (kind, tail_length) in [("Scale", 76), ("Echelle", 76)] {
        let (bytes, _) = named_scale_fixture(kind, 5, tail_length, 0);
        let (operation, losses) = decode_typed_scale_and_losses(&bytes);
        assert!(operation.is_some(), "{kind}");
        assert!(losses.is_empty(), "{kind}");
    }
}

#[test]
fn legacy_scale_with_extended_tail_requires_a_complete_scope_envelope() {
    let (bytes, _) = scale_fixture(5, 78, 0);
    assert!(decode_scale(&bytes, 0, &HashMap::new()).is_some());
    for offset in [154, 155, 156, 157, 217, 218, 219, 220, 221, 316] {
        let mut invalid = bytes.clone();
        invalid[offset] ^= 1;
        assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
    }
    for ordinal in 0..5 {
        for delta in [0, 5, 6, 7, 8, 9, 10] {
            let mut invalid = bytes.clone();
            invalid[158 + ordinal * 11 + delta] ^= 1;
            assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
        }
    }
    let mut invalid_ordinal = bytes.clone();
    invalid_ordinal[231..235].fill(0);
    assert!(decode_scale(&invalid_ordinal, 0, &HashMap::new()).is_none());

    let mut invalid_previous_state = bytes.clone();
    invalid_previous_state[262..266].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_scale(&invalid_previous_state, 0, &HashMap::new()).is_none());

    for end in 0..bytes.len() {
        assert!(decode_scale(&bytes[..end], 0, &HashMap::new()).is_none());
    }
}

fn parse_scope(bytes: &[u8], start: usize) -> Option<DesignParameterScope> {
    let records = indexed_record_offsets_for_test(bytes);
    let class_tag = "350".to_owned().try_into().unwrap();
    parse_parameter_scope(bytes, &records, 100, &class_tag, u64_from_index(start)).unwrap()
}

fn decode_typed_scale(bytes: &[u8]) -> Option<DesignScaleOperation> {
    decode_typed_scale_and_losses(bytes).0
}

/// The typed Scale operation and the loss codes its reading adds.
fn decode_typed_scale_and_losses(
    bytes: &[u8],
) -> (
    Option<DesignScaleOperation>,
    Vec<cadmpeg_ir::report::loss::LossKind>,
) {
    let records = indexed_record_offsets_for_test(bytes);
    let class_tag = "350".to_owned().try_into().unwrap();
    let Some(scope) = parse_parameter_scope_with_type_guid(
        bytes,
        &records,
        100,
        &class_tag,
        0,
        Some(SCALE_TYPE_GUID),
    )
    .unwrap() else {
        return (None, Vec::new());
    };
    let mut losses = Vec::new();
    let operation = exact_scale_operation(bytes, &records, &scope, &HashMap::new(), &mut losses);
    (
        operation,
        losses.into_iter().map(|loss| loss.code).collect(),
    )
}

fn decode_scale(
    bytes: &[u8],
    start: usize,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignScaleOperation> {
    let scope = parse_scope(bytes, start)?;
    let records = indexed_record_offsets_for_test(bytes);
    exact_scale_operation(bytes, &records, &scope, stream_types, &mut Vec::new())
}

fn scale_fixture(reference_count: usize, tail_length: usize, start: usize) -> (Vec<u8>, usize) {
    named_scale_fixture("Scale", reference_count, tail_length, start)
}

fn legacy_uniform_prefix() -> Vec<u8> {
    let (mut bytes, _) = scale_fixture(5, 78, 0);
    bytes.truncate(crate::layout::scale_legacy_operation_prefix::LEN);
    bytes
}

fn append_marked_reference(bytes: &mut Vec<u8>, record: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&record.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}

fn append_scale_fixture_header(bytes: &mut Vec<u8>, class: [u8; 3], record: u32) {
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&class);
    bytes.extend_from_slice(&record.to_le_bytes());
}

fn labeled_scale_fixture(mut prefix: Vec<u8>, references: &[u32]) -> Vec<u8> {
    let (bytes, _) = scale_fixture(5, 78, 0);
    lp_utf16(&mut prefix, "00000000-0000-0000-0000-000000000000");
    prefix.extend_from_slice(&[0; 3]);
    prefix.extend_from_slice(&u32::try_from(references.len()).unwrap().to_le_bytes());
    for &record in references {
        append_marked_reference(&mut prefix, record);
    }
    prefix.extend_from_slice(&19_u32.to_le_bytes());
    lp_utf16(&mut prefix, "Scale");
    prefix.extend_from_slice(&3_u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 4]);
    lp_utf16(&mut prefix, "Example");
    prefix.extend_from_slice(&[0; 7]);
    prefix.extend_from_slice(&[1, 2]);
    prefix.extend_from_slice(&[0; 10]);
    prefix.extend_from_slice(&17_u32.to_le_bytes());
    prefix.extend_from_slice(&252_u32.to_le_bytes());
    prefix.extend_from_slice(&0.25_f64.to_le_bytes());
    prefix.extend_from_slice(&252_u32.to_le_bytes());
    prefix.extend_from_slice(&[1, 3]);
    prefix.extend_from_slice(&[0; 9]);
    prefix.extend_from_slice(&[1, 0, 0, 1, 4]);
    prefix.extend_from_slice(&[0; 11]);
    let paired = usize::try_from(parse_scope(&bytes, 0).unwrap().paired_byte_offset()).unwrap();
    prefix.extend_from_slice(&bytes[paired..]);
    prefix
}

fn named_scale_fixture(
    kind: &str,
    reference_count: usize,
    tail_length: usize,
    start: usize,
) -> (Vec<u8>, usize) {
    let mut bytes = vec![0; start];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"350");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2.5f64.to_le_bytes());
    for target in [105u32, 101] {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&102u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    bytes.extend_from_slice(&u32::try_from(reference_count).unwrap().to_le_bytes());
    let references: &[u32] = match reference_count {
        5 => &[101, 102, 103, 104, 105],
        6 => &[101, 102, 103, 104, 106, 105],
        _ => panic!("synthetic Scale reference count"),
    };
    for target in references {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(*target).to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    }
    bytes.extend_from_slice(&19u32.to_le_bytes());
    lp_utf16(&mut bytes, kind);
    let tail_at = bytes.len();
    bytes.resize(tail_at + tail_length, 0);
    bytes[tail_at..tail_at + 4].copy_from_slice(&3u32.to_le_bytes());
    let previous_at = tail_at
        + match tail_length {
            72 | 76 => 30,
            77 | 78 => 31,
            87 => 41,
            _ => panic!("synthetic Scale tail length"),
        };
    bytes[previous_at..previous_at + 4].copy_from_slice(&17u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"263");
    bytes.extend_from_slice(&100u32.to_le_bytes());

    let point_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&105u32.to_le_bytes());
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
    for target in [104u64, 103] {
        bytes.push(1);
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    }
    bytes.resize(point_at + 208, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&105u32.to_le_bytes());
    (bytes, position_at)
}
