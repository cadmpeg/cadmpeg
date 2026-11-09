// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};

use super::{
    positional_prefix, prefixes_for_annotation_slots, AttachmentPrefix, GeometricTolerancePrefix,
    PlacementPrefix,
};

#[test]
fn named_workbook_position_record_joins_its_annotation_slot() {
    let sample = section(&[]);
    let parsed = unique(&sample, 17).expect("named position row for annotation slot 17");

    assert_eq!(parsed.owner_id, 17);
    assert_eq!(parsed.tolerance_type, 31);
    assert_eq!(parsed.value_string, b"0.75");
    assert_eq!(
        parsed.placement,
        Some(PlacementPrefix {
            placement_type: 1,
            id: 570,
            attachment: Some(AttachmentPrefix::Other),
        })
    );
}

#[test]
fn positional_flatness_joins_by_owner_and_exposes_type_37_attachment() {
    let sample = section(&[PositionalSpec {
        owner: 20,
        tolerance_type: 2,
        tolerance_class: 1,
        value: b"0.2",
        text: b"0.2",
        text_type: 35,
        row_class: Some(3),
        placement_type: 2,
        placement_id: 106,
        attachment: AttachmentSpec::Inline {
            reference_type: 37,
            id: 106,
        },
    }]);
    let parsed = unique(&sample, 20).expect("flatness row for annotation slot 20");

    assert_eq!(parsed.owner_id, 20);
    assert_eq!(parsed.tolerance_type, 2);
    assert_eq!(parsed.value_string, b"0.2");
    assert_eq!(
        parsed.placement,
        Some(PlacementPrefix {
            placement_type: 2,
            id: 106,
            attachment: Some(AttachmentPrefix::Inline {
                reference_type: 37,
                id: 106,
            }),
        })
    );
}

#[test]
fn positional_perpendicularity_replays_the_typed_attachment_fields() {
    let sample = section(&[perpendicularity(AttachmentSpec::Positional {
        reference_type: 37,
        id: 77,
    })]);
    let parsed = unique(&sample, 22).expect("perpendicularity row for annotation slot 22");
    let placement = parsed.placement.expect("placement prefix");

    assert_eq!(parsed.owner_id, 22);
    assert_eq!(parsed.tolerance_type, 22);
    assert_eq!(parsed.value_string, b"1.5");
    assert_eq!(placement.id, 77);
    assert_eq!(
        placement.attachment,
        Some(AttachmentPrefix::Inline {
            reference_type: 37,
            id: 77,
        })
    );
}

#[test]
fn positional_attachment_requires_the_offset_array() {
    let sample = section(
        &[perpendicularity(AttachmentSpec::PositionalWithoutOffsets {
            reference_type: 37,
            id: 77,
        })],
    );
    let parsed = unique(&sample, 22).expect("perpendicularity row for annotation slot 22");

    assert_eq!(parsed.placement.expect("placement prefix").attachment, None);
}

fn perpendicularity(attachment: AttachmentSpec) -> PositionalSpec {
    PositionalSpec {
        owner: 22,
        tolerance_type: 22,
        tolerance_class: 3,
        value: b"1.5",
        text: b"1.5",
        text_type: 35,
        row_class: None,
        placement_type: 2,
        placement_id: 77,
        attachment,
    }
}

#[test]
fn malformed_e3_compound_does_not_hide_a_later_typed_row() {
    let sample = section_with_false_prefix(&[PositionalSpec {
        owner: 20,
        tolerance_type: 2,
        tolerance_class: 1,
        value: b"0.2",
        text: b"0.2",
        text_type: 35,
        row_class: Some(3),
        placement_type: 2,
        placement_id: 106,
        attachment: AttachmentSpec::Inline {
            reference_type: 37,
            id: 106,
        },
    }]);
    let parsed = unique(&sample, 20).expect("typed row after malformed e3 candidate");

    assert_eq!(parsed.owner_id, 20);
    assert_eq!(parsed.tolerance_type, 2);
    assert_eq!(parsed.value_string, b"0.2");
}

