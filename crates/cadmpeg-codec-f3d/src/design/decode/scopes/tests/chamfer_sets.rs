// SPDX-License-Identifier: Apache-2.0
use super::super::chamfer_set::{
    chamfer_set_conflict_loss, chamfer_set_list, retain_consistent_chamfer_sets,
};
use super::super::fillet_set::decoded_group_keys;
use crate::records::feature::chamfer_set::{DesignChamferSetListForm, DesignChamferSetSpec};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{
    DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire,
    DesignParameterSource,
};

const STREAM: &str = "f3d:Design/BulkStream.dat";
const SCOPE: u32 = 282;
const GROUP: u32 = 283;
const SET: u32 = 290;
const FIRST_OWNER: u32 = 292;
const SECOND_OWNER: u32 = 295;
const AUXILIARY: u32 = 294;

fn header(class: &[u8; 3], record_index: u32) -> Vec<u8> {
    let mut out = 3_u32.to_le_bytes().to_vec();
    out.extend_from_slice(class);
    out.extend_from_slice(&record_index.to_le_bytes());
    out
}

fn reference(record_index: u32) -> Vec<u8> {
    let mut out = vec![1];
    out.extend_from_slice(&record_index.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    out
}

#[derive(Clone, Copy)]
enum Form {
    Equal,
    TwoDistances,
    DistanceAngle,
    CompactEqual,
}

struct SetLayout {
    len: usize,
    kind_at: usize,
    kind: u32,
    group: usize,
    word: usize,
    byte: Option<usize>,
    auxiliary: usize,
    scope: usize,
    owners: &'static [usize],
}

fn set_frame(form: Form) -> Vec<u8> {
    let layout = |len, kind_at, kind, group, word, byte, auxiliary, scope, owners| SetLayout {
        len,
        kind_at,
        kind,
        group,
        word,
        byte,
        auxiliary,
        scope,
        owners,
    };
    let SetLayout {
        len,
        kind_at,
        kind,
        group,
        word,
        byte,
        auxiliary,
        scope,
        owners,
    } = match form {
        Form::Equal => layout(83, 36, 1, 40, 53, Some(57), 60, 72, &[24]),
        Form::TwoDistances => layout(93, 46, 2, 50, 63, Some(67), 70, 82, &[24, 35]),
        Form::DistanceAngle => layout(93, 46, 3, 50, 63, Some(67), 70, 82, &[34, 23]),
        Form::CompactEqual => layout(82, 36, 1, 40, 53, None, 59, 71, &[24]),
    };
    let mut frame = header(b"366", SET);
    frame.resize(len, 0);
    frame[21] = 1;
    frame[kind_at..kind_at + 4].copy_from_slice(&kind.to_le_bytes());
    frame[group..group + 11].copy_from_slice(&reference(GROUP));
    frame[word..word + 4].copy_from_slice(&1_u32.to_le_bytes());
    if let Some(byte) = byte {
        frame[byte] = 1;
    }
    frame[auxiliary..auxiliary + 11].copy_from_slice(&reference(AUXILIARY));
    frame[scope..scope + 11].copy_from_slice(&reference(SCOPE));
    for (at, owner) in owners.iter().zip([FIRST_OWNER, SECOND_OWNER]) {
        frame[*at..at + 11].copy_from_slice(&reference(owner));
    }
    frame
}

fn stream(form: Form, compact: bool) -> (Vec<u8>, DesignParameterScope) {
    stream_with(form, compact, 1, DesignFeatureKind::Chamfer)
}

fn stream_with(
    form: Form,
    compact: bool,
    set_count: u32,
    kind: DesignFeatureKind,
) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = header(b"261", SCOPE);
    bytes.extend_from_slice(&[0; 9]);
    if compact {
        bytes.push(1);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
    } else {
        bytes.push(0);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    if compact {
        bytes.push(1);
    } else {
        bytes.extend_from_slice(&[0, 1]);
    }
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for ordinal in 1_u64..=2 {
        bytes.extend_from_slice(&ordinal.to_le_bytes());
        bytes.extend_from_slice(&[1, 0]);
    }
    bytes.extend_from_slice(&set_count.to_le_bytes());
    for _ in 0..set_count {
        bytes.extend(reference(SET));
    }
    bytes.extend_from_slice(&set_count.to_le_bytes());
    for _ in 0..set_count {
        bytes.extend(reference(GROUP));
    }
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    let mut scope =
        DesignParameterScope::empty(&format!("{STREAM}:design-parameter-scope#0"), kind, SCOPE);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                GROUP,
                FIRST_OWNER,
                SECOND_OWNER,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let frame_length = usize::try_from(scope.frame_length()).unwrap();
    assert!(frame_length > bytes.len());
    bytes.resize(frame_length, 0);
    bytes.extend(header(b"257", SCOPE));
    bytes.extend(set_frame(form));
    bytes.extend(header(b"267", SET));
    bytes.extend(header(b"285", AUXILIARY));
    bytes.resize(bytes.len() + 47, 0);
    (bytes, scope)
}

fn owned(owner_record_index: u32, source_kind: &str) -> (DesignParameter, DesignParameterOwner) {
    let parameter_record_index = owner_record_index - 1;
    let base = u64::from(owner_record_index) * 200;
    let parameter = DesignParameter::try_from(DesignParameterDraft {
        id: format!("{STREAM}:design-parameter#{parameter_record_index}"),
        byte_offset: base,
        class_tag: "305".to_owned().try_into().unwrap(),
        record_index: parameter_record_index,
        source_ordinal: 0,
        source: DesignParameterSource::new(source_kind.to_owned(), Some(owner_record_index), None)
            .unwrap(),
        expression: "1".to_owned(),
        expression_offset: base + 20,
        source_kind_offset: base + 40,
        unit: None,
        name: source_kind.into(),
        name_offset: base + 60,
        evaluated_value: 1.0,
        evaluated_value_offset: base + 80,
    })
    .unwrap();
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("{STREAM}:design-parameter-owner#{owner_record_index}"),
        byte_offset: base + 100,
        frame_length: 99,
        class_tag: "296".to_owned().try_into().unwrap(),
        record_index: owner_record_index,
        scope_record_index: SCOPE,
        local_ordinal: 0,
        evaluated_value: 1.0,
        evaluated_value_offset: base + 140,
        parameter_record_index,
        owned_ordinal: 0,
        variant: None,
        companion_record_index: owner_record_index + 1,
    })
    .unwrap();
    (parameter, owner)
}

