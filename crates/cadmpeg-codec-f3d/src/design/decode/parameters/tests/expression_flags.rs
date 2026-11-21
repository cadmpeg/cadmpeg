// SPDX-License-Identifier: Apache-2.0

use super::*;

/// A wide discriminator-6 owned dimension parameter with the given wide
/// expression trailer.
fn wide_owned(trailer: [u8; 11]) -> Vec<u8> {
    let mut frame = parameter_record(
        Some(0xee),
        "40.00 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        4.0,
    );
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    frame.splice(22..22, [0; 4]);
    let expression_end = 50 + 4 + 2 * "40.00 mm".len();
    frame.splice(expression_end..expression_end + 9, trailer);
    frame
}

/// A wide discriminator-6 document parameter with the given wide expression
/// trailer.
fn wide_document(trailer: [u8; 11]) -> Vec<u8> {
    let mut frame = parameter_record(None, "12 mm", "User Parameter", Some("mm"), "uf", 1.2);
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    frame.splice(22..22, [0; 4]);
    let expression_end = 40 + 4 + 2 * "12 mm".len();
    frame.splice(expression_end..expression_end + 9, trailer);
    frame
}

#[test]
fn wide_expression_trailer_states_the_driven_and_favorite_flags() {
    let read = |trailer: [u8; 11]| {
        parse_design_parameter_record(&wide_owned(trailer)).map(|parameter| {
            let flags = parameter.flags();
            (flags.driven, flags.favorite, parameter.driving())
        })
    };
    assert_eq!(read([0; 11]), Some((false, false, None)));
    assert_eq!(
        read([0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]),
        Some((true, false, Some(false)))
    );
    assert_eq!(
        read([0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0]),
        Some((false, true, None))
    );
    assert_eq!(
        read([0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]),
        Some((true, true, Some(false)))
    );
    // Reserved bytes, other flag values, and an owned parameter with the
    // document flag match no grammar.
    for refused in [
        [0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    ] {
        assert_eq!(read(refused), None, "{refused:?}");
    }
}

#[test]
fn wide_document_trailer_states_the_favorite_flag() {
    let parameter =
        parse_design_parameter_record(&wide_document([0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1]))
            .expect("favorite document parameter");
    assert_eq!(parameter.kind(), DesignParameterKind::User);
    assert!(parameter.flags().favorite);
    assert_eq!(parameter.driving(), None);
    let plain = parse_design_parameter_record(&wide_document([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]))
        .expect("document parameter");
    assert!(!plain.flags().favorite);
    // A document parameter has driven flag 0.
    assert!(
        parse_design_parameter_record(&wide_document([0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1])).is_none()
    );
}

#[test]
fn compact_owned_trailer_states_the_driven_flag() {
    let read = |trailer: [u8; 5]| {
        let mut frame = compact_owned_parameter_record(
            0x454,
            0x0f,
            "1.8150388199 mm",
            "Linear Dimension-2",
            Some("mm"),
            "d15",
            0.181_503_881_99,
        );
        let expression_end = 41 + 4 + 2 * "1.8150388199 mm".len();
        frame[expression_end..expression_end + 5].copy_from_slice(&trailer);
        parse_design_parameter_record(&frame)
    };
    let driven = read([0, 1, 0, 0, 0]).expect("driven compact parameter");
    assert!(driven.flags().driven);
    assert_eq!(driven.driving(), Some(false));
    let driving = read([0; 5]).expect("driving compact parameter");
    assert!(!driving.flags().driven);
    assert_eq!(driving.driving(), None);
    for refused in [
        [1, 0, 0, 0, 0],
        [0, 2, 0, 0, 0],
        [0, 0, 1, 0, 0],
        [0, 0, 0, 0, 1],
    ] {
        assert!(read(refused).is_none(), "{refused:?}");
    }
}

/// A wide discriminator-6 text parameter with the given trailer and text value.
fn wide_text(owner: Option<u32>, trailer: [u8; 11], text: &str) -> Vec<u8> {
    let kind = if owner.is_some() {
        "Text1-Text"
    } else {
        "User Parameter"
    };
    let mut frame = parameter_record(owner, "'AB'", kind, Some("Text"), "tp", 0.0);
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    frame.splice(22..22, [0; 4]);
    let expression_end = if owner.is_some() { 50 } else { 40 } + 4 + 2 * "'AB'".len();
    frame.splice(expression_end..expression_end + 9, trailer);
    let text_at = expression_end + 11 + 4 + 2 * kind.len();
    let mut field = Vec::new();
    lp_utf16(&mut field, text);
    frame.splice(text_at..text_at + 4, field);
    frame
}

#[test]
fn text_flag_admits_the_text_value_after_the_source_kind() {
    let document = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    for text in ["AB", "", "Hello world 123"] {
        let parameter =
            parse_design_parameter_record(&wide_text(None, document, text)).expect("text");
        assert_eq!(parameter.kind(), DesignParameterKind::User);
        assert_eq!(parameter.text_value(), Some(text));
        assert_eq!(
            parameter.unit().map(|unit| unit.value.as_str()),
            Some("Text")
        );
        assert_eq!(parameter.name(), "tp");
    }
    let owned = parse_design_parameter_record(&wide_text(
        Some(0xf6),
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        "AB",
    ))
    .expect("owned text parameter");
    assert_eq!(owned.text_value(), Some("AB"));
    assert_eq!(owned.source_kind(), "Text1-Text");

    // Without the text flag, the field after the source kind is empty.
    let numeric = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    assert_eq!(
        parse_design_parameter_record(&wide_text(None, numeric, ""))
            .expect("numeric parameter")
            .text_value(),
        None
    );
    assert!(parse_design_parameter_record(&wide_text(None, numeric, "AB")).is_none());
    assert!(parse_design_parameter_record(&wide_text(
        None,
        [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        "AB"
    ))
    .is_none());
}

/// An eleven-byte discriminated parameter whose expression `1` is followed by
/// `trailer`.
fn discriminated(owner: Option<u32>, kind: &str, trailer: &[u8]) -> Vec<u8> {
    let mut frame = parameter_record(owner, "1", kind, None, "d4", 1.0);
    let expression_end = if owner.is_some() { 46 } else { 36 } + 4 + 2;
    frame.splice(expression_end..expression_end + 9, trailer.iter().copied());
    frame
}

#[test]
fn discriminated_frame_reads_the_nine_byte_and_the_ten_byte_trailer() {
    for trailer in [&[0; 9][..], &[0; 10][..]] {
        let owned = parse_design_parameter_record(&discriminated(
            Some(0x31),
            "Linear Dimension-2",
            trailer,
        ))
        .expect("owned parameter");
        assert_eq!(owned.owner_record_index(), Some(0x31));
        assert_eq!(owned.source_kind(), "Linear Dimension-2");
        assert_eq!(owned.text_value(), None);

        let mut document_trailer = trailer.to_vec();
        *document_trailer.last_mut().unwrap() = 1;
        let document = parse_design_parameter_record(&discriminated(
            None,
            "User Parameter",
            &document_trailer,
        ))
        .expect("document parameter");
        assert_eq!(document.kind(), DesignParameterKind::User);
    }
}

#[test]
fn ten_byte_trailer_keeps_a_parameter_with_a_set_flag_byte() {
    let tangency = parse_design_parameter_record(&discriminated(
        Some(0x5d),
        "TangencyWeight",
        &[0, 0, 0, 0, 0, 1, 0, 0, 0, 0],
    ))
    .expect("ten-byte trailer with its first flag byte set");
    assert_eq!(tangency.owner_record_index(), Some(0x5d));
    assert_eq!(tangency.source_kind(), "TangencyWeight");
    assert_eq!(tangency.driving(), None);

    let null_owner = parse_design_parameter_record(&discriminated(
        None,
        "Angular Dimension-2",
        &[0, 0, 0, 0, 0, 0, 0, 0, 1, 0],
    ))
    .expect("ten-byte trailer with its fourth flag byte set");
    assert_eq!(null_owner.owner_record_index(), None);
    assert_eq!(null_owner.source_kind(), "Angular Dimension-2");

    let nine_byte = parse_design_parameter_record(&discriminated(
        Some(0x5d),
        "TangencyWeight",
        &[0, 0, 0, 0, 1, 0, 0, 0, 0],
    ))
    .expect("nine-byte trailer with its first flag byte set");
    assert_eq!(nine_byte.source_kind(), "TangencyWeight");

    for refused in [
        &[0, 0, 0, 0, 0, 2, 0, 0, 0, 0][..],
        &[0, 0, 1, 0, 0, 0, 0, 0, 0, 0][..],
        &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0][..],
        &[0, 0, 0, 0, 2, 0, 0, 0, 0][..],
    ] {
        assert!(
            parse_design_parameter_record(&discriminated(Some(0x5d), "TangencyWeight", refused))
                .is_none(),
            "{refused:?}"
        );
    }
}

#[test]
fn ten_byte_text_flag_admits_the_text_value_after_the_source_kind() {
    let text_parameter = |trailer: [u8; 10], text: &str| {
        let kind = "Text1-Text";
        let mut frame = parameter_record(Some(0xf6), "'AB'", kind, Some("Text"), "tp", 0.0);
        let expression_end = 46 + 4 + 2 * "'AB'".len();
        frame.splice(expression_end..expression_end + 9, trailer);
        let text_at = expression_end + 10 + 4 + 2 * kind.len();
        let mut field = Vec::new();
        lp_utf16(&mut field, text);
        frame.splice(text_at..text_at + 4, field);
        parse_design_parameter_record(&frame)
    };
    let text =
        text_parameter([1, 0, 0, 0, 0, 0, 0, 0, 0, 0], "B6 Probe").expect("owned text parameter");
    assert_eq!(text.text_value(), Some("B6 Probe"));
    assert_eq!(text.source_kind(), "Text1-Text");
    assert_eq!(text.unit().map(|unit| unit.value.as_str()), Some("Text"));
    assert!(text_parameter([0; 10], "B6 Probe").is_none());
}

#[test]
fn localized_document_source_kind_matches_the_document_flag() {
    let parameter = parse_design_parameter_record(&discriminated(
        None,
        "Parâmetro do usuário",
        &[0, 0, 0, 0, 0, 0, 0, 0, 1],
    ))
    .expect("Portuguese document parameter");
    assert_eq!(parameter.kind(), DesignParameterKind::User);
    assert_eq!(parameter.source_kind(), "Parâmetro do usuário");
}

#[test]
fn document_flag_selects_a_document_parameter_whatever_its_source_kind() {
    for trailer in [
        &[0, 0, 0, 0, 0, 0, 0, 0, 1][..],
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 1][..],
    ] {
        let parameter =
            parse_design_parameter_record(&discriminated(None, "Benutzerparameter", trailer))
                .unwrap_or_else(|| panic!("German document parameter, {trailer:?}"));
        assert_eq!(parameter.kind(), DesignParameterKind::User);
        assert_eq!(parameter.source_kind(), "Benutzerparameter");
        assert_eq!(parameter.owner_record_index(), None);
        assert_eq!(parameter.name(), "d4");

        // Without the document flag, a document source kind names an owned
        // parameter.
        let mut unflagged = trailer.to_vec();
        *unflagged.last_mut().unwrap() = 0;
        let owned =
            parse_design_parameter_record(&discriminated(Some(0x31), "User Parameter", &unflagged))
                .unwrap_or_else(|| panic!("owned parameter, {unflagged:?}"));
        assert_ne!(owned.kind(), DesignParameterKind::User);
        assert_eq!(owned.owner_record_index(), Some(0x31));

        // An owner with the document flag matches no grammar.
        assert!(parse_design_parameter_record(&discriminated(
            Some(0x31),
            "Benutzerparameter",
            trailer
        ))
        .is_none());
    }
}
