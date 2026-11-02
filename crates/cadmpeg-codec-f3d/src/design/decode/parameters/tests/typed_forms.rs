// SPDX-License-Identifier: Apache-2.0
//! Typed, wide, and document parameter forms read their discriminated frames.

use super::super::{
    parse_design_parameter, parse_design_parameter_record, parse_parameter_companion,
    parse_parameter_owner,
};
use super::{typed_parameter_record, typed_reference, TYPED_OWNER_TYPE, TYPED_SCOPE_TYPE};
use crate::design::test_support::parameter_record;
use crate::records::parameters::DesignParameterKind;
use crate::test_support::lp_utf16;

#[test]
fn design_parameter_text_fields_keep_the_name() {
    let payload = parameter_record(Some(44), "1", "AlongDistance", Some("mm"), "d71", 1.0);
    let parsed = parse_design_parameter(&payload).unwrap().unwrap();
    assert_eq!(parsed.name, "d71");
}

#[test]
fn typed_compact_parameters_read_owned_and_document_forms() {
    let owned = typed_parameter_record(537, Some(536), "AlongDistance");
    let parameter = parse_design_parameter_record(&owned).expect("typed owned parameter");
    assert_eq!(parameter.record_index, 537);
    assert_eq!(parameter.source_ordinal, 8);
    assert_eq!(parameter.owner_record_index(), Some(536));
    assert_eq!(
        parameter
            .family_discriminator()
            .map(|located| crate::records::identity::Located {
                value: located.value,
                offset: located.offset.expect("retained test discriminator")
            }),
        None
    );
    assert_eq!(parameter.expression(), "3 mm");
    assert_eq!(parameter.expression_offset(), Some(85));
    assert_eq!(parameter.unit().map(|unit| unit.value.as_str()), Some("mm"));
    assert_eq!(parameter.name(), "d1");
    assert_eq!(parameter.evaluated_value().get(), 0.3);
    assert_eq!(
        parameter.evaluated_value_offset(),
        Some(u64::try_from(owned.len() - 60).unwrap())
    );

    // The owner reference can omit its inline type GUID.
    let mut plain = owned[..30].to_vec();
    plain.push(1);
    plain.extend_from_slice(&536u64.to_le_bytes());
    plain.extend_from_slice(&[0, 0]);
    plain.extend_from_slice(&owned[81..]);
    let parameter = parse_design_parameter_record(&plain).expect("plain-owner typed parameter");
    assert_eq!(parameter.owner_record_index(), Some(536));
    assert_eq!(parameter.expression_offset(), Some(45));

    let document = typed_parameter_record(242, None, "User Parameter");
    let parameter = parse_design_parameter_record(&document).expect("typed document parameter");
    assert_eq!(parameter.owner_record_index(), None);
    assert_eq!(
        parameter.source(),
        &crate::records::parameters::DesignParameterSource::User {
            family_discriminator: None
        }
    );
    assert_eq!(parameter.expression_offset(), Some(35));

    // Owner presence must agree with the document source kind, and the tail
    // must name the typed parameter-tail type.
    assert!(
        parse_design_parameter_record(&typed_parameter_record(537, None, "AlongDistance"))
            .is_none()
    );
    assert!(parse_design_parameter_record(&typed_parameter_record(
        537,
        Some(536),
        "User Parameter"
    ))
    .is_none());
    let mut other_tail = owned;
    let guid_at = other_tail.len() - 38;
    other_tail[guid_at] = b'C';
    assert!(parse_design_parameter_record(&other_tail).is_none());
}