#[test]
fn positional_rows_join_when_the_prototype_has_no_drf_id_frame() {
    let sample = section(&[flatness()]);
    let mut bytes = sample.bytes.clone();
    let framed_drf_id = b"\xe3\xe0\x01drf_id\0\x03";
    let start = bytes
        .windows(framed_drf_id.len())
        .position(|window| window == framed_drf_id)
        .expect("prototype drf_id frame");
    bytes.splice(start..start + framed_drf_id.len(), [0xe1]);
    let sample = Sample { bytes, ..sample };
    let parsed = prefixes(&sample, &[17, 20]).expect("named tolerance prefix");

    assert_eq!(parsed[0].map(|prefix| prefix.owner_id), Some(17));
    assert_eq!(parsed[1].map(|prefix| prefix.owner_id), Some(20));
}

#[test]
fn empty_item_names_frame_named_and_positional_rows() {
    let sample = section(&[flatness()]);
    let mut bytes = sample.bytes.clone();
    let named_name = b"\xe0\x0aname\0\xf1\xe1";
    let start = bytes
        .windows(named_name.len())
        .position(|window| window == named_name)
        .expect("named item name");
    bytes[start + named_name.len() - 1] = 0;
    let positional_name = b"\xe2\xe2\xe1\x02";
    let start = bytes
        .windows(positional_name.len())
        .position(|window| window == positional_name)
        .expect("positional item name");
    bytes[start + 2] = 0;
    let sample = Sample { bytes, ..sample };
    let parsed = prefixes(&sample, &[17, 20]).expect("named tolerance prefix");

    assert_eq!(parsed[0].map(|prefix| prefix.owner_id), Some(17));
    assert_eq!(
        parsed[1].map(|prefix| prefix.value_string),
        Some(b"0.2".as_slice())
    );
}

fn flatness() -> PositionalSpec {
    PositionalSpec {
        owner: 20,
        tolerance_type: 2,
        tolerance_class: 1,
        value: b"0.2",
        text: b"0.2",
        text_type: 35,
        row_class: Some(3),
        placement_type: 2,
        placement_id: 106,
        attachment: AttachmentSpec::Inline {
            reference_type: 37,
            id: 106,
        },
    }
}

#[test]
fn duplicate_typed_rows_for_one_annotation_slot_are_ambiguous() {
    let spec = PositionalSpec {
        owner: 20,
        tolerance_type: 2,
        tolerance_class: 1,
        value: b"0.2",
        text: b"0.2",
        text_type: 35,
        row_class: Some(3),
        placement_type: 2,
        placement_id: 106,
        attachment: AttachmentSpec::Inline {
            reference_type: 37,
            id: 106,
        },
    };
    let sample = section(&[spec, spec]);

    assert!(unique(&sample, 20).is_none());
}

#[test]
fn duplicate_requested_annotation_slots_are_ambiguous() {
    let sample = section(&[PositionalSpec {
        owner: 20,
        tolerance_type: 2,
        tolerance_class: 1,
        value: b"0.2",
        text: b"0.2",
        text_type: 35,
        row_class: Some(3),
        placement_type: 2,
        placement_id: 106,
        attachment: AttachmentSpec::Inline {
            reference_type: 37,
            id: 106,
        },
    }]);
    let parsed = prefixes(&sample, &[20, 20]).expect("bounded tolerance prefix scan");

    assert_eq!(parsed.len(), 2);
    assert!(parsed.iter().all(Option::is_none));
}

#[test]
fn unsupported_coordinate_scalar_keeps_the_placement_header() {
    let sample = section(&[PositionalSpec {
        owner: 20,
        tolerance_type: 2,
        tolerance_class: 1,
        value: b"0.2",
        text: b"0.2",
        text_type: 35,
        row_class: Some(3),
        placement_type: 2,
        placement_id: 106,
        attachment: AttachmentSpec::Null,
    }]);
    let mut bytes = sample.bytes.clone();
    bytes[sample.positional_coordinate_offsets[0]] = 0xf0;
    let sample = Sample { bytes, ..sample };
    let parsed =
        unique(&sample, 20).expect("typed tolerance prefix with unsupported placement coordinate");
    let placement = parsed.placement.expect("placement header");

    assert_eq!(parsed.value_string, b"0.2");
    assert_eq!(placement.placement_type, 2);
    assert_eq!(placement.id, 106);
    assert_eq!(placement.attachment, None);
}

