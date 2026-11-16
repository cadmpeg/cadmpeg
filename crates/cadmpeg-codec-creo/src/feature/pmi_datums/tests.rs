// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::{datum_tags, DatumAttachment, DatumTagIdentity, DatumTagOutcome};

fn field(name: &[u8], value: &[u8]) -> Vec<u8> {
    let mut field = Vec::with_capacity(name.len() + value.len() + 2);
    field.extend_from_slice(b"\xe0");
    field.extend_from_slice(name);
    field.push(0);
    field.extend_from_slice(value);
    field
}

fn array(name: &[u8], count: u8, class_id: u8, nulls: usize, prototype: &[u8]) -> Vec<u8> {
    let mut bytes = field(name, &[0xf2, 0xf8, count, 0xf7, class_id, 0xfb]);
    bytes.extend(std::iter::repeat_n(0xe1, nulls));
    bytes.extend_from_slice(prototype);
    bytes
}

fn definition_array(count: u8, nulls: usize, attachment: Option<(u8, u8)>) -> Vec<u8> {
    let mut bytes = array(
            b"\x00gtol_datum_def_xar",
            count,
            10,
            nulls,
            b"\xe3\xe0\x01datum_type\0\x01\xe0\x01datum_id\0\xf6\xe0\x01feat_id\0\xf6\xe0\x01gtol_id\0\xf6\xe0\x00p_ldr\0\xe3\xe0\x01ldr_attrib\0\x00",
        );
    if let Some((reference_type, id)) = attachment {
        bytes.extend_from_slice(b"\xe0\x00note_att\0\xe3\xe0\x01type\0");
        bytes.push(reference_type);
        bytes.extend_from_slice(b"\xe0\x01attach_id\0");
        bytes.push(id);
    }
    bytes.extend_from_slice(
            b"\xe0\x00att_offsets\0\xf8\x00\xf1\xf7\x14\xe3\xe0\x00p_tgt_info\0\xe1\xe0\x01disp_dtm_geom\0\x01\xf1\xf7\x0a\xe3",
        );
    for _ in nulls..usize::from(count).saturating_sub(1) {
        bytes.extend_from_slice(b"\x01\xf6\xf6\xf6\xe3");
    }
    bytes
}

fn tag_array(
    count: u8,
    nulls: usize,
    definition_id: Option<u8>,
    label: &[u8],
    include_positionals: bool,
) -> Vec<u8> {
    let mut bytes = array(
        b"\x00set_datum_tag_xar",
        count,
        30,
        nulls,
        &tag_prototype(definition_id, label),
    );
    if include_positionals {
        bytes.extend_from_slice(&positional_tag(14, b"B", true));
        bytes.extend_from_slice(&positional_tag(15, b"C", false));
    }
    bytes
}

fn tag_prototype(definition_id: Option<u8>, label: &[u8]) -> Vec<u8> {
    let mut tag = b"\xe3\xe0\x09attributes\0\x01\xe0\x01dtm_def_id\0".to_vec();
    tag.push(definition_id.unwrap_or(0xf6));
    tag.extend_from_slice(b"\xe0\x00p_sld_info\0\xe3\xe0\x01ref_type\0\x01\xe0\x01ref_id\0\x01\xe0\x00p_cosm\0\xf1\xe3\xe0\x09attributes\0\x01\xe0\x00gen_info\0\xe2\xe0\x00p_text\0\xf1\xe3\xe0\x01db_type\0\x3e\xe0\x01text_type\0\x12\xe0\x0atext_value\0X\0\xe0\x00p_addl_text\0\xf1\xe3\xe0\x00p_attach\0\xf1\xe1\xe0\x0ap_datum_tag_name\0");
    tag.extend_from_slice(label);
    tag.extend_from_slice(b"\0\xf1\xf7\x1e\xe3");
    tag
}

fn positional_tag(definition_id: u8, label: &[u8], first: bool) -> Vec<u8> {
    positional_tag_with_prefix(&[0xa0, 0x01, definition_id], label, first)
}

