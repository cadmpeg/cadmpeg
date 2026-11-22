// SPDX-License-Identifier: Apache-2.0
use super::super::fillet_set::{
    decoded_group_keys, fillet_set_conflict_loss, retain_consistent_fillet_sets,
};
use crate::records::feature::fillet_set::{DesignFilletSetLaw, DesignFilletSetListForm};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{
    DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire,
    DesignParameterSource,
};

const STREAM: &str = "f3d:Design/BulkStream.dat";

/// The fillet set list without its padding observation.
fn fillet_set_list(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[DesignParameterOwner],
) -> Option<crate::records::feature::fillet_set::DesignFilletSetList> {
    super::super::fillet_set::fillet_set_list(bytes, records, scope, parameters, parameter_owners)
        .map(|(list, _)| list)
}
const SCOPE: u32 = 282;
const SET: u32 = 283;
const GROUP: u32 = 284;
const TANGENCY_OWNER: u32 = 292;
const LAW_OWNER: u32 = 295;

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
    Constant,
    Chordal,
    Compact,
    ShortConstant,
}

struct Fixture {
    prologue_one: bool,
    compact_run: bool,
    set_form: Form,
    prefix_group: u32,
    set_group: u32,
    set_count: u32,
    group_count: u32,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            prologue_one: false,
            compact_run: false,
            set_form: Form::Constant,
            prefix_group: GROUP,
            set_group: GROUP,
            set_count: 1,
            group_count: 1,
        }
    }
}

fn set_frame(form: Form, group: u32) -> Vec<u8> {
    let (len, marker, flag, group_at, law_at, tangency_at) = match form {
        Form::Constant => (97, 23, Some(26), 29, 63, Some(82)),
        Form::Chordal => (97, 22, Some(26), 39, 28, Some(82)),
        Form::Compact => (93, 22, Some(24), 27, 59, Some(78)),
        Form::ShortConstant => (80, 22, None, 26, 57, None),
    };
    let mut frame = header(b"350", SET);
    frame.resize(len, 0);
    frame[marker] = 1;
    if let Some(flag) = flag {
        frame[flag] = 1;
    }
    frame[group_at..group_at + 11].copy_from_slice(&reference(group));
    frame[law_at..law_at + 11].copy_from_slice(&reference(LAW_OWNER));
    if let Some(tangency_at) = tangency_at {
        frame[tangency_at..tangency_at + 11].copy_from_slice(&reference(TANGENCY_OWNER));
    }
    frame
}