#[test]
fn direct_positional_parser_returns_workbook_values_after_validating_text_grammar() {
    let sample = section(&[PositionalSpec {
        owner: 18,
        tolerance_type: 31,
        tolerance_class: 4,
        value: b"0.75",
        text: b"0.75 A B C",
        text_type: 42,
        row_class: None,
        placement_type: 1,
        placement_id: 570,
        attachment: AttachmentSpec::Null,
    }]);
    let row_offset = sample.positional_row_offsets[0];
    let parsed = positional_prefix(
        View::over_retained(&sample.bytes)
            .child(row_offset, sample.dim_array_offset)
            .expect("row-bounded view"),
    )
    .expect("typed positional row");

    assert_eq!(parsed.owner_id, 18);
    assert_eq!(parsed.tolerance_type, 31);
    assert_eq!(parsed.value_string, b"0.75");
    assert_eq!(parsed.placement.map(|placement| placement.id), Some(570));
}

#[test]
fn positional_value_string_can_carry_the_named_array_wrapper() {
    let sample = section(&[PositionalSpec {
        value: b"\xf8\x01\xe8\xe2\x8c\x800.35",
        ..flatness()
    }]);
    let parsed = unique(&sample, 20).expect("typed row with a wrapped value string");

    assert_eq!(parsed.value_string, b"\xe8\xe2\x8c\x800.35");
}

#[derive(Clone, Copy)]
struct PositionalSpec {
    owner: u8,
    tolerance_type: u8,
    tolerance_class: u8,
    value: &'static [u8],
    text: &'static [u8],
    text_type: u8,
    row_class: Option<u8>,
    placement_type: u8,
    placement_id: u32,
    attachment: AttachmentSpec,
}

#[derive(Clone, Copy)]
enum AttachmentSpec {
    Null,
    Inline { reference_type: u8, id: u8 },
    Positional { reference_type: u8, id: u8 },
    PositionalWithoutOffsets { reference_type: u8, id: u8 },
}

const ATT_OFFSETS: &[u8] = &[0xf8, 2, 0xf7, 59, 0xfb];

struct Sample {
    bytes: Vec<u8>,
    section_start: usize,
    positional_row_offsets: Vec<usize>,
    positional_coordinate_offsets: Vec<usize>,
    dim_array_offset: usize,
}

fn section_view(sample: &Sample) -> View<'_> {
    View::over_retained(&sample.bytes)
        .child(sample.section_start, sample.bytes.len())
        .expect("bounded section view")
}

fn unique(sample: &Sample, slot: u32) -> Option<GeometricTolerancePrefix<'_>> {
    prefixes(sample, &[slot])?.into_iter().next()?
}

fn prefixes<'a>(
    sample: &'a Sample,
    slots: &[u32],
) -> Option<Vec<Option<GeometricTolerancePrefix<'a>>>> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&sample.bytes, &arena, &policy).ok()?;
    prefixes_for_annotation_slots(&ctx, section_view(sample), slots).ok()?
}

fn section(rows: &[PositionalSpec]) -> Sample {
    section_with_false_prefix(rows)
}

