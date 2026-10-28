// SPDX-License-Identifier: Apache-2.0
//! Exact NIST FTC11 cr4050 cross-section array, including its owned terminal.

const SOURCE_ARRAY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/ftc11-cross-section-terminal.bin"
));

fn parse(bytes: &[u8]) -> Vec<crate::curve::DepdbCurveRow> {
    crate::decode::with_service_context(|ctx| crate::curve::depdb_cross_section_rows(ctx, bytes))
        .expect("bounded cross-section parser")
}

#[test]
fn owned_section_terminal_retains_the_complete_source_rows() {
    let rows = parse(SOURCE_ARRAY);
    assert_eq!(rows.len(), 15);
    // (id, prefix offset, body offset, suffix offset, row end, next, reference)
    let expected = [
        (305, 254, 261, 285, 294, 319, 328),
        (306, 296, 303, 335, 344, 308, 321),
        (307, 346, 353, 385, 394, 305, 335),
        (308, 396, 403, 432, 441, 312, 322),
        (309, 443, 450, 479, 488, 307, 334),
        (310, 490, 497, 559, 568, 315, 331),
        (311, 570, 577, 639, 648, 316, 325),
        (312, 650, 657, 782, 791, 314, 323),
        (313, 793, 800, 923, 932, 309, 333),
        (314, 934, 941, 1079, 1088, 311, 324),
        (315, 1090, 1097, 1235, 1244, 313, 332),
        (316, 1246, 1253, 1391, 1400, 318, 326),
        (317, 1402, 1409, 1547, 1556, 310, 330),
        (318, 1558, 1565, 1688, 1697, 304, 327),
        (319, 1699, 1706, 1829, 1838, 317, 329),
    ];
    for (row, (id, offset, body, suffix, end, next, reference)) in rows.iter().zip(expected) {
        assert_eq!(row.id, id);
        assert_eq!(row.type_byte, 0);
        assert_eq!(row.feature_id, 301);
        assert_eq!(row.offset, offset);
        assert_eq!(row.body, SOURCE_ARRAY[body..suffix]);
        assert_eq!(row.suffix.section_plane, 303);
        assert_eq!(row.suffix.next_curve, next);
        assert_eq!(row.suffix.reference_surface, reference);
        assert!(end <= SOURCE_ARRAY.len() - 3);
    }
}

#[test]
fn section_terminal_is_not_an_interior_or_partial_close() {
    let mut interior = SOURCE_ARRAY.to_vec();
    interior.push(0);
    assert!(parse(&interior).is_empty());
    for cut in 1..=3 {
        assert!(parse(&SOURCE_ARRAY[..SOURCE_ARRAY.len() - cut]).is_empty());
    }
    let mut other_close = SOURCE_ARRAY.to_vec();
    *other_close.last_mut().expect("complete source") = 5;
    assert!(parse(&other_close).is_empty());
}

#[test]
fn section_terminal_does_not_supply_a_missing_declared_row() {
    let mut missing_row = SOURCE_ARRAY.to_vec();
    assert_eq!(missing_row[12], 16);
    missing_row[12] = 17;
    assert!(parse(&missing_row).is_empty());
}

#[test]
fn section_body_does_not_promote_interior_numeric_images() {
    for row in parse(SOURCE_ARRAY) {
        assert!(row.scalar_tokens.is_empty());
        assert!(row.references.is_empty());
        assert_eq!(row.opaque_spans.len(), 1);
        assert_eq!(row.opaque_spans[0].offset, 0);
        assert_eq!(row.opaque_spans[0].raw, row.body);
    }
}