fn positional_tag_with_prefix(prefix: &[u8], label: &[u8], first: bool) -> Vec<u8> {
    let mut tag = Vec::new();
    if !first {
        tag.push(0xe3);
    }
    tag.extend_from_slice(prefix);
    tag.extend_from_slice(b"\xe3\x80\xdb");
    tag.extend_from_slice(b"\xe3\x3e\x12");
    tag.extend_from_slice(label);
    tag.push(0);
    tag.extend_from_slice(b"\xe3\x3e\x12\0");
    tag
}

fn full_fixture(definition_id: Option<u8>, label: &[u8], attachment: Option<(u8, u8)>) -> Vec<u8> {
    let mut payload = definition_array(16, 13, attachment);
    payload.extend_from_slice(&tag_array(16, 13, definition_id, label, true));
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));
    payload
}

fn decode(payload: &[u8]) -> DatumTagOutcome {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("test input fits decode limits");
    datum_tags(&ctx, root).expect("datum array allocation fits decode limits")
}

fn decoded(payload: &[u8]) -> (Vec<DatumTagIdentity>, usize) {
    match decode(payload) {
        DatumTagOutcome::Decoded {
            tags,
            unresolved_populated_slots,
            ..
        } => (tags, unresolved_populated_slots),
        outcome => panic!("expected decoded datum tags, got {outcome:?}"),
    }
}

#[test]
fn joins_step_datum_labels_to_definition_slots_and_keeps_a_typed_plane_reference() {
    let payload = full_fixture(Some(13), b"A", Some((0x25, 106)));
    let a_source_offset = payload
        .windows(b"\xe0\x0ap_datum_tag_name\0".len())
        .position(|window| window == b"\xe0\x0ap_datum_tag_name\0")
        .expect("datum label field");
    let b_source_offset = payload
        .windows(b"\xe3\x3e\x12B\0".len())
        .position(|window| window == b"\xe3\x3e\x12B\0")
        .expect("datum B text value");
    let (tags, unresolved) = decoded(&payload);
    assert_eq!(unresolved, 0);
    assert_eq!(
        tags.iter()
            .map(|tag| (
                tag.identification.as_str(),
                tag.definition_id,
                tag.definition_slot
            ))
            .collect::<Vec<_>>(),
        [
            ("A", Some(13), Some(13)),
            ("B", Some(14), Some(14)),
            ("C", Some(15), Some(15))
        ]
    );
    assert_eq!(tags[0].source_offset, a_source_offset);
    assert_eq!(
        tags[0].attachment,
        Some(DatumAttachment {
            reference_type: 0x25,
            id: 106
        })
    );
    assert_eq!(tags[1].source_offset, b_source_offset);
    assert_eq!(tags[1].attachment, None);
    assert_eq!(tags[2].attachment, None);
}

#[test]
fn reports_absent_and_null_arrays_without_dropped_tag_losses() {
    assert_eq!(
        decode(b"no datum tag array"),
        DatumTagOutcome::AbsentOrEmpty
    );

    let empty = [
        array(b"\x00set_datum_tag_xar", 0, 30, 0, &[]).as_slice(),
        field(b"\x00drf_data_xar", &[0xe1]).as_slice(),
    ]
    .concat();
    assert_eq!(decode(&empty), DatumTagOutcome::AbsentOrEmpty);

    let nulls = [
        array(b"\x00set_datum_tag_xar", 3, 30, 3, &[]).as_slice(),
        field(b"\x00drf_data_xar", &[0xe1]).as_slice(),
    ]
    .concat();
    assert_eq!(decode(&nulls), DatumTagOutcome::AbsentOrEmpty);
}

#[test]
fn retains_labels_when_a_definition_slot_join_is_unresolved() {
    let payload = full_fixture(None, b"A", Some((0x25, 106)));
    assert!(decode(&payload).label_roster_complete());
    let (tags, unresolved) = decoded(&payload);
    assert_eq!(tags[0].identification, "A");
    assert_eq!(tags[0].definition_id, None);
    assert_eq!(tags[0].definition_slot, None);
    assert_eq!(tags[0].attachment, None);
    assert_eq!(tags[1].definition_slot, Some(14));
    assert_eq!(unresolved, 1);

    let (tags, unresolved) = decoded(&full_fixture(Some(14), b"A", Some((0x25, 106))));
    assert_eq!(tags[0].identification, "A");
    assert_eq!(tags[0].definition_slot, None);
    assert_eq!(tags[0].attachment, None);
    assert_eq!(unresolved, 1);
}

