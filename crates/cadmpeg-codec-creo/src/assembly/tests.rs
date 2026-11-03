// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::View;

use super::{frame_scalar, frame_slots, member_table, MemberKind};
use crate::test_support::members_table;

fn scalar(bytes: &[u8]) -> Option<(f64, usize)> {
    let mut view = View::over_retained(bytes);
    let head = view.u8()?;
    let value = frame_scalar(head, &mut view)?;
    Some((value, view.position()))
}

fn bits(bytes: &[u8]) -> Option<(u64, usize)> {
    scalar(bytes).map(|(value, end)| (value.to_bits(), end))
}

#[test]
fn single_byte_constants() {
    assert_eq!(scalar(&[0x0f]), Some((1.0, 1)));
    assert_eq!(scalar(&[0x10]), Some((-1.0, 1)));
    assert_eq!(scalar(&[0x18]), Some((0.0, 1)));
    assert_eq!(bits(&[0x04]), Some((0x3fef_ffff_ffff_ffff, 1)));
}

#[test]
fn constant_forms_replace_trailing_bytes() {
    assert_eq!(bits(&[0x00, 0x10, 0x02]), Some((0xbff0_0000_0000_0002, 3)));
    assert_eq!(
        bits(&[0x01, 0x0f, 0x12, 0x34]),
        Some((0x3ff0_0000_0000_1234, 4))
    );
    assert_eq!(
        bits(&[0x02, 0x04, 0xb8, 0x1d, 0xc2]),
        Some((0x3fef_ffff_ffb8_1dc2, 5))
    );
    assert_eq!(scalar(&[0x00, 0x18, 0x02]), None);
    assert_eq!(scalar(&[0x03, 0x0f, 0, 0, 0, 0]), None);
}

#[test]
fn grouped_lattice_selects_lead_byte_and_form() {
    assert_eq!(
        bits(&[0x19, 0x91, 0xa6, 0x26, 0x33, 0x14, 0x5c, 0x07]),
        Some((0x3c91_a626_3314_5c07, 8))
    );
    assert_eq!(scalar(&[0x2d, 0x75, 0x40, 0, 0, 0, 0, 0]), Some((340.0, 8)));
    assert_eq!(bits(&[0x42, 0xef, 0xff]), Some((0xbfef_ffff_ffff_ffff, 3)));
    assert_eq!(scalar(&[0x2f, 0x71, 0x80]), Some((280.0, 3)));
    assert_eq!(scalar(&[0x48, 0x76, 0x30]), Some((-355.0, 3)));
    assert_eq!(scalar(&[0x31, 0x59, 0, 0, 0, 0, 0]), Some((100.0, 7)));
    assert_eq!(scalar(&[0x2b, 0x59, 0, 0, 0, 0, 0, 0]), None);
}

#[test]
fn dict_prefixes_select_two_lead_bytes() {
    assert_eq!(
        bits(&[0x7a, 0xff, 0xff, 0xf3, 0xe4, 0xcb, 0x17]),
        Some((0x3fef_ffff_f3e4_cb17, 7))
    );
    assert_eq!(
        bits(&[0xb9, 0xa0, 0x9e, 0x66, 0x4c, 0x72, 0x2c]),
        Some((0xbfe6_a09e_664c_722c, 7))
    );
    assert_eq!(scalar(&[0x8f, 0, 0, 0, 0, 0, 0]), Some((2.5, 7)));
    assert_eq!(scalar(&[0xa4, 0, 0, 0, 0, 0, 0]), None);
}

#[test]
fn explicit_lead_byte_forms() {
    assert_eq!(
        bits(&[0xea, 0x39, 0x44, 0x00]),
        Some((0x3944_0000_0000_0000, 4))
    );
    assert_eq!(
        bits(&[0xec, 0x39, 0x33, 0x77, 0xce, 0x85, 0x8a, 0x80]),
        Some((0x3933_77ce_858a_8000, 8))
    );
    assert_eq!(
        bits(&[0xed, 0x39, 0x53, 0x77, 0xce, 0x85, 0x8a, 0x5d, 0x48]),
        Some((0x3953_77ce_858a_5d48, 9))
    );
    assert_eq!(
        bits(&[0xee, 0xb9, 0x3f, 0xff]),
        Some((0xb93f_ffff_ffff_ffff, 4))
    );
    assert_eq!(scalar(&[0xee, 0xb9, 0x3f, 0x00]), None);
}