fn stream(fixture: &Fixture) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = header(b"336", SCOPE);
    bytes.extend_from_slice(&[0; 9]);
    if fixture.prologue_one {
        bytes.push(1);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
    } else {
        bytes.push(0);
    }
    bytes.extend_from_slice(&fixture.set_count.to_le_bytes());
    if fixture.set_count != 0 {
        bytes.extend(reference(SET));
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    if fixture.compact_run {
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    } else {
        bytes.extend_from_slice(&[1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
    bytes.extend_from_slice(&fixture.group_count.to_le_bytes());
    if fixture.set_count != 0 {
        bytes.extend(reference(fixture.prefix_group));
    }
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#0"),
        DesignFeatureKind::Fillet,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                GROUP,
                TANGENCY_OWNER,
                LAW_OWNER,
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
    bytes.extend(set_frame(fixture.set_form, fixture.set_group));
    bytes.extend(header(b"343", GROUP));
    bytes.resize(bytes.len() + 32, 0);
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

fn decode(
    fixture: &Fixture,
    law_kind: &str,
) -> Option<crate::records::feature::fillet_set::DesignFilletSetList> {
    let (bytes, scope) = stream(fixture);
    let (law_parameter, law_owner) = owned(LAW_OWNER, law_kind);
    let (tangency_parameter, tangency_owner) = owned(TANGENCY_OWNER, "TangencyWeight");
    fillet_set_list(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[law_parameter, tangency_parameter],
        &[law_owner, tangency_owner],
    )
}

#[test]
fn short_constant_set_requires_its_following_group_and_radius_owner() {
    let fixture = Fixture {
        compact_run: true,
        set_form: Form::ShortConstant,
        ..Fixture::default()
    };
    let list = decode(&fixture, "Radius").expect("short constant set");
    let [set] = list.sets.as_slice() else {
        panic!("one short constant set");
    };
    assert_eq!(set.frame_length, 80);
    assert_eq!(set.group_record_index, GROUP);
    assert_eq!(
        set.law,
        DesignFilletSetLaw::Constant {
            radius_owner_record_index: LAW_OWNER,
        }
    );
    assert_eq!(set.tangency_weight_owner_record_index, None);
    assert_eq!(set.tangent_chain, None);
    assert_eq!(set.curvature_continuity, None);
    let stored = crate::json_value(&list).unwrap();
    let restored: crate::records::feature::fillet_set::DesignFilletSetList =
        serde_json::from_value(stored).unwrap();
    assert_eq!(restored, list);
    assert!(decode(&fixture, "ChordLen").is_none());

    let (mut bytes, scope) = stream(&fixture);
    let end = usize::try_from(set.byte_offset + set.frame_length).unwrap();
    let other_group = header(b"343", GROUP + 10);
    bytes[end..end + other_group.len()].copy_from_slice(&other_group);
    let (parameter, owner) = owned(LAW_OWNER, "Radius");
    assert!(fillet_set_list(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[parameter],
        &[owner],
    )
    .is_none());
}

#[test]
fn constant_set_names_group_radius_and_tangency_owners() {
    let list = decode(&Fixture::default(), "Radius").unwrap();
    assert_eq!(list.form, DesignFilletSetListForm::Current);
    let [set] = list.sets.as_slice() else {
        panic!("one set");
    };
    assert_eq!(set.record_index, SET);
    assert_eq!(set.frame_length, 97);
    assert_eq!(set.group_record_index, GROUP);
    assert_eq!(
        set.law,
        DesignFilletSetLaw::Constant {
            radius_owner_record_index: LAW_OWNER
        }
    );
    assert_eq!(set.tangency_weight_owner_record_index, Some(TANGENCY_OWNER));
    assert_eq!(set.tangent_chain, Some(true));
    assert_eq!(set.curvature_continuity, Some(false));
    assert!(list.rolling_ball_corner);
}

#[test]
fn chordal_set_names_chord_length_owner() {
    let fixture = Fixture {
        set_form: Form::Chordal,
        ..Fixture::default()
    };
    let list = decode(&fixture, "ChordLen").unwrap();
    assert_eq!(
        list.sets[0].law,
        DesignFilletSetLaw::Chordal {
            chord_length_owner_record_index: LAW_OWNER
        }
    );
}

#[test]
fn compact_set_follows_prologue_and_compact_flag_run() {
    let fixture = Fixture {
        prologue_one: true,
        compact_run: true,
        set_form: Form::Compact,
        ..Fixture::default()
    };
    let list = decode(&fixture, "Radius").unwrap();
    assert_eq!(list.form, DesignFilletSetListForm::Compact);
    assert_eq!(list.sets[0].frame_length, 93);
    assert_eq!(list.sets[0].group_record_index, GROUP);
}

/// Compact flag-run start in a prologue-one prefix of one set.
const COMPACT_RUN_AT: usize = 44;

fn decode_bytes(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<(
    crate::records::feature::fillet_set::DesignFilletSetList,
    bool,
)> {
    decode_bytes_as(bytes, scope, "Radius")
}

fn decode_bytes_as(
    bytes: &[u8],
    scope: &DesignParameterScope,
    law_kind: &str,
) -> Option<(
    crate::records::feature::fillet_set::DesignFilletSetList,
    bool,
)> {
    let (law_parameter, law_owner) = owned(LAW_OWNER, law_kind);
    let (tangency_parameter, tangency_owner) = owned(TANGENCY_OWNER, "TangencyWeight");
    super::super::fillet_set::fillet_set_list(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
        &[law_parameter, tangency_parameter],
        &[law_owner, tangency_owner],
    )
}

#[test]
fn compact_padding_bytes_hold_any_value() {
    let fixture = Fixture {
        prologue_one: true,
        compact_run: true,
        set_form: Form::Compact,
        ..Fixture::default()
    };
    let (mut bytes, scope) = stream(&fixture);
    assert_eq!(
        bytes[COMPACT_RUN_AT..COMPACT_RUN_AT + 2],
        [1, 1],
        "fixture places the compact run"
    );
    assert!(!decode_bytes(&bytes, &scope).unwrap().1);
    let set_start =
        crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
    bytes[COMPACT_RUN_AT + 2..COMPACT_RUN_AT + 11]
        .copy_from_slice(&[0, 0x10, 0x77, 0x9f, 0xce, 0xb1, 0x02, 0, 0]);
    bytes[set_start + 70..set_start + 74].copy_from_slice(&[0x30, 0, 0x30, 0]);
    bytes[set_start + 89..set_start + 93].copy_from_slice(&[0x11, 0xe4, 0x4a, 0xc0]);
    let (list, padding_holds_bytes) =
        decode_bytes(&bytes, &scope).expect("compact padding bytes hold any value");
    assert_eq!(list.form, DesignFilletSetListForm::Compact);
    assert_eq!(list.sets[0].group_record_index, GROUP);
    assert!(padding_holds_bytes);
    for at in [COMPACT_RUN_AT + 4, set_start + 71, set_start + 90] {
        let mut single = stream(&fixture).0;
        single[at] = 0x30;
        assert_eq!(
            decode_bytes(&single, &scope).map(|(_, flag)| flag),
            Some(true),
            "byte {at}"
        );
    }

    for (at, value) in [
        (COMPACT_RUN_AT, 0),
        (COMPACT_RUN_AT + 1, 0),
        (set_start + 38, 1),
        (set_start + 69, 1),
        (set_start + 88, 1),
    ] {
        let mut broken = bytes.clone();
        broken[at] = value;
        assert!(
            decode_bytes(&broken, &scope).is_none(),
            "byte {at} = {value}"
        );
    }
}

#[test]
fn constant_form_bytes_after_its_owners_stay_zero() {
    let (bytes, scope) = stream(&Fixture::default());
    let set_start =
        crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
    for at in [set_start + 74, set_start + 93] {
        let mut broken = bytes.clone();
        broken[at] = 0x30;
        assert!(decode_bytes(&broken, &scope).is_none(), "byte {at}");
    }
    let chordal = Fixture {
        set_form: Form::Chordal,
        ..Fixture::default()
    };
    let (bytes, scope) = stream(&chordal);
    let set_start =
        crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
    assert!(decode_bytes_as(&bytes, &scope, "ChordLen").is_some());
    for at in [set_start + 70, set_start + 89] {
        let mut broken = bytes.clone();
        broken[at] = 0x30;
        assert!(
            decode_bytes_as(&broken, &scope, "ChordLen").is_none(),
            "byte {at}"
        );
    }
}

#[test]
fn law_owner_of_another_source_kind_rejects_the_set() {
    assert!(decode(&Fixture::default(), "ChordLen").is_none());
    let chordal = Fixture {
        set_form: Form::Chordal,
        ..Fixture::default()
    };
    assert!(decode(&chordal, "Radius").is_none());
}

#[test]
fn set_group_must_match_prefix_group_and_scope_reference() {
    let mismatched = Fixture {
        set_group: GROUP + 1,
        ..Fixture::default()
    };
    assert!(decode(&mismatched, "Radius").is_none());
    let unreferenced = Fixture {
        prefix_group: GROUP + 1,
        set_group: GROUP + 1,
        ..Fixture::default()
    };
    assert!(decode(&unreferenced, "Radius").is_none());
}

#[test]
fn nonzero_unnamed_set_byte_rejects_the_set() {
    let (mut bytes, scope) = stream(&Fixture::default());
    let set_start =
        crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
    bytes[set_start + 50] = 1;
    let (law_parameter, law_owner) = owned(LAW_OWNER, "Radius");
    let (tangency_parameter, tangency_owner) = owned(TANGENCY_OWNER, "TangencyWeight");
    assert!(fillet_set_list(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[law_parameter, tangency_parameter],
        &[law_owner, tangency_owner],
    )
    .is_none());
}

#[test]
fn empty_or_unpaired_prefix_lists_reject_the_set_list() {
    let empty = Fixture {
        set_count: 0,
        group_count: 0,
        ..Fixture::default()
    };
    assert!(decode(&empty, "Radius").is_none());
    let unpaired = Fixture {
        group_count: 2,
        ..Fixture::default()
    };
    assert!(decode(&unpaired, "Radius").is_none());
}

fn group() -> crate::records::topology::construction::DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": format!("{STREAM}:group#{GROUP}"),
        "scope_record_index": SCOPE,
        "scope_reference_ordinal": 0,
        "record_index": GROUP,
        "byte_offset": 1000,
        "class_tag": "343",
        "members": [],
        "member_offsets": [],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X10.raw(),
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

fn radius_group(
    radius_parameter_record_index: u32,
) -> crate::records::topology::fillet::DesignFilletRadiusGroup {
    crate::records::topology::fillet::DesignFilletRadiusGroup {
        id: format!("{STREAM}:design-fillet-radius-group#{GROUP}"),
        scope_record_index: SCOPE,
        group_ordinal: 0,
        group_record_index: GROUP,
        edge_operand_record_indices: Vec::new(),
        law: crate::records::topology::fillet::DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index,
        },
        tangency_weight_parameter_record_index: Some(TANGENCY_OWNER - 1),
    }
}

fn retained(
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    radius_groups: &[crate::records::topology::fillet::DesignFilletRadiusGroup],
) -> (DesignParameterScope, Vec<String>) {
    let (bytes, mut scope) = stream(&Fixture::default());
    let (law_parameter, law_owner) = owned(LAW_OWNER, "Radius");
    let (tangency_parameter, tangency_owner) = owned(TANGENCY_OWNER, "TangencyWeight");
    let owners = [law_owner, tangency_owner];
    let list = fillet_set_list(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[law_parameter, tangency_parameter],
        &owners,
    );
    assert!(list.is_some());
    if let crate::records::feature::scope::DesignScopePayloadMut::Fillet(slot) = scope.payload_mut()
    {
        slot.sets = list;
    }
    let mut scopes = vec![scope];
    let conflicts = retain_consistent_fillet_sets(
        &mut scopes,
        &decoded_group_keys(groups),
        radius_groups,
        &owners,
    );
    (scopes.remove(0), conflicts)
}

#[test]
fn a_set_list_that_agrees_with_its_radius_group_is_kept() {
    let (scope, conflicts) = retained(&[group()], &[radius_group(LAW_OWNER - 1)]);
    assert!(scope.fillet_sets().is_some());
    assert!(!scope.payload().is_uninterpreted());
    assert!(conflicts.is_empty());
    let (ungrouped_radius, conflicts) = retained(&[group()], &[]);
    assert!(ungrouped_radius.fillet_sets().is_some());
    assert!(conflicts.is_empty());
}

#[test]
fn a_disagreeing_set_list_is_dropped_and_reported() {
    let mut chordal = radius_group(LAW_OWNER - 1);
    chordal.law = crate::records::topology::fillet::DesignFilletRadiusLaw::Chordal {
        chord_length_parameter_record_index: LAW_OWNER - 1,
    };
    let mut other_tangency = radius_group(LAW_OWNER - 1);
    other_tangency.tangency_weight_parameter_record_index = Some(LAW_OWNER - 1);
    let mut no_tangency = radius_group(LAW_OWNER - 1);
    no_tangency.tangency_weight_parameter_record_index = None;
    for radius_group in [
        radius_group(TANGENCY_OWNER - 1),
        chordal,
        other_tangency,
        no_tangency,
    ] {
        let (scope, conflicts) = retained(&[group()], &[radius_group]);
        assert!(scope.fillet_sets().is_none());
        assert!(scope.payload().is_uninterpreted());
        assert_eq!(conflicts, [scope.id.clone()]);
        let loss = fillet_set_conflict_loss(&conflicts).unwrap();
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::FeatureFilletSetConflict.kind()
        );
    }
}

#[test]
fn an_ungrouped_set_list_is_dropped_without_a_conflict() {
    let (scope, conflicts) = retained(&[], &[]);
    assert!(scope.fillet_sets().is_none());
    assert!(scope.payload().is_uninterpreted());
    assert!(conflicts.is_empty());
    assert!(fillet_set_conflict_loss(&conflicts).is_none());
}

#[test]
fn each_unadmitted_byte_rejects_the_set_list() {
    let (bytes, scope) = stream(&Fixture::default());
    let set_start =
        crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
    let cases = [
        (15, 1),
        (20, 2),
        (21, 9),
        (36, 1),
        (42, 0),
        (44, 1),
        (67, 35),
        (set_start + 23, 0),
        (set_start + 26, 2),
        (set_start + 97 + 4, b'x'),
    ];
    for (at, value) in cases {
        let mut bytes = bytes.clone();
        bytes[at] = value;
        let (law_parameter, law_owner) = owned(LAW_OWNER, "Radius");
        let (tangency_parameter, tangency_owner) = owned(TANGENCY_OWNER, "TangencyWeight");
        assert!(
            fillet_set_list(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                &scope,
                &[law_parameter, tangency_parameter],
                &[law_owner, tangency_owner],
            )
            .is_none(),
            "byte {at} = {value}"
        );
    }
}

#[test]
fn a_tangency_owner_of_another_kind_or_without_back_reference_rejects_the_set() {
    let (bytes, scope) = stream(&Fixture::default());
    let offsets = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let (law_parameter, law_owner) = owned(LAW_OWNER, "Radius");
    let (wrong_kind, wrong_kind_owner) = owned(TANGENCY_OWNER, "Radius");
    assert!(fillet_set_list(
        &bytes,
        &offsets,
        &scope,
        &[law_parameter.clone(), wrong_kind],
        &[law_owner.clone(), wrong_kind_owner],
    )
    .is_none());
    let (_, tangency_owner) = owned(TANGENCY_OWNER, "TangencyWeight");
    let (unlinked, _) = owned(TANGENCY_OWNER + 10, "TangencyWeight");
    assert!(fillet_set_list(
        &bytes,
        &offsets,
        &scope,
        &[law_parameter.clone(), unlinked],
        &[law_owner.clone(), tangency_owner.clone()],
    )
    .is_none());
    assert!(fillet_set_list(
        &bytes,
        &offsets,
        &scope,
        &[law_parameter],
        &[law_owner, tangency_owner.clone(), tangency_owner],
    )
    .is_none());
}

#[test]
fn a_set_ordinal_other_than_its_position_rejects_the_set() {
    let (mut bytes, scope) = stream(&Fixture::default());
    let set_start =
        crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
    bytes[set_start + 78] = 1;
    let (law_parameter, law_owner) = owned(LAW_OWNER, "Radius");
    let (tangency_parameter, tangency_owner) = owned(TANGENCY_OWNER, "TangencyWeight");
    assert!(fillet_set_list(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[law_parameter, tangency_parameter],
        &[law_owner, tangency_owner],
    )
    .is_none());
}

#[test]
fn rolling_ball_corner_and_curvature_continuity_read_from_their_bytes() {
    let (mut bytes, scope) = stream(&Fixture::default());
    let set_start =
        crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
    bytes[40] = 0;
    bytes[set_start + 25] = 1;
    let (law_parameter, law_owner) = owned(LAW_OWNER, "Radius");
    let (tangency_parameter, tangency_owner) = owned(TANGENCY_OWNER, "TangencyWeight");
    let parameters = [law_parameter, tangency_parameter];
    let owners = [law_owner, tangency_owner];
    let list = fillet_set_list(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &parameters,
        &owners,
    )
    .unwrap();
    assert!(!list.rolling_ball_corner);
    assert_eq!(list.sets[0].curvature_continuity, Some(true));
    for (at, value) in [(40, 2), (set_start + 25, 2)] {
        let mut bytes = bytes.clone();
        bytes[at] = value;
        assert!(fillet_set_list(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &parameters,
            &owners,
        )
        .is_none());
    }
    let chordal = Fixture {
        set_form: Form::Chordal,
        ..Fixture::default()
    };
    assert_eq!(
        decode(&chordal, "ChordLen").unwrap().sets[0].curvature_continuity,
        None
    );
}

#[test]
fn a_nonzero_byte_25_rejects_the_chordal_and_compact_forms() {
    for (fixture, law_kind) in [
        (
            Fixture {
                set_form: Form::Chordal,
                ..Fixture::default()
            },
            "ChordLen",
        ),
        (
            Fixture {
                prologue_one: true,
                compact_run: true,
                set_form: Form::Compact,
                ..Fixture::default()
            },
            "Radius",
        ),
    ] {
        let (mut bytes, scope) = stream(&fixture);
        let set_start =
            crate::design::test_support::indexed_record_offsets_for_test(&bytes).offsets(SET)[0];
        bytes[set_start + 25] = 1;
        let (law_parameter, law_owner) = owned(LAW_OWNER, law_kind);
        let (tangency_parameter, tangency_owner) = owned(TANGENCY_OWNER, "TangencyWeight");
        assert!(fillet_set_list(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[law_parameter, tangency_parameter],
            &[law_owner, tangency_owner],
        )
        .is_none());
    }
}

#[test]
fn set_ordinals_form_a_permutation_of_the_list_positions() {
    use super::super::fillet_set::ordinals_are_a_permutation;
    assert!(ordinals_are_a_permutation(&[0]));
    assert!(ordinals_are_a_permutation(&[2, 0, 1]));
    assert!(!ordinals_are_a_permutation(&[]));
    assert!(!ordinals_are_a_permutation(&[0, 0]));
    assert!(!ordinals_are_a_permutation(&[1]));
    assert!(!ordinals_are_a_permutation(&[0, 2]));
}

#[test]
fn asymmetric_fillet_set_requires_two_offset_owners_and_the_prefix_word() {
    use crate::layout::fillet_set_asymmetric as layout;

    let (mut bytes, scope) = stream(&Fixture::default());
    let set_at = usize::try_from(scope.frame_length()).unwrap() + header(b"257", SCOPE).len();
    let prefix_word_at = header(b"336", SCOPE).len() + 10 + size_of::<u32>() + reference(SET).len();
    bytes[prefix_word_at] = 1;
    let mut frame = header(b"350", SET);
    frame.resize(layout::LEN, 0);
    frame[layout::FORM_MARKER] = layout::FORM_MARKER_VALUE;
    for (at, value) in [
        (layout::COMPATIBILITY_WORD, layout::COMPATIBILITY_WORD_VALUE),
        (layout::FIRST_CONTROL_WORD, layout::FIRST_CONTROL_WORD_VALUE),
        (
            layout::SECOND_CONTROL_WORD,
            layout::SECOND_CONTROL_WORD_VALUE,
        ),
    ] {
        frame[at..at + size_of::<u32>()].copy_from_slice(&value.to_le_bytes());
    }
    for (at, target) in [
        (layout::EDGE_GROUP_REFERENCE, GROUP),
        (layout::OFFSET_ONE_OWNER_REFERENCE, LAW_OWNER),
        (layout::OFFSET_TWO_OWNER_REFERENCE, TANGENCY_OWNER),
    ] {
        let marked = reference(target);
        frame[at..at + marked.len()].copy_from_slice(&marked);
    }
    bytes[set_at..set_at + frame.len()].copy_from_slice(&frame);
    let (first, first_owner) = owned(LAW_OWNER, "EdgeOffset1");
    let (second, second_owner) = owned(TANGENCY_OWNER, "EdgeOffset2");
    let parameters = [first, second];
    let owners = [first_owner, second_owner];
    let read = |stream: &[u8], owners: &[DesignParameterOwner]| {
        fillet_set_list(
            stream,
            &crate::design::test_support::indexed_record_offsets_for_test(stream),
            &scope,
            &parameters,
            owners,
        )
    };
    let set = read(&bytes, &owners).unwrap().sets.remove(0);
    assert_eq!(
        set.law,
        DesignFilletSetLaw::Asymmetric {
            offset_one_owner_record_index: LAW_OWNER,
            offset_two_owner_record_index: TANGENCY_OWNER,
        }
    );
    assert_eq!(set.tangency_weight_owner_record_index, None);
    assert_eq!(set.tangent_chain, None);
    assert!(read(&bytes, &owners[..1]).is_none());
    assert!(read(&bytes[..set_at + frame.len()], &owners).is_none());
    bytes[prefix_word_at] = 0;
    assert!(read(&bytes, &owners).is_none());
}

#[test]
fn variable_fillet_set_keeps_midpoint_owner_pairs_and_refuses_repeated_links() {
    use crate::layout::fillet_set_variable as layout;
    use crate::records::feature::fillet_set::DesignFilletMidpointOwners;

    let (mut bytes, scope) = stream(&Fixture::default());
    let set_at = usize::try_from(scope.frame_length()).unwrap() + header(b"257", SCOPE).len();
    let mut frame = header(b"350", SET);
    frame.resize(layout::LEN, 0);
    for (at, value) in [
        (
            layout::MIDPOINT_PARAMETER_COUNT,
            layout::MIDPOINT_PARAMETER_COUNT_VALUE,
        ),
        (layout::RADIUS_COUNT, layout::RADIUS_COUNT_VALUE),
    ] {
        frame[at..at + size_of::<u32>()].copy_from_slice(&value.to_le_bytes());
    }
    let inputs = [
        (layout::START_RADIUS_OWNER_REFERENCE, 300, "StartRadius"),
        (layout::END_RADIUS_OWNER_REFERENCE, 303, "EndRadius"),
        (layout::MIDDLE_RADIUS_ONE_OWNER_REFERENCE, 306, "MidRadius"),
        (layout::MIDDLE_RADIUS_TWO_OWNER_REFERENCE, 309, "MidRadius"),
        (
            layout::MIDPOINT_PARAMETER_ONE_OWNER_REFERENCE,
            312,
            "MidParams",
        ),
        (
            layout::MIDPOINT_PARAMETER_TWO_OWNER_REFERENCE,
            315,
            "MidParams",
        ),
        (
            layout::TANGENCY_WEIGHT_OWNER_REFERENCE,
            TANGENCY_OWNER,
            "TangencyWeight",
        ),
    ];
    let mut parameters = Vec::new();
    let mut owners = Vec::new();
    for (at, index, kind) in inputs {
        let marked = reference(index);
        frame[at..at + marked.len()].copy_from_slice(&marked);
        let (parameter, owner) = owned(index, kind);
        parameters.push(parameter);
        owners.push(owner);
    }
    let group = reference(GROUP);
    frame[layout::EDGE_GROUP_REFERENCE..layout::EDGE_GROUP_REFERENCE + group.len()]
        .copy_from_slice(&group);
    drop(bytes.splice(
        set_at..set_at + set_frame(Form::Constant, GROUP).len(),
        frame,
    ));
    let read = |stream: &[u8]| {
        fillet_set_list(
            stream,
            &crate::design::test_support::indexed_record_offsets_for_test(stream),
            &scope,
            &parameters,
            &owners,
        )
    };
    assert_eq!(
        read(&bytes).unwrap().sets[0].law,
        DesignFilletSetLaw::Variable {
            start_radius_owner_record_index: 300,
            end_radius_owner_record_index: 303,
            middle: [
                DesignFilletMidpointOwners {
                    radius_owner_record_index: 306,
                    position_owner_record_index: 312,
                },
                DesignFilletMidpointOwners {
                    radius_owner_record_index: 309,
                    position_owner_record_index: 315,
                },
            ],
        }
    );
    let at = set_at + layout::MIDPOINT_PARAMETER_TWO_OWNER_REFERENCE;
    let repeated = reference(312);
    bytes[at..at + repeated.len()].copy_from_slice(&repeated);
    assert!(read(&bytes).is_none());
}