#[test]
fn refuses_duplicate_array_owners() {
    let item = full_fixture(Some(13), b"A", Some((0x25, 106)));
    let payload = [item.as_slice(), item.as_slice()].concat();
    assert_eq!(
        decode(&payload),
        DatumTagOutcome::Unresolved {
            populated_slots: None
        }
    );
}

#[test]
fn requires_the_prototype_at_the_first_occupied_tag_slot() {
    let mut payload = definition_array(16, 13, Some((0x25, 106)));
    let mut tag = array(
        b"\x00set_datum_tag_xar",
        16,
        30,
        13,
        b"\xe3\xe0\x01unknown\0\x01\xf1\xf7\x1e\xe3",
    );
    tag.extend_from_slice(&tag_prototype(Some(13), b"A"));
    tag.extend_from_slice(&positional_tag(14, b"B", true));
    tag.extend_from_slice(&positional_tag(15, b"C", false));
    payload.extend_from_slice(&tag);
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));
    assert_eq!(
        decode(&payload),
        DatumTagOutcome::Unresolved {
            populated_slots: Some(3)
        }
    );
}

#[test]
fn does_not_read_a_tag_name_after_the_first_prototype_row() {
    let mut payload = definition_array(16, 13, Some((0x25, 106)));
    let mut tag = array(
            b"\x00set_datum_tag_xar",
            16,
            30,
            13,
            b"\xe3\xe0\x09attributes\0\x01\xe0\x01dtm_def_id\0\x0d\xe0\x00p_sld_info\0\xe3\xf1\xf7\x1e\xe3",
        );
    tag.extend_from_slice(b"\xe0\x0ap_datum_tag_name\0C\0\xf1\xf7\x1e\xe3");
    payload.extend_from_slice(&tag);
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));
    assert_eq!(
        decode(&payload),
        DatumTagOutcome::Unresolved {
            populated_slots: Some(3)
        }
    );
}

#[test]
fn does_not_read_a_leader_attachment_after_the_definition_row() {
    let mut payload = definition_array(16, 13, None);
    payload.extend_from_slice(
        b"\xe0\x00p_ldr\0\xe3\xe0\x00note_att\0\xe3\xe0\x01type\0%\xe0\x01attach_id\0j",
    );
    payload.extend_from_slice(&tag_array(16, 13, Some(13), b"A", true));
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));
    let (tags, _) = decoded(&payload);
    assert_eq!(tags[0].identification, "A");
    assert_eq!(tags[0].definition_slot, Some(13));
    assert_eq!(tags[0].attachment, None);
}

#[test]
fn keeps_the_first_tag_and_counts_rows_with_unsupported_positional_framing() {
    let mut payload = definition_array(16, 13, Some((0x25, 106)));
    let mut tags = tag_array(16, 13, Some(13), b"A", false);
    tags.extend_from_slice(&positional_tag(14, b"B", true));
    payload.extend_from_slice(&tags);
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));
    let (tags, unresolved) = decoded(&payload);
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].identification, "A");
    assert_eq!(unresolved, 2);
}

#[test]
fn preserves_absolute_view_offsets_while_parsing_a_bounded_section() {
    let prefix = b"source-prefix";
    let payload = full_fixture(Some(13), b"A", Some((0x25, 106)));
    let expected_offset = prefix.len()
        + payload
            .windows(b"\xe0\x0ap_datum_tag_name\0".len())
            .position(|window| window == b"\xe0\x0ap_datum_tag_name\0")
            .expect("datum label field");
    let mut source = prefix.to_vec();
    source.extend_from_slice(&payload);
    source.extend_from_slice(b"\xe3\x80\xdb\xe3\x3e\x12D\0\xe3\x3e\x12\0");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&source, &arena, &policy)
        .expect("test input fits decode limits");
    let view = root
        .child(prefix.len(), prefix.len() + payload.len())
        .expect("section view");
    let (tags, unresolved) = match datum_tags(&ctx, view).expect("datum rows allocate") {
        DatumTagOutcome::Decoded {
            tags,
            unresolved_populated_slots,
            ..
        } => (tags, unresolved_populated_slots),
        outcome => panic!("expected bounded datum tags, got {outcome:?}"),
    };
    assert_eq!(tags[0].identification, "A");
    assert_eq!(tags[0].source_offset, expected_offset);
    assert_eq!(tags.len(), 3);
    assert_eq!(unresolved, 0);
}

