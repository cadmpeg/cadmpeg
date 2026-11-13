// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::direct_face::exact_direct_face_operation;
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope_with_type_guid;
use crate::records::feature::direct_face::{
    DesignDirectFaceOperation, DesignShellOperation, DesignShellPrefixFlag,
};
use crate::records::references::DesignClassTag;
use crate::test_support::{indexed_header, lp_utf16};
use cadmpeg_ir::report::loss::LossNote;

const SHELL_TYPE_GUID: &str = "E8063467-BC05-4688-8196-AECB90F0EFEE";
const GROUP: u32 = 91;
const MEMBER: u32 = 92;
const INSIDE: u32 = 93;
const OUTSIDE: u32 = 94;

#[derive(Clone)]
enum Prefix {
    Cornered,
    Compact,
    Marked,
    Counted(Vec<u32>),
}

#[derive(Clone)]
struct Fixture {
    prefix: Prefix,
    flag: u8,
    class_tag: [u8; 3],
    tangent_chain: u8,
    rounded_corners: u8,
    inside: Option<u32>,
    outside: Option<u32>,
    table: Vec<u32>,
    group_reference: u32,
    gap_before_table: usize,
    scalars: Vec<(u32, f64)>,
    corrupt: Option<(usize, u8)>,
    lead: usize,
}