/// A scalar owner frame whose five references all use the typed spelling.
fn typed_owner_frame(repeated_scope_tail: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&3u32.to_le_bytes());
    frame.extend_from_slice(b"378");
    frame.extend_from_slice(&536u32.to_le_bytes());
    frame.extend_from_slice(&[0; 8]);
    frame.extend_from_slice(&[1, 1, 0, 0, 0]);
    typed_reference(&mut frame, 226, TYPED_SCOPE_TYPE);
    frame.extend_from_slice(&2u32.to_le_bytes());
    frame.push(0);
    frame.extend_from_slice(&0.3f64.to_le_bytes());
    typed_reference(&mut frame, 537, TYPED_OWNER_TYPE);
    frame.extend_from_slice(&1u32.to_le_bytes());
    frame.extend_from_slice(&[0; 4]);
    typed_reference(&mut frame, 226, TYPED_SCOPE_TYPE);
    frame.extend_from_slice(repeated_scope_tail);
    typed_reference(&mut frame, 538, TYPED_OWNER_TYPE);
    frame.push(0);
    typed_reference(&mut frame, 226, TYPED_SCOPE_TYPE);
    frame
}

#[test]
fn typed_owner_frame_widens_every_reference_and_keeps_the_scalar_envelope() {
    let frame = typed_owner_frame(&[0, 0]);
    assert_eq!(frame.len(), 303);
    let parsed = parse_parameter_owner(&frame).expect("typed scalar owner");
    assert_eq!(parsed.scope_record_index, 226);
    assert_eq!(parsed.local_ordinal, 2);
    assert_eq!(parsed.evaluated_value.get(), 0.3);
    assert_eq!(parsed.parameter_record_index, 537);
    assert_eq!(parsed.owned_ordinal, 1);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.companion_record_index, 538);
    let owner = parsed.into_record("Design1", 1000).expect("typed owner");
    assert_eq!(owner.evaluated_value_offset(), Some(1080));

    let variant = typed_owner_frame(&[1, 0, 0]);
    let parsed = parse_parameter_owner(&variant).expect("typed variant owner");
    assert_eq!(parsed.frame_length, 304);
    assert_eq!(parsed.variant, Some(0));
    assert!(parsed.into_record("Design1", 0).is_some());

    // One plain reference among typed ones is not an owner frame.
    let mut mixed = frame;
    mixed.truncate(mixed.len() - 51);
    mixed.push(1);
    mixed.extend_from_slice(&226u64.to_le_bytes());
    mixed.extend_from_slice(&[0, 0]);
    assert!(parse_parameter_owner(&mixed).is_none());
}

#[test]
fn wide_discriminated_parameter_stores_four_zero_bytes_before_its_discriminator() {
    // The current discriminated family: discriminator 6 and the `00 01 10` tail.
    let mut narrow = parameter_record(Some(305), "10 mm", "AlongDistance", Some("mm"), "d3", 1.0);
    narrow[22..30].copy_from_slice(&6u64.to_le_bytes());
    let tail = narrow.len() - 10;
    narrow[tail] = 0x10;
    let expected = parse_design_parameter_record(&narrow).expect("narrow discriminated parameter");
    assert_eq!(
        expected
            .family_discriminator()
            .and_then(|field| field.offset),
        Some(22)
    );

    let mut wide = narrow.clone();
    wide.splice(22..22, [0; 4]);
    let parameter = parse_design_parameter_record(&wide).expect("wide discriminated parameter");
    assert_eq!(parameter.owner_record_index(), Some(305));
    assert_eq!(parameter.expression(), "10 mm");
    assert_eq!(parameter.name(), "d3");
    assert_eq!(parameter.evaluated_value().get(), 1.0);
    let discriminator = parameter.family_discriminator().expect("discriminator");
    assert_eq!(
        (discriminator.value.code(), discriminator.offset),
        (6, Some(26))
    );

    // The four bytes before the discriminator are zero.
    let mut nonzero = wide.clone();
    nonzero[24] = 1;
    assert!(parse_design_parameter_record(&nonzero).is_none());
}