fn decode_bytes(
    bytes: &[u8],
    scope: &DesignParameterScope,
    kinds: [&str; 2],
) -> Option<crate::records::feature::chamfer_set::DesignChamferSetList> {
    let (first_parameter, first_owner) = owned(FIRST_OWNER, kinds[0]);
    let (second_parameter, second_owner) = owned(SECOND_OWNER, kinds[1]);
    chamfer_set_list(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
        &[first_parameter, second_parameter],
        &[first_owner, second_owner],
    )
}

fn decode(
    form: Form,
    compact: bool,
    kinds: [&str; 2],
) -> Option<crate::records::feature::chamfer_set::DesignChamferSetList> {
    let (bytes, scope) = stream(form, compact);
    decode_bytes(&bytes, &scope, kinds)
}

#[test]
fn each_set_form_names_its_group_owners_and_auxiliary_record() {
    let list = decode(Form::Equal, false, ["Distance", "Angle"]).unwrap();
    assert_eq!(list.form, DesignChamferSetListForm::Current);
    assert_eq!(list.entry_count, 2);
    let [set] = list.sets.as_slice() else {
        panic!("one set");
    };
    assert_eq!(
        (set.record_index, set.frame_length, set.group_record_index),
        (SET, 83, GROUP)
    );
    assert_eq!(set.auxiliary_record_index, AUXILIARY);
    assert!(set.tangent_chain);
    assert_eq!(set.entry_count, 2);
    assert_eq!(
        list.corner_type,
        crate::records::feature::chamfer_set::DesignChamferCornerType::Chamfer
    );
    assert_eq!(
        set.spec,
        DesignChamferSetSpec::EqualDistance {
            distance_owner_record_index: FIRST_OWNER
        }
    );
    let two = decode(Form::TwoDistances, false, ["Distance 1", "Distance 2"]).unwrap();
    assert_eq!(
        two.sets[0].spec,
        DesignChamferSetSpec::TwoDistances {
            first_owner_record_index: FIRST_OWNER,
            second_owner_record_index: SECOND_OWNER
        }
    );
    let angle = decode(Form::DistanceAngle, false, ["leftDistance", "Rotate Angle"]).unwrap();
    assert_eq!(
        angle.sets[0].spec,
        DesignChamferSetSpec::DistanceAngle {
            distance_owner_record_index: FIRST_OWNER,
            angle_owner_record_index: SECOND_OWNER
        }
    );
    let compact = decode(Form::CompactEqual, true, ["Distance", "Angle"]).unwrap();
    assert_eq!(compact.form, DesignChamferSetListForm::Compact);
    assert_eq!(compact.sets[0].frame_length, 82);
}