impl Fixture {
    fn new(inside: Option<u32>, outside: Option<u32>) -> Self {
        let mut table = vec![GROUP, MEMBER];
        table.extend(inside);
        table.extend(outside);
        Self {
            prefix: Prefix::Cornered,
            flag: 0,
            class_tag: *b"412",
            tangent_chain: 0,
            rounded_corners: 0,
            inside,
            outside,
            table,
            group_reference: GROUP,
            gap_before_table: 0,
            scalars: vec![(INSIDE, 0.25), (OUTSIDE, 0.5)],
            corrupt: None,
            lead: 0,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        fn marked(bytes: &mut Vec<u8>, record: u32) {
            bytes.push(1);
            bytes.extend_from_slice(&record.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, self.class_tag, 90);
        match self.prefix {
            Prefix::Cornered => {
                bytes.extend_from_slice(&[0; 10]);
                bytes.push(self.tangent_chain);
                bytes.push(self.rounded_corners);
            }
            Prefix::Compact => {
                bytes.extend_from_slice(&[0; 10]);
                bytes.push(self.flag);
            }
            Prefix::Marked | Prefix::Counted(_) => {
                bytes.extend_from_slice(&[0; 9]);
                bytes.push(1);
                bytes.push(self.flag);
                bytes.extend_from_slice(&[0, 0, 0, 1]);
            }
        }
        if matches!(self.prefix, Prefix::Counted(_)) {
            bytes.push(0);
        }
        for slot in [self.inside, self.outside] {
            match slot {
                Some(record) => marked(&mut bytes, record),
                None => bytes.push(0),
            }
        }
        match &self.prefix {
            Prefix::Cornered => bytes.extend_from_slice(&[0; 12]),
            Prefix::Compact | Prefix::Marked => bytes.extend_from_slice(&[0; 8]),
            Prefix::Counted(auxiliary) => {
                bytes.extend_from_slice(&[0; 4]);
                bytes.extend_from_slice(&u32::try_from(auxiliary.len()).unwrap().to_le_bytes());
                for record in auxiliary {
                    marked(&mut bytes, *record);
                }
                bytes.extend_from_slice(&[0; 4]);
            }
        }
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        marked(&mut bytes, self.group_reference);
        lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
        bytes.resize(bytes.len() + 3 + self.gap_before_table, 0);
        bytes.extend_from_slice(&u32::try_from(self.table.len()).unwrap().to_le_bytes());
        for record in &self.table {
            marked(&mut bytes, *record);
        }
        bytes.extend_from_slice(&17_u32.to_le_bytes());
        lp_utf16(&mut bytes, "Coque");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&2_u32.to_le_bytes());
        tail[31..35].copy_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        indexed_header(&mut bytes, *b"259", 90);
        for (record, value) in &self.scalars {
            let mut scalar = vec![0; 105];
            scalar[0..4].copy_from_slice(&3_u32.to_le_bytes());
            scalar[4..7].copy_from_slice(b"321");
            scalar[7..11].copy_from_slice(&record.to_le_bytes());
            scalar[24] = 1;
            scalar[25..29].copy_from_slice(&42_u32.to_le_bytes());
            scalar[40..48].copy_from_slice(&value.to_le_bytes());
            bytes.extend_from_slice(&scalar);
            indexed_header(&mut bytes, *b"265", *record);
        }
        if let Some((offset, value)) = self.corrupt {
            bytes[offset] = value;
        }
        bytes
    }

    fn decode(&self) -> Option<DesignShellOperation> {
        self.decode_with_losses().0
    }

    fn decode_with_losses(&self) -> (Option<DesignShellOperation>, Vec<LossNote>) {
        let mut bytes = vec![0; self.lead];
        bytes.extend(self.bytes());
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let class_tag =
            DesignClassTag::try_from(String::from_utf8(self.class_tag.to_vec()).unwrap()).unwrap();
        let scope = parse_parameter_scope_with_type_guid(
            &bytes,
            &records,
            90,
            &class_tag,
            u64::try_from(self.lead).unwrap(),
            Some(SHELL_TYPE_GUID),
        )
        .unwrap()
        .expect("Shell scope");
        let mut losses = Vec::new();
        let operation = match exact_direct_face_operation(&bytes, &records, &scope, &mut losses) {
            Some(DesignDirectFaceOperation::Shell(operation)) => Some(operation),
            Some(_) => panic!("expected a Shell operation"),
            None => None,
        };
        (operation, losses)
    }
}

#[test]
fn each_wall_slot_selects_its_direction_and_scalar() {
    let inside = Fixture::new(Some(INSIDE), None).decode().unwrap();
    assert_eq!(inside.thickness.get(), 0.25);
    assert_eq!(inside.thickness_record_index, INSIDE);
    assert!(!inside.outward);
    assert_eq!(inside.outward_offset, 23);
    assert_eq!(inside.wall_slots.unwrap().outside, None);

    let outside = Fixture::new(None, Some(OUTSIDE)).decode().unwrap();
    assert_eq!(outside.thickness.get(), 0.5);
    assert_eq!(outside.thickness_record_index, OUTSIDE);
    assert!(outside.outward);
    assert_eq!(outside.outward_offset, 24);
    assert_eq!(outside.wall_slots.unwrap().outside, None);

    let both = Fixture::new(Some(INSIDE), Some(OUTSIDE)).decode().unwrap();
    assert_eq!(both.thickness.get(), 0.25);
    assert_eq!(both.thickness_record_index, INSIDE);
    assert!(!both.outward);
    let wall = both.wall_slots.unwrap().outside.unwrap();
    assert_eq!(wall.thickness.get(), 0.5);
    assert_eq!(wall.thickness_record_index, OUTSIDE);
}

#[test]
fn unequal_decoded_wall_slots_project_without_a_symmetric_thickness() {
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
    };
    use crate::records::topology::construction::DesignConstructionOperandGroup;
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, ShellJoin, ShellMode,
    };

    let operation = Fixture::new(Some(INSIDE), Some(OUTSIDE)).decode().unwrap();
    assert_eq!(operation.thickness.get(), 0.25);
    assert_eq!(
        operation
            .wall_slots
            .as_ref()
            .unwrap()
            .outside
            .as_ref()
            .unwrap()
            .thickness
            .get(),
        0.5
    );
    let mut scope = DesignParameterScope::empty("f3d:test:scope#90", DesignFeatureKind::Shell, 90);
    let DesignScopePayloadMut::Shell(slot) = scope.payload_mut() else {
        panic!("expected a Shell scope");
    };
    *slot = Some(operation);
    let group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:group#91",
        "scope_record_index": 90,
        "scope_reference_ordinal": 0,
        "record_index": 91,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [92],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": 68719476736_u64,
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    }))
    .unwrap();
    let Some(FeatureDefinition::Operation(FeatureOperation::Shell {
        thickness,
        outward,
        mode,
        removed_faces,
        join,
        ..
    })) = crate::design::feature_project::project_shell(&scope, &[], std::slice::from_ref(&group))
        .unwrap()
    else {
        panic!("expected a neutral Shell");
    };
    assert_eq!(thickness, None);
    assert_eq!(outward, None);
    assert_eq!(mode, Some(ShellMode::BothSides));
    assert_eq!(
        removed_faces,
        FaceSelection::Native("f3d:test:group#91".into())
    );
    assert_eq!(join, Some(ShellJoin::Intersection));
}