#[test]
fn discriminated_parameter_with_the_00_01_11_tail_has_discriminator_6() {
    let mut frame = parameter_record(
        Some(2553),
        "-65.0 deg",
        "alignAngle",
        Some("deg"),
        "d1",
        -1.134,
    );
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    let tail = frame.len() - 10;
    frame[tail] = 0x11;
    let parameter = parse_design_parameter_record(&frame).expect("tail 0x11 parameter");
    assert_eq!(parameter.name(), "d1");
    assert_eq!(parameter.evaluated_value().get(), -1.134);
    assert_eq!(
        parameter
            .family_discriminator()
            .map(|field| field.value.code()),
        Some(6)
    );

    // The 00 01 11 tail belongs to discriminator 6 only.
    frame[22..30].copy_from_slice(&0u64.to_le_bytes());
    assert!(parse_design_parameter_record(&frame).is_none());
}

#[test]
fn typed_companion_prefix_moves_the_timestamp_after_the_wide_owner_reference() {
    let mut frame = Vec::new();
    frame.extend_from_slice(&3u32.to_le_bytes());
    frame.extend_from_slice(b"408");
    frame.extend_from_slice(&538u32.to_le_bytes());
    frame.extend_from_slice(&[0; 20]);
    typed_reference(&mut frame, 536, TYPED_OWNER_TYPE);
    frame.extend_from_slice(&1_500_000_000_000_000u64.to_le_bytes());
    frame.extend_from_slice(&[0; 8]);
    assert_eq!(frame.len(), 98);
    frame.extend_from_slice(&[7; 12]);
    let parsed = parse_parameter_companion(&frame).expect("typed companion");
    assert_eq!(parsed.record_index, 538);
    assert_eq!(parsed.owner_record_index, 536);
    assert_eq!(parsed.timestamp_micros.get(), 1_500_000_000_000_000);
    let record = parsed
        .into_record("Design/BulkStream.dat", 1000)
        .expect("typed companion record");
    assert_eq!(record.timestamp_micros_offset(), Some(1082));
    assert_eq!(record.payload_byte_offset(), Some(1098));

    // A nonzero byte in the zero run after the timestamp is not a prefix.
    frame[95] = 1;
    assert!(parse_parameter_companion(&frame).is_none());
}

/// A wide discriminated document parameter whose expression is followed by
/// nine zero bytes and `u8 1`.
#[test]
fn document_parameter_admits_the_ten_byte_expression_trailer() {
    let frame = |trailer: &[u8]| {
        let mut frame = Vec::new();
        frame.extend_from_slice(&3u32.to_le_bytes());
        frame.extend_from_slice(b"317");
        frame.extend_from_slice(&202u32.to_le_bytes());
        frame.extend_from_slice(&[0; 15]);
        frame.extend_from_slice(&6u64.to_le_bytes());
        frame.push(0);
        frame.extend_from_slice(&1u32.to_le_bytes());
        frame.push(0);
        lp_utf16(&mut frame, "20 mm");
        frame.extend_from_slice(trailer);
        lp_utf16(&mut frame, "User Parameter");
        frame.extend_from_slice(&0u32.to_le_bytes());
        lp_utf16(&mut frame, "mm");
        lp_utf16(&mut frame, "W1");
        frame.extend_from_slice(&2.0f64.to_le_bytes());
        frame.extend_from_slice(&[0, 1, 0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        frame
    };
    let parameter = parse_design_parameter_record(&frame(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 1]))
        .expect("document parameter with the ten-byte trailer");
    assert_eq!(parameter.kind(), DesignParameterKind::User);
    assert_eq!(parameter.owner_record_index(), None);
    assert_eq!(parameter.name(), "W1");
    assert_eq!(parameter.expression(), "20 mm");
    assert_eq!(parameter.evaluated_value().get(), 2.0);
    assert!(parse_design_parameter_record(&frame(&[0, 0, 0, 0, 0, 0, 0, 0, 1])).is_some());
    assert!(parse_design_parameter_record(&frame(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])).is_none());
    assert!(parse_design_parameter_record(&frame(&[0, 0, 0, 0, 0, 0, 1, 0, 1])).is_some());
    assert!(parse_design_parameter_record(&frame(&[0, 0, 0, 0, 0, 0, 2, 0, 1])).is_none());
    assert!(parse_design_parameter_record(&frame(&[0, 0, 0, 1, 0, 0, 0, 0, 1])).is_none());
    // The last flag byte marks a document parameter.
    assert!(parse_design_parameter_record(&frame(&[0, 0, 0, 0, 0, 0, 0, 1, 0])).is_none());
}