#[test]
fn non_finite_values_are_refused() {
    assert_eq!(scalar(&[0xed, 0x7f, 0xf0, 0, 0, 0, 0, 0, 0]), None);
}

#[test]
fn repeats_copy_the_preceding_slot() {
    let mut slots = [f64::NAN; 3];
    let mut view = View::over_retained(&[0x0f, 0x18, 0xe4]);
    assert_eq!(frame_slots(&mut view, &mut slots), Some(()));
    assert_eq!(slots, [1.0, 0.0, 0.0]);

    let mut slots = [f64::NAN; 3];
    let mut view = View::over_retained(&[0x10, 0xe5]);
    assert_eq!(frame_slots(&mut view, &mut slots), Some(()));
    assert_eq!(slots, [-1.0; 3]);
}

#[test]
fn a_repeat_needs_a_preceding_slot_and_room() {
    let mut slots = [0.0; 3];
    assert_eq!(
        frame_slots(&mut View::over_retained(&[0xe4, 0x0f, 0x0f]), &mut slots),
        None
    );
    assert_eq!(
        frame_slots(&mut View::over_retained(&[0x0f, 0x18, 0xe5]), &mut slots),
        None
    );
}

#[test]
fn table_rows_decode_in_source_order() {
    let table = member_table(&members_table()).expect("member table");
    assert_eq!(table.offset, 0);
    assert_eq!(table.declared, 3);
    assert_eq!(table.stop, None);
    let rows = table
        .members
        .iter()
        .map(|member| (member.id, member.kind, member.name.as_str(), member.frame))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            (
                0x2a,
                MemberKind::Assembly,
                "SUBASM",
                [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]
            ),
            (
                0x0a0f,
                MemberKind::Part,
                "BRACKET",
                [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 340.0, 408.0, 0.0]
            ),
            (
                0x0a10,
                MemberKind::Part,
                "BRACKET",
                [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, -355.0, 2.5, 619.0]
            ),
        ]
    );
    assert!(table
        .members
        .iter()
        .all(|member| member.family_name == member.name));
}

#[test]
fn row_offsets_move_with_the_table() {
    let mut payload = b"prefix".to_vec();
    payload.extend(members_table());
    let table = member_table(&payload)
        .expect("member table")
        .at(100)
        .expect("offsets");
    assert_eq!(table.offset, 106);
    assert_eq!(
        table.members.first().map(|member| member.offset),
        Some(106 + b"\xe0\x00members\0\xf1\xf8\x03\xf7\x44\xfb\xe3".len())
    );
}

#[test]
fn a_declared_count_that_differs_stops_the_table() {
    let mut payload = members_table();
    payload[b"\xe0\x00members\0\xf1\xf8".len()] = 0x04;
    let table = member_table(&payload).expect("member table");
    assert_eq!(table.members.len(), 3);
    assert_eq!(
        table.stop.map(|stop| stop.reason),
        Some("the table ends with a row count that differs from its declared count")
    );
}

#[test]
fn an_unadmitted_row_keeps_the_rows_before_it() {
    let payload = members_table();
    let third_row = payload
        .windows(2)
        .position(|window| window == [0x8a, 0x10])
        .expect("third row");
    let mut damaged = payload.clone();
    let tha_ref = third_row + 2 + b"\x02BRACKET\0BRACKET\0".len();
    assert_eq!(damaged[tha_ref], 0xe1);
    damaged[tha_ref] = 0xf6;
    let table = member_table(&damaged).expect("member table");
    assert_eq!(table.members.len(), 2);
    assert_eq!(
        table.stop.map(|stop| (stop.offset, stop.reason)),
        Some((
            third_row,
            "a positional row does not match the component row grammar"
        ))
    );
}

#[test]
fn a_payload_without_one_members_field_has_no_table() {
    assert_eq!(member_table(b"no table"), None);
    let mut twice = members_table();
    twice.extend(members_table());
    assert_eq!(member_table(&twice), None);
}

#[test]
fn row_kinds_select_model_extensions() {
    assert_eq!(MemberKind::from_byte(1).extension(), Some("asm"));
    assert_eq!(MemberKind::from_byte(2).extension(), Some("prt"));
    assert_eq!(MemberKind::from_byte(3), MemberKind::Other(3));
    assert_eq!(MemberKind::from_byte(3).extension(), None);
}