#[test]
fn an_owner_outside_the_spec_namespace_rejects_the_set() {
    assert!(decode(Form::Equal, false, ["Distance 1", "Angle"]).is_none());
    assert!(decode(Form::TwoDistances, false, ["Distance 1", "Angle"]).is_none());
    assert!(decode(Form::DistanceAngle, false, ["Distance", "Distance 2"]).is_none());
}

#[test]
fn each_unadmitted_byte_rejects_the_set_list() {
    let (bytes, scope) = stream(Form::Equal, false);
    let set_start =
        crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
    let cases = [
        (15, 1),
        (20, 2),
        (21, 4),
        (39, 2),
        (40, 1),
        (25, 2),
        (31, 3),
        (39, 0),
        (41, 9),
        (51, 2),
        (27, 0),
        (66, 2),
        (81, 35),
        (71, 0x30),
        (set_start + 41, 0x30),
        (set_start + 61, 0x30),
        (set_start + 21, 2),
        (set_start + 22, 1),
        (set_start + 36, 2),
        (set_start + 53, 2),
        (set_start + 57, 0),
        (set_start + 73, 9),
    ];
    for (at, value) in cases {
        let mut bytes = bytes.clone();
        bytes[at] = value;
        assert!(
            decode_bytes(&bytes, &scope, ["Distance", "Angle"]).is_none(),
            "byte {at} = {value}"
        );
    }
}

#[test]
fn a_mixed_two_distance_namespace_rejects_the_set() {
    assert!(decode(Form::TwoDistances, false, ["Distance 1", "rightDistance"]).is_none());
    assert!(decode(Form::TwoDistances, false, ["leftDistance", "Distance 2"]).is_none());
    assert!(decode(Form::TwoDistances, false, ["leftDistance", "rightDistance"]).is_some());
}

#[test]
fn each_form_rejects_a_nonzero_unnamed_byte_and_a_second_frame() {
    for (form, kinds) in [
        (Form::TwoDistances, ["Distance 1", "Distance 2"]),
        (Form::DistanceAngle, ["Distance", "Angle"]),
        (Form::CompactEqual, ["Distance", "Angle"]),
    ] {
        let (bytes, scope) = stream(form, matches!(form, Form::CompactEqual));
        assert!(decode_bytes(&bytes, &scope, kinds).is_some());
        let set_start =
            crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
        let mut mutated = bytes.clone();
        mutated[set_start + 22] = 1;
        assert!(decode_bytes(&mutated, &scope, kinds).is_none());
        let mut third = bytes.clone();
        third.extend(header(b"268", SET));
        third.resize(third.len() + 16, 0);
        assert!(decode_bytes(&third, &scope, kinds).is_none());
    }
}

#[test]
fn prologue_kind_and_duplicate_references_reject_the_set_list() {
    let (mut bytes, scope) = stream(Form::CompactEqual, true);
    bytes[21] = 1;
    assert!(decode_bytes(&bytes, &scope, ["Distance", "Angle"]).is_none());
    let (bytes, scope) = stream_with(Form::Equal, false, 1, DesignFeatureKind::Fillet);
    assert!(decode_bytes(&bytes, &scope, ["Distance", "Angle"]).is_none());
    let (bytes, scope) = stream_with(Form::Equal, false, 2, DesignFeatureKind::Chamfer);
    assert!(decode_bytes(&bytes, &scope, ["Distance", "Angle"]).is_none());
    let (bytes, scope) = stream(Form::Equal, false);
    let (first_parameter, first_owner) = owned(FIRST_OWNER, "Distance");
    assert!(chamfer_set_list(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[first_parameter],
        &[first_owner.clone(), first_owner],
    )
    .is_none());
}

