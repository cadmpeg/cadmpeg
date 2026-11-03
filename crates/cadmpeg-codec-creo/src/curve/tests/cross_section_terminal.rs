// SPDX-License-Identifier: Apache-2.0
//! A complete cross-section array, including its owned terminal.

const ROW_COUNT: u16 = 15;
const FEATURE: u16 = 417;
const SECTION_PLANE: u16 = 419;
const FIRST_ROW_ID: u16 = 421;
const COUNT_OFFSET: usize = b"crv_array\0".len() + 2;
const NUMERIC_IMAGES: [&[u8]; 3] = [
    &[0x2f, 0x30, 0x00],
    &[0x48, 0x30, 0x00],
    &[0x2d, 0x37, 0xc0, 0xcb, 0x12, 0x34, 0x56, 0x58],
];

struct ExpectedRow {
    id: u16,
    offset: usize,
    body: std::ops::Range<usize>,
    next: u16,
    reference: u16,
}

struct SourceArray {
    bytes: Vec<u8>,
    rows: Vec<ExpectedRow>,
}

fn reference(value: u16) -> [u8; 2] {
    [
        0x80 | u8::try_from(value >> 8).expect("two-byte id"),
        u8::try_from(value & 0xff).expect("low byte"),
    ]
}

fn named(rank: u8, name: &[u8], value: &[u8]) -> Vec<u8> {
    [&[0xe0, rank][..], name, &[0], value].concat()
}

/// The prototype row, positional rows that carry opaque numeric images, and the
/// physical `Xsections` terminal.
fn source_array() -> SourceArray {
    let mut bytes = b"crv_array\0\xf2\xf8".to_vec();
    bytes.push(u8::try_from(ROW_COUNT + 1).expect("one-byte count"));
    bytes.extend_from_slice(b"\xf7\x51\xfb\xe3");
    bytes.extend(named(1, b"crv_id", &reference(FIRST_ROW_ID - 1)));
    bytes.extend(named(1, b"type", &[0]));
    bytes.extend(named(1, b"feat_id", &reference(FEATURE)));
    bytes.extend(named(1, b"crv_pnt_dir", &[0xf8, 0x02, 0x01, 0xf6]));
    bytes.extend(named(2, b"crv_pnt_arr", &[0xf9, 0x02, 0x2f, 0x30, 0x00]));
    bytes.extend(named(1, b"crv_hdr_geom_ptr[0]", &[0]));
    bytes.extend(named(1, b"crv_hdr_geom_ptr[1]", &reference(SECTION_PLANE)));
    bytes.extend(named(1, b"next_crv_hdr_ptr[0]", &[0]));
    bytes.extend(named(1, b"next_crv_hdr_ptr[1]", &reference(FIRST_ROW_ID)));
    bytes.extend(named(1, b"ref_geom[0]", &reference(450)));
    bytes.extend(named(1, b"ref_geom[1]", &[0]));
    bytes.extend_from_slice(b"\xe0\x00topol_ref_data\0\xe1\xf1\xf7\x51\xe3");
    let mut rows = Vec::new();
    for index in 0..ROW_COUNT {
        let id = FIRST_ROW_ID + index;
        let (next, reference_surface) = (FIRST_ROW_ID + (index + 4) % ROW_COUNT, 451 + index);
        let offset = bytes.len();
        bytes.extend(reference(id));
        bytes.push(0);
        bytes.extend(reference(FEATURE));
        bytes.extend([0xf6, 0x01]);
        let body_start = bytes.len();
        for image in 0..=usize::from(index % 5) {
            bytes.extend_from_slice(NUMERIC_IMAGES[image % NUMERIC_IMAGES.len()]);
        }
        let body = body_start..bytes.len();
        bytes.push(0);
        bytes.extend(reference(SECTION_PLANE));
        bytes.push(0);
        bytes.extend(reference(next));
        bytes.extend(reference(reference_surface));
        bytes.push(0);
        if index + 1 < ROW_COUNT {
            bytes.extend([0xe1, 0xe3]);
        }
        rows.push(ExpectedRow {
            id,
            offset,
            body,
            next,
            reference: reference_surface,
        });
    }
    bytes.extend_from_slice(b"\xe1\xf4\x04");
    SourceArray { bytes, rows }
}

fn parse(bytes: &[u8]) -> Vec<crate::curve::DepdbCurveRow> {
    crate::decode::with_service_context(|ctx| crate::curve::depdb_cross_section_rows(ctx, bytes))
        .expect("bounded cross-section parser")
}

#[test]
fn owned_section_terminal_retains_the_complete_source_rows() {
    let source = source_array();
    let rows = parse(&source.bytes);
    assert_eq!(rows.len(), usize::from(ROW_COUNT));
    for (row, expected) in rows.iter().zip(&source.rows) {
        assert_eq!(row.id, u32::from(expected.id));
        assert_eq!(row.type_byte, 0);
        assert_eq!(row.feature_id, u32::from(FEATURE));
        assert_eq!(row.offset, expected.offset);
        assert_eq!(row.body, source.bytes[expected.body.clone()]);
        assert_eq!(row.suffix.section_plane, u32::from(SECTION_PLANE));
        assert_eq!(row.suffix.next_curve, u32::from(expected.next));
        assert_eq!(row.suffix.reference_surface, u32::from(expected.reference));
        assert!(expected.body.end <= source.bytes.len() - 3);
    }
}

#[test]
fn section_terminal_is_not_an_interior_or_partial_close() {
    let source = source_array().bytes;
    let mut interior = source.clone();
    interior.push(0);
    assert!(parse(&interior).is_empty());
    for cut in 1..=3 {
        assert!(parse(&source[..source.len() - cut]).is_empty());
    }
    let mut other_close = source;
    *other_close.last_mut().expect("complete source") = 5;
    assert!(parse(&other_close).is_empty());
}

#[test]
fn section_terminal_does_not_supply_a_missing_declared_row() {
    let mut missing_row = source_array().bytes;
    let declared = u8::try_from(ROW_COUNT + 1).expect("one-byte count");
    assert_eq!(missing_row[COUNT_OFFSET], declared);
    missing_row[COUNT_OFFSET] = declared + 1;
    assert!(parse(&missing_row).is_empty());
}

#[test]
fn section_body_does_not_promote_interior_numeric_images() {
    let rows = parse(&source_array().bytes);
    assert_eq!(rows.len(), usize::from(ROW_COUNT));
    for row in rows {
        assert!(row.scalar_tokens.is_empty());
        assert!(row.references.is_empty());
        assert_eq!(row.opaque_spans.len(), 1);
        assert_eq!(row.opaque_spans[0].offset, 0);
        assert_eq!(row.opaque_spans[0].raw, row.body);
    }
}
