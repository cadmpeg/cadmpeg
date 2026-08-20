// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{report_code_count, strict_options, valid_global_fields};
use crate::loss::IgesLossCode;
use crate::test_support::{
    card, directory_card, fixed_ascii_with_global, parameter_card, point_file,
    point_file_with_global,
};
use crate::IgesCodec;

fn point_file_with_delimiters(parameter: char, record: char) -> Vec<u8> {
    let mut fields = valid_global_fields();
    fields[0] = format!("1H{parameter}");
    fields[1] = format!("1H{record}");
    let global = format!("{}{record}", fields.join(&parameter.to_string()));
    let mut bytes = fixed_ascii_with_global(global.as_bytes());
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["116", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["116", "0", "0", "1", "0", "", "", "POINT", "0"],
        2,
    ));
    bytes.extend(parameter_card(
        format!("116{parameter}1.0{parameter}2.0{parameter}3.0{record}").as_bytes(),
        1,
        1,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn fixed_ascii_with_global_chunks(chunks: &[&[u8]]) -> Vec<u8> {
    let mut bytes = card(b"original fixture", b'S', 1);
    let cards = chunks
        .iter()
        .flat_map(|chunk| chunk.chunks(72))
        .collect::<Vec<_>>();
    for (index, chunk) in cards.iter().enumerate() {
        bytes.extend(card(chunk, b'G', u32::try_from(index + 1).unwrap()));
    }
    bytes.extend(card(
        format!("S0000001G{:07}D0000000P0000000", cards.len()).as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn inspect_parses_alternate_delimiters_and_cross_card_hollerith() {
    let product = "p".repeat(70);
    let global = format!(
        "1H^^1H!^70H{product}^8Hpart.igs^7Hcadmpeg^3H0.1^32^38^6^308^15^0H^1.0^2^2HMM^1^1.0^15H20260714.000000^0.001^1000.0^6Hauthor^3Horg^11^0^0H^0H!"
    );
    let bytes = fixed_ascii_with_global(global.as_bytes());

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"parameter_delimiter=^".into()));
    assert!(summary.notes.contains(&"record_delimiter=!".into()));
    assert!(summary.notes.contains(&format!("sender_product={product}")));
    assert!(summary.notes.contains(&"iges_version=5.3".into()));
    assert!(summary.notes.contains(&"units=MM".into()));
}

#[test]
fn global_card_padding_is_ignored_outside_hollerith_values() {
    let bytes = fixed_ascii_with_global_chunks(&[
        b"1H,,1H;,7Hproduct,8Hpart.igs,",
        b"7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    ]);
    let (parsed, _) = crate::global::parse(&crate::card::scan(&bytes).unwrap()).unwrap();

    assert_eq!(parsed.sender_product().as_deref(), Some("product"));
    assert_eq!(parsed.native_file_name().as_deref(), Some("part.igs"));
}

#[test]
fn global_card_padding_does_not_remove_hollerith_payload_spaces() {
    let bytes = fixed_ascii_with_global_chunks(&[
        b"1H,,1H;,3Hab ",
        b",8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    ]);
    let (parsed, _) = crate::global::parse(&crate::card::scan(&bytes).unwrap()).unwrap();

    assert_eq!(parsed.sender_product().as_deref(), Some("ab "));
}

#[test]
fn global_hollerith_values_reject_non_printable_ascii() {
    for byte in [0x00, 0x1f, 0x7f, 0x80, 0xff] {
        let mut bytes = point_file();
        let product = bytes
            .windows(9)
            .position(|window| window == b"7Hproduct")
            .expect("sender product");
        bytes[product + 5] = byte;

        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(!result
            .ir()
            .source
            .as_ref()
            .unwrap()
            .attributes
            .contains_key("sender_product"));
        assert_eq!(result.ir().model.points.len(), 1, "{byte:#04x}");
        assert_eq!(
            report_code_count(result.report(), IgesLossCode::GlobalMetadataFieldUnusable),
            1,
            "{byte:#04x}"
        );
    }
}

#[test]
fn a_forbidden_delimiter_payload_still_refuses_the_file() {
    for field in [b"1H,".as_slice(), b"1H;".as_slice()] {
        let mut bytes = point_file();
        let position = bytes
            .windows(3)
            .position(|window| window == field)
            .expect("delimiter declaration");
        bytes[position + 2] = 0x01;

        assert!(
            matches!(
                IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()),
                Err(CodecError::Malformed(_))
            ),
            "{field:?}"
        );
    }
}

#[test]
fn a_twenty_seventh_global_field_decodes_with_the_noncanonical_framing_loss() {
    let mut fields = valid_global_fields();
    fields.push("0H".into());
    let mut global = fields.join(",");
    global.push(';');
    let bytes = point_file_with_global(global.as_bytes());

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::GlobalNoncanonicalFraming),
        1
    );

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(false))
        .unwrap_err();
    match error {
        CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
            loss_code,
            IgesLossCode::GlobalNoncanonicalFraming.kind().as_str()
        ),
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn a_prohibited_delimiter_declaration_is_honored_and_charges_noncanonical_framing() {
    for (parameter, record, expected) in [
        (',', ';', 0_usize),
        ('+', ';', 1),
        (',', 'D', 1),
        ('+', 'D', 2),
    ] {
        let bytes = point_file_with_delimiters(parameter, record);

        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();

        assert_eq!(
            result.ir().model.points.len(),
            1,
            "{parameter}{record}: {:#?}",
            result.report().losses
        );
        let position = &result.ir().model.points[0].position;
        assert_eq!(
            (position.x, position.y, position.z),
            (1.0, 2.0, 3.0),
            "{parameter}{record}"
        );
        assert_eq!(
            report_code_count(result.report(), IgesLossCode::GlobalNoncanonicalFraming),
            expected,
            "{parameter}{record}: {:#?}",
            result.report().losses
        );
    }
}

#[test]
fn omitted_delimiter_fields_select_the_specification_defaults() {
    for global in [
        b",,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;".as_slice(),
        b"1H,,,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;".as_slice(),
        b",1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;".as_slice(),
    ] {
        let (parsed, losses) =
            crate::global::parse(&crate::card::scan(&fixed_ascii_with_global(global)).unwrap())
                .unwrap();

        assert_eq!(parsed.parameter_delimiter, b',');
        assert_eq!(parsed.record_delimiter, b';');
        assert_eq!(parsed.sender_product().as_deref(), Some("product"));
        assert!(losses.is_empty(), "{losses:#?}");
    }
}
