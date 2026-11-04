// SPDX-License-Identifier: Apache-2.0

use super::*;

/// The LP-UTF16 field of `text`.
fn text_field(text: &str) -> Vec<u8> {
    let mut field = Vec::new();
    lp_utf16(&mut field, text);
    field
}

/// A wide discriminator-6 owned dimension parameter with the given comment.
fn wide_owned(comment: &str) -> Vec<u8> {
    let mut frame = parameter_record(
        Some(0xee),
        "40.00 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        4.0,
    );
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    frame[31..35].copy_from_slice(&3u32.to_le_bytes());
    frame.splice(22..22, [0; 4]);
    let expression_end = 50 + 4 + 2 * "40.00 mm".len();
    frame.splice(expression_end..expression_end, [0; 2]);
    frame.splice(31..35, text_field(comment));
    frame
}

/// A wide discriminator-6 document parameter with the given comment.
fn wide_document(comment: &str) -> Vec<u8> {
    let mut frame = parameter_record(None, "12 mm", "User Parameter", Some("mm"), "uf", 1.2);
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    frame.splice(22..22, [0; 4]);
    let expression_end = 40 + 4 + 2 * "12 mm".len();
    frame.splice(expression_end..expression_end, [0; 2]);
    frame.splice(31..35, text_field(comment));
    frame
}

#[test]
fn wide_parameter_comment_moves_the_later_fields() {
    for comment in ["note", "Ø entraxe 12"] {
        let parameter = parse_design_parameter_record(&wide_owned(comment)).expect("commented");
        assert_eq!(parameter.comment(), Some(comment));
        assert_eq!(parameter.source_ordinal, 3);
        assert_eq!(parameter.owner_record_index(), Some(0xee));
        assert_eq!(parameter.expression(), "40.00 mm");
        assert_eq!(parameter.name(), "d1");
        assert_eq!(
            parameter
                .family_discriminator()
                .map(|field| (field.value.code(), field.offset)),
            Some((6, 26))
        );
        assert_eq!(
            parameter.expression_offset(),
            u64::try_from(50 + 4 + 2 * comment.encode_utf16().count()).unwrap()
        );

        let document = parse_design_parameter_record(&wide_document(comment)).expect("document");
        assert_eq!(document.kind(), DesignParameterKind::User);
        assert_eq!(document.comment(), Some(comment));
        assert_eq!(document.expression(), "12 mm");
    }
    let plain = parse_design_parameter_record(&wide_owned("")).expect("no comment");
    assert_eq!(plain.comment(), None);
    assert_eq!(plain.expression_offset(), 54);
    let blank = parse_design_parameter_record(&wide_owned("  ")).expect("blank comment");
    assert_eq!(blank.comment(), None);
    assert_eq!(blank.expression_offset(), 58);
}

#[test]
fn wide_parameter_comment_requires_its_zero_byte_and_complete_code_units() {
    let mut marked = wide_owned("note");
    assert_eq!(wide_parameter_comment(&marked), Some(("note".into(), 43)));
    marked[30] = 1;
    assert!(wide_parameter_comment(&marked).is_none());
    assert!(parse_design_parameter_record(&marked).is_none());

    let mut overlong = wide_owned("note");
    overlong[31..35].copy_from_slice(&5u32.to_le_bytes());
    assert!(parse_design_parameter_record(&overlong).is_none());

    let mut discriminator = wide_owned("note");
    discriminator[26..30].copy_from_slice(&7u32.to_le_bytes());
    assert!(parse_design_parameter_record(&discriminator).is_none());
}