#[test]
fn the_form_has_no_class_requirement() {
    for class_tag in [*b"262", *b"369", *b"412"] {
        let mut fixture = Fixture::new(Some(INSIDE), None);
        fixture.class_tag = class_tag;
        assert_eq!(fixture.decode().unwrap().thickness_record_index, INSIDE);
    }
}

#[test]
fn the_table_length_follows_the_member_count() {
    let mut fixture = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    fixture.table = vec![GROUP, MEMBER, 95, INSIDE, OUTSIDE];
    assert!(fixture
        .decode()
        .unwrap()
        .wall_slots
        .unwrap()
        .outside
        .is_some());
}

#[test]
fn flags_read_tangent_chain_and_rounded_corners_apart() {
    for (tangent_chain, rounded_corners) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let mut fixture = Fixture::new(Some(INSIDE), None);
        fixture.tangent_chain = tangent_chain;
        fixture.rounded_corners = rounded_corners;
        let slots = fixture.decode().unwrap().wall_slots.unwrap();
        assert_eq!(slots.tangent_chain, tangent_chain == 1);
        assert_eq!(slots.rounded_corners, rounded_corners == 1);
    }
    let mut invalid = Fixture::new(Some(INSIDE), None);
    invalid.tangent_chain = 2;
    assert_eq!(invalid.decode(), None);
    let mut invalid = Fixture::new(Some(INSIDE), None);
    invalid.rounded_corners = 2;
    assert_eq!(invalid.decode(), None);
}

#[test]
fn a_frame_outside_the_grammar_is_not_this_form() {
    let mut no_wall = Fixture::new(None, None);
    no_wall.table = vec![GROUP, MEMBER, INSIDE];
    assert_eq!(no_wall.decode(), None);

    let mut invalid_marker = Fixture::new(Some(INSIDE), None);
    invalid_marker.corrupt = Some((23, 2));
    assert_eq!(invalid_marker.decode(), None);

    for offset in [11, 20] {
        let mut nonzero = Fixture::new(Some(INSIDE), None);
        nonzero.corrupt = Some((offset, 1));
        assert_eq!(nonzero.decode(), None, "offset {offset}");
    }

    let mut nonzero_after_slots = Fixture::new(Some(INSIDE), None);
    nonzero_after_slots.corrupt = Some((35, 1));
    assert_eq!(nonzero_after_slots.decode(), None);

    let mut group_count = Fixture::new(Some(INSIDE), None);
    group_count.corrupt = Some((47, 2));
    assert_eq!(group_count.decode(), None);

    let mut other_group = Fixture::new(Some(INSIDE), None);
    other_group.group_reference = MEMBER;
    assert_eq!(other_group.decode(), None);

    let mut gap = Fixture::new(Some(INSIDE), None);
    gap.gap_before_table = 1;
    assert_eq!(gap.decode(), None);
}

#[test]
fn the_table_must_end_with_every_wall_in_slot_order() {
    let mut swapped = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    swapped.table = vec![GROUP, MEMBER, OUTSIDE, INSIDE];
    assert_eq!(swapped.decode(), None);

    let mut one_listed = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    one_listed.table = vec![GROUP, MEMBER, 95, INSIDE];
    assert_eq!(one_listed.decode(), None);

    let mut no_selection = Fixture::new(Some(INSIDE), None);
    no_selection.table = vec![GROUP, INSIDE];
    assert_eq!(no_selection.decode(), None);
}