fn crossed_sets(crossed: bool) -> (DesignParameterScope, [DesignParameterOwner; 2]) {
    use crate::records::feature::chamfer_set::{DesignChamferSet, DesignChamferSetList};
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#0"),
        DesignFeatureKind::Chamfer,
        SCOPE,
    );
    crate::design::test_support::name_scope_references(&mut scope, [GROUP, GROUP + 1]);
    let owner = |record_index: u32, local_ordinal: u32| {
        let (_, owner) = owned(record_index, "Distance");
        let mut wire = serde_json::to_value(&owner).unwrap();
        wire["local_ordinal"] = serde_json::json!(local_ordinal);
        serde_json::from_value::<DesignParameterOwner>(wire).unwrap()
    };
    let owners = [owner(FIRST_OWNER, 0), owner(SECOND_OWNER, 1)];
    let (first, second) = if crossed {
        (SECOND_OWNER, FIRST_OWNER)
    } else {
        (FIRST_OWNER, SECOND_OWNER)
    };
    let set = |group_record_index: u32, distance_owner_record_index: u32| DesignChamferSet {
        record_index: SET,
        byte_offset: 0,
        frame_length: 83,
        group_record_index,
        spec: DesignChamferSetSpec::EqualDistance {
            distance_owner_record_index,
        },
        auxiliary_record_index: AUXILIARY,
        entry_count: 1,
        tangent_chain: true,
    };
    if let crate::records::feature::scope::DesignScopePayloadMut::Chamfer(slot) =
        scope.payload_mut()
    {
        slot.sets = Some(DesignChamferSetList {
            form: DesignChamferSetListForm::Current,
            corner_type: crate::records::feature::chamfer_set::DesignChamferCornerType::Chamfer,
            entry_count: 1,
            sets: vec![set(GROUP, first), set(GROUP + 1, second)],
        });
    }
    (scope, owners)
}

fn decoded_keys() -> super::super::fillet_set::DecodedGroupKeys {
    [GROUP, GROUP + 1]
        .into_iter()
        .map(|group| (STREAM.to_owned(), SCOPE, group))
        .collect()
}

#[test]
fn a_set_list_in_owner_order_is_kept() {
    let (scope, owners) = crossed_sets(false);
    let mut scopes = vec![scope];
    let conflicts = retain_consistent_chamfer_sets(&mut scopes, &decoded_keys(), &owners);
    assert!(conflicts.is_empty());
    assert!(scopes[0].chamfer_sets().is_some());
    assert!(chamfer_set_conflict_loss(&conflicts).is_none());
}

#[test]
fn a_set_list_against_owner_order_is_dropped_and_reported() {
    let (scope, owners) = crossed_sets(true);
    let mut scopes = vec![scope];
    let conflicts = retain_consistent_chamfer_sets(&mut scopes, &decoded_keys(), &owners);
    assert_eq!(conflicts, [scopes[0].id.clone()]);
    assert!(scopes[0].chamfer_sets().is_none());
    assert!(scopes[0].payload().is_uninterpreted());
    let loss = chamfer_set_conflict_loss(&conflicts).unwrap();
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeatureChamferSetConflict.kind()
    );
}

#[test]
fn an_ungrouped_set_list_is_cleared() {
    let (bytes, mut scope) = stream(Form::Equal, false);
    let list = decode_bytes(&bytes, &scope, ["Distance", "Angle"]);
    assert!(list.is_some());
    if let crate::records::feature::scope::DesignScopePayloadMut::Chamfer(slot) =
        scope.payload_mut()
    {
        slot.sets = list;
    }
    let mut scopes = vec![scope];
    assert!(!scopes[0].payload().is_uninterpreted());
    let conflicts = retain_consistent_chamfer_sets(&mut scopes, &decoded_group_keys(&[]), &[]);
    assert!(conflicts.is_empty());
    assert!(scopes[0].chamfer_sets().is_none());
    assert!(scopes[0].payload().is_uninterpreted());
}