/// The earlier discriminated family stores a document parameter with
/// discriminator 4, the expression trailer six zero bytes and `01 00 01`, and
/// the `00 01 13` tail.
#[test]
fn earlier_family_document_parameter_admits_its_trailer() {
    let mut frame = Vec::new();
    frame.extend_from_slice(&3u32.to_le_bytes());
    frame.extend_from_slice(b"282");
    frame.extend_from_slice(&182u32.to_le_bytes());
    frame.extend_from_slice(&[0; 11]);
    frame.extend_from_slice(&4u64.to_le_bytes());
    frame.push(0);
    frame.extend_from_slice(&5u32.to_le_bytes());
    frame.push(0);
    lp_utf16(&mut frame, "45 deg");
    frame.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 0, 1]);
    lp_utf16(&mut frame, "User Parameter");
    frame.extend_from_slice(&0u32.to_le_bytes());
    lp_utf16(&mut frame, "deg");
    lp_utf16(&mut frame, "ClipAngle");
    frame.extend_from_slice(&std::f64::consts::FRAC_PI_4.to_le_bytes());
    frame.extend_from_slice(&[0, 1, 0x13, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let parameter =
        parse_design_parameter_record(&frame).expect("earlier-family document parameter");
    assert_eq!(parameter.kind(), DesignParameterKind::User);
    assert_eq!(parameter.owner_record_index(), None);
    assert_eq!(parameter.name(), "ClipAngle");
    assert_eq!(parameter.expression(), "45 deg");
}

#[test]
fn wide_owned_parameter_accepts_eleven_zero_bytes_after_expression() {
    let mut frame = parameter_record(Some(1), "5 mm", "AlongDistance", Some("mm"), "d1", 1.0);
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    let tail_at = frame.len() - 10;
    frame[tail_at] = 16;
    frame.splice(22..22, [0; 4]);
    let expression_end = 50 + 4 + 2 * "5 mm".len();
    frame.splice(expression_end..expression_end, [0; 2]);

    let parameter = parse_design_parameter_record(&frame).expect("wide owned parameter");
    assert_eq!(parameter.owner_record_index(), Some(1));
    assert_eq!(parameter.expression(), "5 mm");
    assert_eq!(parameter.source_kind(), "AlongDistance");
    let wire = serde_json::to_value(&parameter).unwrap();
    assert!(wire.get("source_kind_offset").is_none());
    assert_eq!(parameter.name(), "d1");
    assert_eq!(parameter.evaluated_value().get(), 1.0);

    for extra_at in [expression_end + 9, expression_end + 10] {
        let mut nonzero = frame.clone();
        nonzero[extra_at] = 1;
        assert!(parse_design_parameter_record(&nonzero).is_none());
    }
    let mut longer = frame.clone();
    longer.insert(expression_end, 0);
    assert!(parse_design_parameter_record(&longer).is_none());

    let mut intermediate = frame.clone();
    let tail_at = intermediate.len() - 10;
    intermediate[tail_at] = 17;
    assert!(parse_design_parameter_record(&intermediate).is_none());

    let mut narrow = frame.clone();
    narrow.drain(22..26);
    assert!(parse_design_parameter_record(&narrow).is_none());
    let mut scale = parameter_record(Some(1), "1", "ScaleFactor", None, "d1", 1.0);
    let tail_at = scale.len() - 10;
    scale[tail_at] = 16;
    scale.splice(22..22, [0; 4]);
    assert!(parse_design_parameter_record(&scale).is_some());
    let expression_end = 50 + 4 + 2;
    scale.splice(expression_end..expression_end, [0; 2]);
    assert!(parse_design_parameter_record(&scale).is_none());
}

/// A discriminated expression trailer is four zero bytes and five flag bytes;
/// the last flag byte is set exactly for a document parameter.
#[test]
fn discriminated_expression_trailer_carries_flag_bytes() {
    let with_trailer = |owner: Option<u32>, source_kind: &str, flags: [u8; 5]| {
        let mut frame = parameter_record(owner, "4.00 mm", source_kind, Some("mm"), "d14", 0.4);
        frame[22..30].copy_from_slice(&4u64.to_le_bytes());
        let expression_at = if owner.is_some() { 46 } else { 36 };
        let (_, expression_end) =
            crate::bytes::lp_utf16_bounded(&frame, expression_at, 1..=256).expect("expression");
        frame[expression_end + 4..expression_end + 9].copy_from_slice(&flags);
        frame
    };
    for flags in [
        [1, 0, 0, 0, 0],
        [0, 1, 0, 0, 0],
        [0, 0, 1, 0, 0],
        [0, 1, 1, 0, 0],
    ] {
        let parameter =
            parse_design_parameter_record(&with_trailer(Some(344), "Linear Dimension-10", flags))
                .expect("owned parameter with trailer flags");
        assert_eq!(parameter.owner_record_index(), Some(344));
        assert_eq!(parameter.kind(), DesignParameterKind::Dimension);
    }
    for flags in [[0, 0, 0, 1, 0], [1, 0, 0, 1, 0], [0, 0, 1, 1, 0], [0; 5]] {
        let parameter =
            parse_design_parameter_record(&with_trailer(None, "Linear Dimension-12", flags))
                .expect("owner-less dimension parameter with trailer flags");
        assert_eq!(parameter.owner_record_index(), None);
        assert_eq!(parameter.kind(), DesignParameterKind::Dimension);
    }
    assert!(parse_design_parameter_record(&with_trailer(
        Some(344),
        "AlongDistance",
        [0, 2, 0, 0, 0]
    ))
    .is_none());
    assert!(parse_design_parameter_record(&with_trailer(
        Some(344),
        "AlongDistance",
        [0, 0, 0, 0, 1]
    ))
    .is_none());
    assert!(parse_design_parameter_record(&with_trailer(None, "User Parameter", [0; 5])).is_none());
    let user =
        parse_design_parameter_record(&with_trailer(None, "User Parameter", [0, 0, 0, 1, 1]))
            .expect("document parameter with trailer flags");
    assert_eq!(user.kind(), DesignParameterKind::User);
}

/// The intermediate discriminated family stores discriminator 5 or 6.
#[test]
fn intermediate_family_admits_discriminators_five_and_six() {
    for (discriminator, admitted) in [(5u64, true), (6, true), (4, false)] {
        let mut frame = parameter_record(
            Some(77),
            "25.0 deg",
            "Angle Threshold",
            Some("deg"),
            "d1",
            0.436,
        );
        frame[22..30].copy_from_slice(&discriminator.to_le_bytes());
        let tail = frame.len() - 12;
        frame[tail + 2] = 17;
        assert_eq!(parse_design_parameter_record(&frame).is_some(), admitted);
    }
}

#[test]
fn discriminated_thread_length_parameter_recovers_its_owner() {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"286", 944);
    bytes.extend_from_slice(&[0; 11]);
    bytes.extend_from_slice(&5u64.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&18u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&945u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    lp_utf16(&mut bytes, "10.00 mm");
    bytes.extend_from_slice(&[0; 9]);
    lp_utf16(&mut bytes, "ThreadLength");
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, "mm");
    lp_utf16(&mut bytes, "d18");
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let parameter = parse_design_parameter_record(&bytes).expect("thread length parameter");
    assert_eq!(parameter.source_kind(), "ThreadLength");
    assert_eq!(parameter.owner_record_index(), Some(945));
    assert_eq!(parameter.evaluated_value().get(), 1.0);
}