#[test]
fn every_present_wall_needs_a_positive_scalar() {
    let mut missing_inside = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    missing_inside.scalars = vec![(OUTSIDE, 0.5)];
    assert_eq!(missing_inside.decode(), None);

    let mut missing_outside = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    missing_outside.scalars = vec![(INSIDE, 0.25)];
    assert_eq!(missing_outside.decode(), None);

    let mut zero_outside = Fixture::new(Some(INSIDE), Some(OUTSIDE));
    zero_outside.scalars = vec![(INSIDE, 0.25), (OUTSIDE, 0.0)];
    assert_eq!(zero_outside.decode(), None);

    let mut negative_inside = Fixture::new(Some(INSIDE), None);
    negative_inside.scalars = vec![(INSIDE, -0.25)];
    assert_eq!(negative_inside.decode(), None);
}

fn prefixed(prefix: Prefix, inside: Option<u32>, outside: Option<u32>) -> Fixture {
    let mut fixture = Fixture::new(inside, outside);
    fixture.prefix = prefix;
    fixture
}

#[test]
fn each_prefix_starts_its_slots_at_its_offset() {
    for (prefix, slots_at) in [
        (Prefix::Compact, 22),
        (Prefix::Marked, 26),
        (Prefix::Counted(Vec::new()), 27),
    ] {
        let inside = prefixed(prefix.clone(), Some(INSIDE), None)
            .decode()
            .unwrap();
        assert_eq!(inside.thickness.get(), 0.25);
        assert_eq!(inside.thickness_record_index, INSIDE);
        assert!(!inside.outward);
        assert_eq!(inside.outward_offset, slots_at);
        assert_eq!(inside.wall_slots, None);
        assert!(inside.auxiliary_references.is_empty());

        let outside = prefixed(prefix, None, Some(OUTSIDE)).decode().unwrap();
        assert_eq!(outside.thickness.get(), 0.5);
        assert_eq!(outside.thickness_record_index, OUTSIDE);
        assert!(outside.outward);
        assert_eq!(outside.outward_offset, slots_at + 1);
    }
}

#[test]
fn the_offset_21_boolean_leaves_the_wall_direction_to_the_slot() {
    for prefix in [Prefix::Compact, Prefix::Marked, Prefix::Counted(Vec::new())] {
        for flag in [0, 1] {
            let mut fixture = prefixed(prefix.clone(), Some(INSIDE), None);
            fixture.flag = flag;
            assert!(!fixture.decode().unwrap().outward);
        }
        let mut invalid = prefixed(prefix, Some(INSIDE), None);
        invalid.flag = 2;
        assert_eq!(invalid.decode(), None);
    }
}

#[test]
fn the_offset_21_boolean_is_stored_and_reported() {
    for (prefix, name) in [
        (Prefix::Compact, "compact"),
        (Prefix::Marked, "marked"),
        (Prefix::Counted(Vec::new()), "counted"),
    ] {
        let mut stored = Vec::new();
        for flag in [0, 1] {
            let mut fixture = prefixed(prefix.clone(), Some(INSIDE), None);
            fixture.flag = flag;
            let (operation, losses) = fixture.decode_with_losses();
            let operation = operation.unwrap();
            assert_eq!(
                operation.prefix_flag,
                Some(DesignShellPrefixFlag {
                    value: flag == 1,
                    offset: 21,
                })
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                crate::loss::F3dLossCode::FeatureShellPrefixFlagAssumed.kind()
            );
            assert!(
                losses[0].message.contains(&format!(
                    "Boolean {flag} at byte 21 (primary-header offset 21 of its {name} prefix)"
                )),
                "{}",
                losses[0].message
            );
            assert!(losses[0].message.contains("DR-315"));
            stored.push(operation);
        }
        assert_ne!(stored[0], stored[1]);

        let mut placed = prefixed(prefix, Some(INSIDE), None);
        placed.flag = 1;
        placed.lead = 48_192;
        let (operation, losses) = placed.decode_with_losses();
        assert_eq!(operation.unwrap().prefix_flag.unwrap().offset, 48_213);
        assert!(
            losses[0].message.contains(&format!(
                "Boolean 1 at byte 48213 (primary-header offset 21 of its {name} prefix)"
            )),
            "{}",
            losses[0].message
        );
    }

    let (cornered, losses) = Fixture::new(Some(INSIDE), None).decode_with_losses();
    assert_eq!(cornered.unwrap().prefix_flag, None);
    assert!(losses.is_empty());
}