#[test]
fn corner_type_and_tangent_chain_read_from_their_bytes() {
    use crate::records::feature::chamfer_set::DesignChamferCornerType;
    let (bytes, scope) = stream(Form::Equal, false);
    let set_start =
        crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
    for (value, corner_type) in [
        (2, DesignChamferCornerType::Miter),
        (3, DesignChamferCornerType::Blend),
    ] {
        let mut bytes = bytes.clone();
        bytes[21] = value;
        bytes[set_start + 21] = 0;
        let list = decode_bytes(&bytes, &scope, ["Distance", "Angle"]).unwrap();
        assert_eq!(list.corner_type, corner_type);
        assert!(!list.sets[0].tangent_chain);
    }
}

#[test]
fn every_entry_names_a_set_and_every_set_has_an_entry() {
    use super::super::chamfer_set::set_entry_counts;
    assert_eq!(set_entry_counts(&[1, 2, 2], 2), Some(vec![1, 2]));
    assert_eq!(set_entry_counts(&[1, 1], 2), None);
    assert_eq!(set_entry_counts(&[1, 3], 2), None);
    assert_eq!(set_entry_counts(&[0, 1], 1), None);
}

#[test]
fn a_scope_can_name_equal_and_two_distance_set_frames_together() {
    let (_, mut scope) = stream(Form::Equal, false);
    let other_set = SET + 20;
    let other_group = GROUP + 20;
    let mut bytes = header(b"261", SCOPE);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for (ordinal, set_number) in [(1_u64, 1_u8), (2, 2)] {
        bytes.extend_from_slice(&ordinal.to_le_bytes());
        bytes.extend_from_slice(&[set_number, 0]);
    }
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend(reference(SET));
    bytes.extend(reference(other_set));
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend(reference(GROUP));
    bytes.extend(reference(other_group));
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    crate::design::test_support::name_scope_references(&mut scope, [GROUP, other_group]);
    bytes.resize(usize::try_from(scope.frame_length()).unwrap(), 0);
    bytes.extend(header(b"257", SCOPE));
    bytes.extend(set_frame(Form::Equal));
    bytes.extend(header(b"267", SET));
    let mut second = set_frame(Form::TwoDistances);
    second[7..11].copy_from_slice(&other_set.to_le_bytes());
    second[24..35].copy_from_slice(&reference(FIRST_OWNER + 20));
    second[35..46].copy_from_slice(&reference(SECOND_OWNER + 20));
    second[50..61].copy_from_slice(&reference(other_group));
    second[63..67].copy_from_slice(&2_u32.to_le_bytes());
    bytes.extend(second);
    bytes.extend(header(b"267", other_set));
    bytes.extend(header(b"285", AUXILIARY));
    bytes.resize(bytes.len() + 47, 0);
    let owned = [
        owned(FIRST_OWNER, "Distance"),
        owned(FIRST_OWNER + 20, "Distance 1"),
        owned(SECOND_OWNER + 20, "Distance 2"),
    ];
    let parameters = owned
        .iter()
        .map(|(parameter, _)| parameter.clone())
        .collect::<Vec<_>>();
    let owners = owned
        .into_iter()
        .map(|(_, owner)| owner)
        .collect::<Vec<_>>();
    let list = chamfer_set_list(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &parameters,
        &owners,
    )
    .unwrap();
    assert_eq!(list.sets.len(), 2);
    assert!(matches!(
        list.sets[0].spec,
        DesignChamferSetSpec::EqualDistance { .. }
    ));
    assert!(matches!(
        list.sets[1].spec,
        DesignChamferSetSpec::TwoDistances { .. }
    ));
    for end in 0..usize::try_from(scope.frame_length()).unwrap() {
        assert!(chamfer_set_list(
            &bytes[..end],
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes[..end]),
            &scope,
            &parameters,
            &owners
        )
        .is_none());
    }
}