#[test]
fn ignores_typed_leader_and_label_markers_after_the_selected_array() {
    let mut payload = definition_array(16, 13, Some((0x25, 106)));
    payload.extend_from_slice(&tag_array(16, 13, Some(13), b"A", false));
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));
    payload.extend_from_slice(&positional_tag(14, b"B", true));
    let (tags, unresolved) = decoded(&payload);
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].identification, "A");
    assert_eq!(unresolved, 2);
}

#[test]
fn keeps_repeated_labels_as_separate_definition_slot_identities() {
    let mut payload = definition_array(16, 13, Some((0x25, 106)));
    let mut tags = tag_array(16, 13, Some(13), b"A", false);
    tags.extend_from_slice(&positional_tag(14, b"A", true));
    tags.extend_from_slice(&positional_tag(15, b"C", false));
    payload.extend_from_slice(&tags);
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));
    let (tags, unresolved) = decoded(&payload);
    assert_eq!(
        tags.iter()
            .map(|tag| tag.identification.as_str())
            .collect::<Vec<_>>(),
        ["A", "A", "C"]
    );
    assert_eq!(
        tags.iter()
            .map(|tag| tag.definition_slot)
            .collect::<Vec<_>>(),
        [Some(13), Some(14), Some(15)]
    );
    assert_eq!(unresolved, 0);
}

#[test]
fn reads_two_byte_attributes_and_definition_ids_before_slave_info() {
    let mut payload = definition_array(16, 13, Some((0x25, 106)));
    let mut tags = tag_array(16, 13, Some(13), b"A", false);
    tags.extend_from_slice(&positional_tag_with_prefix(
        &[0xa0, 0x01, 0x81, 0x25],
        b"B",
        true,
    ));
    tags.extend_from_slice(&positional_tag(15, b"C", false));
    payload.extend_from_slice(&tags);
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));

    let (tags, unresolved) = decoded(&payload);
    assert_eq!(tags[1].identification, "B");
    assert_eq!(tags[1].definition_id, Some(0x125));
    assert_eq!(tags[1].definition_slot, None);
    assert_eq!(tags[2].identification, "C");
    assert_eq!(tags[2].definition_id, Some(15));
    assert_eq!(tags[2].definition_slot, Some(15));
    assert_eq!(unresolved, 1);
}

#[test]
fn retains_labels_when_a_control_prefix_precedes_a_candidate_id() {
    let mut payload = definition_array(16, 13, Some((0x25, 106)));
    let mut tags = tag_array(16, 13, Some(13), b"A", false);
    tags.extend_from_slice(&positional_tag_with_prefix(&[0xa0, 0xf0, 0x25], b"B", true));
    tags.extend_from_slice(&positional_tag(15, b"C", false));
    payload.extend_from_slice(&tags);
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));

    let (tags, unresolved) = decoded(&payload);
    assert_eq!(tags[1].identification, "B");
    assert_eq!(tags[1].definition_id, None);
    assert_eq!(tags[1].definition_slot, None);
    assert_eq!(tags[2].definition_slot, Some(15));
    assert_eq!(unresolved, 1);
}

#[test]
fn does_not_label_join_when_an_unparsed_row_could_repeat_a_known_label() {
    let mut payload = definition_array(16, 13, Some((0x25, 106)));
    let mut tags = tag_array(16, 13, Some(13), b"A", false);
    let mut broken_b = positional_tag(14, b"B", true);
    broken_b.truncate(broken_b.len() - b"\xe3\x3e\x12\0".len());
    tags.extend_from_slice(&broken_b);
    tags.extend_from_slice(&positional_tag(15, b"A", false));
    payload.extend_from_slice(&tags);
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));

    let outcome = decode(&payload);
    assert!(!outcome.label_roster_complete());
    let DatumTagOutcome::Decoded {
        tags,
        unparsed_label_slots,
        ..
    } = outcome
    else {
        panic!("expected a decoded first datum label");
    };
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].identification, "A");
    assert_eq!(unparsed_label_slots, 2);
}