#[test]
fn the_counted_prefix_keeps_its_auxiliary_references() {
    let fixture = prefixed(Prefix::Counted(vec![95, 96]), Some(INSIDE), None);
    let operation = fixture.decode().unwrap();
    assert_eq!(operation.auxiliary_references, vec![95, 96]);
    assert_eq!(operation.thickness_record_index, INSIDE);
    assert_eq!(operation.outward_offset, 27);

    let mut unmarked = prefixed(Prefix::Counted(vec![95]), Some(INSIDE), None);
    unmarked.corrupt = Some((47, 2));
    assert_eq!(unmarked.decode(), None);

    let mut trailing = prefixed(Prefix::Counted(vec![95]), Some(INSIDE), None);
    trailing.corrupt = Some((58, 1));
    assert_eq!(trailing.decode(), None);
}

#[test]
fn the_wall_scalar_can_be_any_ordered_reference_but_the_group() {
    for table in [
        vec![INSIDE, GROUP, MEMBER],
        vec![GROUP, MEMBER, INSIDE],
        vec![GROUP, MEMBER, 95, INSIDE],
    ] {
        let mut fixture = prefixed(Prefix::Marked, Some(INSIDE), None);
        fixture.table = table;
        assert_eq!(fixture.decode().unwrap().thickness_record_index, INSIDE);
    }

    let mut unlisted_wall = prefixed(Prefix::Marked, Some(INSIDE), None);
    unlisted_wall.table = vec![GROUP, MEMBER, 95];
    assert_eq!(unlisted_wall.decode(), None);

    let mut unlisted_group = prefixed(Prefix::Marked, Some(INSIDE), None);
    unlisted_group.table = vec![95, MEMBER, INSIDE];
    assert_eq!(unlisted_group.decode(), None);

    let mut two_references = prefixed(Prefix::Marked, Some(INSIDE), None);
    two_references.table = vec![GROUP, INSIDE];
    assert_eq!(two_references.decode(), None);

    let mut wall_is_group = prefixed(Prefix::Marked, Some(INSIDE), None);
    wall_is_group.group_reference = INSIDE;
    assert_eq!(wall_is_group.decode(), None);
}

#[test]
fn a_prefix_without_the_cornered_flags_stores_one_wall() {
    for prefix in [Prefix::Compact, Prefix::Marked, Prefix::Counted(Vec::new())] {
        let both = prefixed(prefix.clone(), Some(INSIDE), Some(OUTSIDE));
        assert_eq!(both.decode(), None);
        let mut none = prefixed(prefix, None, None);
        none.table = vec![GROUP, MEMBER, INSIDE];
        assert_eq!(none.decode(), None);
    }
}

#[test]
fn the_marked_prefix_constants_admit_the_form() {
    for class_tag in [*b"369", *b"396", *b"430"] {
        let mut fixture = prefixed(Prefix::Marked, Some(INSIDE), None);
        fixture.class_tag = class_tag;
        assert_eq!(fixture.decode().unwrap().thickness_record_index, INSIDE);
    }
    for (offset, value) in [(11, 1), (20, 0), (22, 1), (25, 0), (37, 1)] {
        let mut fixture = prefixed(Prefix::Marked, Some(INSIDE), None);
        fixture.corrupt = Some((offset, value));
        assert_eq!(fixture.decode(), None, "offset {offset}");
    }
    let mut counted_separator = prefixed(Prefix::Counted(Vec::new()), Some(INSIDE), None);
    counted_separator.corrupt = Some((26, 2));
    assert_eq!(counted_separator.decode(), None);
}
