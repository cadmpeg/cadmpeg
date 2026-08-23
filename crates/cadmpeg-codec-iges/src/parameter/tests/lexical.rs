// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::super::{tokenize, ParameterDefect, TokenValue, TokenizeFailure};
use crate::global::Dialect;
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

#[test]
fn numeric_parameter_and_delimiter_must_share_a_card() {
    let mut bytes = b"116,".to_vec();
    bytes.extend(std::iter::repeat_n(b'0', 59));
    bytes.push(b'1');
    bytes.extend_from_slice(b",2,3,0;");

    assert!(matches!(
        tokenize(&bytes, &[64], b',', b';', Dialect::V5_3, None),
        Err(TokenizeFailure::Defect(
            ParameterDefect::NumericCrossesCard,
            4
        ))
    ));
}

#[test]
fn a_zero_hollerith_count_is_not_a_null_string() {
    assert!(matches!(
        tokenize(b"116,0H,2,3,0;", &[64], b',', b';', Dialect::V5_3, None),
        Err(TokenizeFailure::Defect(
            ParameterDefect::HollerithCountZero,
            4
        ))
    ));

    let error = super::super::layout_parameter_cards(b"116,0H;").unwrap_err();
    assert!(error.to_string().contains("count must be positive"));
}

#[test]
fn numeric_fields_may_have_leading_but_not_embedded_or_trailing_blanks() {
    let (tokens, _) = tokenize(b"116, 1,2,3,0;", &[64], b',', b';', Dialect::V5_3, None)
        .unwrap_or_else(|_| panic!("leading blanks are ignored"));
    assert_eq!(tokens[1].value, TokenValue::Integer(1));

    for field in [b"1  ".as_slice(), b"1 2".as_slice()] {
        let bytes = [b"116,".as_slice(), field, b",2,3,0;".as_slice()].concat();
        assert!(matches!(
            tokenize(&bytes, &[64], b',', b';', Dialect::V5_3, None),
            Err(TokenizeFailure::Defect(
                ParameterDefect::NumericContainsBlanks,
                4
            ))
        ));
    }
}

#[test]
fn a_hollerith_payload_may_cross_a_card_but_its_header_may_not() {
    let mut payload_crosses = b"116,".to_vec();
    payload_crosses.extend(std::iter::repeat_n(b'0', 56));
    payload_crosses.push(b'1');
    payload_crosses.extend_from_slice(b",4Habcd,;");
    let (tokens, _) = tokenize(&payload_crosses, &[64], b',', b';', Dialect::V5_3, None)
        .unwrap_or_else(|_| panic!("a Hollerith payload may cross its card boundary"));
    assert!(matches!(tokens[2].value, TokenValue::String(ref value) if value == b"abcd"));

    let mut header_crosses = b"116,".to_vec();
    header_crosses.extend(std::iter::repeat_n(b'0', 57));
    header_crosses.push(b'1');
    header_crosses.extend_from_slice(b",4Habcd,;");
    assert!(matches!(
        tokenize(&header_crosses, &[64], b',', b';', Dialect::V5_3, None),
        Err(TokenizeFailure::Defect(
            ParameterDefect::HollerithHeaderCrossesCard,
            63
        ))
    ));
}

#[test]
fn hollerith_string_bytes_follow_the_declared_dialect() {
    let bytes = b"116,3Ha\0c,2,3,0;";
    let (tokens, _) = tokenize(bytes, &[64], b',', b';', Dialect::V4_0, None)
        .unwrap_or_else(|_| panic!("IGES 4.0 permits ASCII control bytes in strings"));
    assert!(matches!(
        tokens[1].value,
        TokenValue::String(ref value) if value == b"a\0c"
    ));

    assert!(matches!(
        tokenize(bytes, &[64], b',', b';', Dialect::V5_3, None),
        Err(TokenizeFailure::Defect(
            ParameterDefect::HollerithForbiddenByte,
            4
        ))
    ));
}

#[test]
fn generated_parameter_layout_keeps_headers_and_numeric_delimiters_legal() {
    let mut payload = b"116,70H".to_vec();
    payload.extend(std::iter::repeat_n(b'x', 70));
    payload.extend_from_slice(b",1,;");
    let cards = super::super::layout_parameter_cards(&payload).unwrap();
    assert_eq!(cards.len(), 2);
    assert_eq!(&cards[0][4..7], b"70H");

    let mut numeric = b"116,".to_vec();
    numeric.extend(std::iter::repeat_n(b'0', 58));
    numeric.extend_from_slice(b",2,;");
    let cards = super::super::layout_parameter_cards(&numeric).unwrap();
    assert_eq!(cards.len(), 2);
    assert_eq!(&cards[1][..2], b"2,");
}

#[test]
fn a_split_numeric_parameter_is_quarantined_in_the_decode() {
    let mut x = String::with_capacity(60);
    x.extend(std::iter::repeat_n('0', 59));
    x.push('1');
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_raw_parameters(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "SPLIT".into(),
                status: "00010000",
                parameters: format!("116,{x},2,3,0;"),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.points.is_empty());
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(
        result.report().losses[0].code,
        IgesLossCode::ParameterDataQuarantined.kind()
    );
    assert!(result.report().losses[0]
        .message
        .contains("numeric field or its delimiter crosses a card boundary"));
}