fn section_with_false_prefix(rows: &[PositionalSpec]) -> Sample {
    let mut bytes = vec![0x44, 0x55];
    let section_start = bytes.len();
    named(&mut bytes, 0, b"geom_tol_array", &[0xf8, 24, 0xf7, 2, 0xfb]);
    bytes.extend(std::iter::repeat_n(0xe1, 17));
    named_row(&mut bytes);
    named(
        &mut bytes,
        0,
        b"drfs_data",
        &[0xf4, 8, 0xf8, 1, 0xf7, 0x37, 0xfb],
    );
    bytes.extend_from_slice(b"\xe3\xe0\x01drf_id\0\x03\xf3\xf7\x02");
    bytes.extend_from_slice(b"\xe3\xe2\xe2\xe1\x1f\x04\xf6\x78\0\xe3");

    let mut positional_row_offsets = Vec::new();
    let mut positional_coordinate_offsets = Vec::new();
    for row in rows {
        let (row_offset, coordinate_offset) = positional_row(&mut bytes, *row);
        positional_row_offsets.push(row_offset);
        positional_coordinate_offsets.push(coordinate_offset);
    }
    let dim_array_offset = bytes.len();
    named(&mut bytes, 0, b"dim_array", &[0xf8, 0, 0xf7, 2, 0xfb]);
    Sample {
        bytes,
        section_start,
        positional_row_offsets,
        positional_coordinate_offsets,
        dim_array_offset,
    }
}

fn named_row(bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(b"\xe3\xe0\x00dtl_named_item\0\xe2\xe0\x00dtl_item\0\xe2");
    named(bytes, 10, b"name", &[0xf1, 0xe1]);
    named(bytes, 1, b"tol_type", &[0xf1, 31]);
    named(bytes, 1, b"tol_class", &[4]);
    named(bytes, 1, b"feat_id", &[0xf6]);
    named(
        bytes,
        10,
        b"value_string",
        &[0xf8, 1, b'0', b'.', b'7', b'5', 0],
    );
    named(bytes, 0, b"tol_text", &[0xf8, 1, 0xf7, 10, 0xfb, 0xe3]);
    named(bytes, 1, b"db_type", &[62]);
    named(bytes, 1, b"text_type", &[42]);
    named(bytes, 10, b"text_value", b"0.75 A B C\0");
    named(bytes, 1, b"txt_attrib", &[0xf5, 0xeb, 0x80, 0, 0, 0]);
    named(bytes, 1, b"txt_attrib2", &[0]);
    named(bytes, 1, b"owner_id", &[17]);
    named(bytes, 2, b"height", &[16]);
    named(bytes, 2, b"width_factor", &[16]);
    named(bytes, 0, b"format_info", &[0xe2]);
    named(bytes, 2, b"thickness", &[0xf1, 16]);
    named(bytes, 2, b"actual_vertical_extent_start", &[16]);
    named(bytes, 2, b"actual_vertical_extent_end", &[16]);
    named(bytes, 0, b"gtol_place_ptr", &[]);
    bytes.extend_from_slice(b"\xf2\xe3\xe0\x01type\0\x01\xe0\x01id\0\x82\x3a");
    bytes.extend_from_slice(b"\xe0\x01attribute\0\0\xe0\x02coord\0\xf8\x03\x18\x18\x18");
    named(bytes, 0, b"gtol_attach_pnt", &[0xe1]);
}