/// A definition array whose later rows replay `p_ldr.note_att` surface leaders.
fn definition_array_with_leaders(count: u8, nulls: usize, leaders: &[u8]) -> Vec<u8> {
    let mut bytes = array(
            b"\x00gtol_datum_def_xar",
            count,
            10,
            nulls,
            b"\xe3\xe0\x01datum_type\0\x01\xe0\x01datum_id\0\xf6\xe0\x01feat_id\0\xf6\xe0\x01gtol_id\0\xf6\xe0\x00p_ldr\0\xe3\xe0\x01ldr_attrib\0\x00\xe0\x00note_att\0\xe3\xe0\x01type\0\x25\xe0\x01attach_id\0\x6a",
        );
    bytes.extend_from_slice(
            b"\xe0\x00att_offsets\0\xf8\x00\xf1\xf7\x14\xe3\xe0\x00p_tgt_info\0\xe1\xe0\x01disp_dtm_geom\0\x01\xf1\xf7\x0a\xe3",
        );
    for (index, id) in leaders.iter().enumerate() {
        if index > 0 {
            bytes.push(0xe3);
        }
        bytes.extend_from_slice(b"\x01\xf6\xf6\xf6\xe3\x00\xe3\x25");
        bytes.push(*id);
        bytes.extend_from_slice(b"\xf8\x00\xe4\x00\xe1\x01");
    }
    bytes
}

fn leader_fixture(leaders: &[u8]) -> Vec<u8> {
    let mut payload = definition_array_with_leaders(16, 13, leaders);
    payload.extend_from_slice(&tag_array(16, 13, Some(13), b"A", true));
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));
    payload
}

#[test]
fn later_definition_leaders_fill_the_slots_after_the_prototype() {
    let (tags, unresolved) = decoded(&leader_fixture(&[0x52, 0x5e]));
    assert_eq!(unresolved, 0);
    assert_eq!(
        tags.iter()
            .map(|tag| (
                tag.identification.as_str(),
                tag.definition_slot,
                tag.attachment
            ))
            .collect::<Vec<_>>(),
        [
            (
                "A",
                Some(13),
                Some(DatumAttachment {
                    reference_type: 0x25,
                    id: 0x6a
                })
            ),
            (
                "B",
                Some(14),
                Some(DatumAttachment {
                    reference_type: 0x25,
                    id: 0x52
                })
            ),
            (
                "C",
                Some(15),
                Some(DatumAttachment {
                    reference_type: 0x25,
                    id: 0x5e
                })
            ),
        ]
    );
}

#[test]
fn definition_leaders_need_one_row_per_later_slot() {
    let (tags, unresolved) = decoded(&leader_fixture(&[0x52]));
    assert_eq!(unresolved, 2);
    assert!(tags
        .iter()
        .skip(1)
        .all(|tag| tag.definition_slot.is_none() && tag.attachment.is_none()));
}

#[test]
fn a_positional_prototype_leader_keeps_its_type_and_identifier() {
    let mut payload = definition_array_with_leaders(16, 13, &[0x52, 0x5e]);
    let named = b"\xe3\xe0\x01type\0\x25\xe0\x01attach_id\0\x6a\xe0\x00att_offsets\0";
    let start = payload
        .windows(named.len())
        .position(|window| window == named)
        .expect("named prototype leader");
    payload.splice(start..start + named.len(), *b"\xe3\xf7\x63\x05\x81\x7f");
    payload.extend_from_slice(&tag_array(16, 13, Some(13), b"A", true));
    payload.extend_from_slice(&field(b"\x00drf_data_xar", &[0xe1]));
    let (tags, _) = decoded(&payload);
    assert_eq!(
        tags[0].attachment,
        Some(DatumAttachment {
            reference_type: 5,
            id: 0x17f
        })
    );
}