fn positional_row(bytes: &mut Vec<u8>, spec: PositionalSpec) -> (usize, usize) {
    let row_offset = bytes.len();
    bytes.push(0xe3);
    if let Some(class) = spec.row_class {
        bytes.extend_from_slice(&[0xf7, class]);
    }
    bytes.extend_from_slice(b"\xe2\xe2\xe1");
    bytes.push(spec.tolerance_type);
    bytes.push(spec.tolerance_class);
    bytes.push(0xf6);
    bytes.extend_from_slice(spec.value);
    bytes.push(0);
    bytes.extend_from_slice(&[0xf8, 1, 0xf7, 10, 0xfb, 0xe3, 0xf7, 11, 62, spec.text_type]);
    bytes.extend_from_slice(spec.text);
    bytes.push(0);
    bytes.extend_from_slice(&[0xf5, 0xeb, 0x80, 0, 0, 0, 0]);
    bytes.push(spec.owner);
    bytes.extend_from_slice(&[16, 16, 0xe2, 16, 16, 16]);
    bytes.extend_from_slice(&[0xe3, spec.placement_type]);
    compact(bytes, spec.placement_id);
    bytes.push(0);
    let coordinate_offset = bytes.len();
    bytes.extend_from_slice(&[0x18, 0x18, 0x18]);
    match spec.attachment {
        AttachmentSpec::Null => bytes.push(0xe1),
        AttachmentSpec::Inline { reference_type, id } => {
            bytes.extend_from_slice(b"\xe3\xe0\x01type\0\xf0");
            bytes.push(reference_type);
            bytes.extend_from_slice(b"\xe0\x01attach_id\0");
            bytes.push(id);
            bytes.extend_from_slice(b"\xe0\x00att_offsets\0");
            bytes.extend_from_slice(ATT_OFFSETS);
        }
        AttachmentSpec::Positional { reference_type, id } => {
            bytes.extend_from_slice(&[0xe3, reference_type, id]);
            bytes.extend_from_slice(ATT_OFFSETS);
        }
        AttachmentSpec::PositionalWithoutOffsets { reference_type, id } => {
            bytes.extend_from_slice(&[0xe3, reference_type, id, 0xe1]);
        }
    }
    (row_offset, coordinate_offset)
}

fn named(bytes: &mut Vec<u8>, kind: u8, name: &[u8], value: &[u8]) {
    bytes.extend_from_slice(&[0xe0, kind]);
    bytes.extend_from_slice(name);
    bytes.push(0);
    bytes.extend_from_slice(value);
}

fn compact(bytes: &mut Vec<u8>, value: u32) {
    if value < 0x80 {
        bytes.push(u8::try_from(value).expect("one-byte compact integer"));
    } else {
        assert!(value < 0x4000);
        let high = u8::try_from((value >> 8) & 0x3f).expect("two-byte compact high byte");
        let low = u8::try_from(value & 0xff).expect("two-byte compact low byte");
        bytes.extend_from_slice(&[0x80 | high, low]);
    }
}

/// A positional dimension element whose `p_sld_info` names owner `(3886, 219)`
/// and whose `attach_ptr` chain holds two edge links.
fn dimension_element(owner_references: usize, class: &[u8]) -> Vec<u8> {
    let mut bytes = b"\xe3\xe2\xe2\xe1\x08\x05\xf6".to_vec();
    for _ in 0..owner_references {
        bytes.push(0xe3);
        bytes.extend_from_slice(class);
        bytes.extend_from_slice(b"\x80\xdb\x8f\x2e\xe1\xe1\x0a\xf6\xf6\x29\xe3\x33");
    }
    bytes.extend_from_slice(b"\xe3\x01\x87\xe5");
    bytes.extend_from_slice(ATT_OFFSETS);
    bytes.extend_from_slice(b"\xe3\xf7\x3c\x2e\x18\x18\xf7\x80\xa9\xe3\x01\x87\xe3");
    bytes.extend_from_slice(ATT_OFFSETS);
    bytes.extend_from_slice(b"\xe3\xf7\x3c\x2e\x18\x18");
    bytes
}

fn attachments_of(bytes: &[u8]) -> Option<Vec<(u32, u32)>> {
    let span = crate::feature::dimensions::ElementSpan {
        slot: 567,
        start: 0,
        end: bytes.len(),
    };
    super::dimension_attachments(View::over_retained(bytes), span, (3886, 219))
}

#[test]
fn dimension_attachments_follow_the_owner_reference() {
    assert_eq!(
        attachments_of(&dimension_element(1, b"")),
        Some(vec![(1, 0x07e5), (1, 0x07e3)])
    );
    assert_eq!(
        attachments_of(&dimension_element(1, b"\xf7\x80\x9f")),
        Some(vec![(1, 0x07e5), (1, 0x07e3)])
    );
}

#[test]
fn dimension_attachments_require_one_owner_reference() {
    assert_eq!(attachments_of(&dimension_element(0, b"")), None);
    assert_eq!(attachments_of(&dimension_element(2, b"")), None);
}
