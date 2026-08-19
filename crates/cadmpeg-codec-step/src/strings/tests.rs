// SPDX-License-Identifier: Apache-2.0
//! Part 21 string codec tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::StepLossCode;
use crate::test_support::decode_inline;
use crate::StepCodec;

#[test]
pub(crate) fn string_codec_decodes_all_part21_escape_forms_and_round_trips_unicode() {
    use crate::strings::{decode, decode_utf8, encode};

    assert_eq!(decode(b"it''s").unwrap(), "it's");
    assert_eq!(decode(b"a\\\\b").unwrap(), "a\\b");
    assert_eq!(decode(b"\\X\\E9").unwrap(), "é");
    assert_eq!(decode(b"\\X2\\03A9\\X0\\").unwrap(), "Ω");
    assert_eq!(decode(b"\\X4\\0001F642\\X0\\").unwrap(), "🙂");
    assert_eq!(decode(b"\\S\\D").unwrap(), "Ä");
    assert_eq!(decode(b"\\PA\\\\S\\D").unwrap(), "Ä");
    assert_eq!(decode(b"\\PB\\\\S\\A").unwrap(), "Á");
    assert_eq!(decode(b"\\PC\\\\S\\!").unwrap(), "Ħ");
    assert_eq!(decode(b"\\PD\\\\S\\!").unwrap(), "Ą");
    assert_eq!(decode(b"\\PE\\\\S\\0").unwrap(), "А");
    assert_eq!(decode(b"\\PF\\\\S\\G").unwrap(), "ا");
    assert_eq!(decode(b"\\PG\\\\S\\A").unwrap(), "Α");
    assert_eq!(decode(b"\\PH\\\\S\\`").unwrap(), "א");
    assert_eq!(decode(b"\\PI\\\\S\\P").unwrap(), "Ğ");
    assert_eq!(decode(b"line\\N\\text\\F\\tail").unwrap(), "linetexttail");
    assert_eq!(decode_utf8(b"caf\xC3\xA9").unwrap(), "café");
    assert_eq!(
        decode_utf8(b"caf\xC3\xA9\\X2\\03A9\\X0\\").unwrap(),
        "caféΩ"
    );
    assert_eq!(
        decode_utf8(b"caf\xE9").unwrap_err().message,
        "invalid UTF-8 direct string bytes"
    );

    for text in ["ASCII", "it's \\ quoted", "café Ω 🙂"] {
        assert_eq!(decode(encode(text).as_bytes()).unwrap(), text);
    }
}

#[test]
fn writer_and_lexer_preserve_apostrophes_and_backslashes_once() {
    use crate::lex::{lex, TokenKind};

    let source = "O'Brien \\ fixtures";
    let encoded = crate::writer::string(source);
    let tokens = lex(encoded.as_bytes()).expect("lex encoded string");
    let TokenKind::String(bytes) = &tokens[0].kind else {
        panic!("encoded text did not lex as a string")
    };
    assert_eq!(crate::strings::decode(bytes).unwrap(), source);
    assert!(encoded.contains("O''Brien"));
    assert!(encoded.contains("\\\\"));
}

#[test]
fn invalid_step_string_escape_is_reported_as_metadata_loss() {
    let decoded = decode_inline(r"#1=PRODUCT('\X\GG','valid name','',());");

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::MetadataStringInvalid.kind()
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss
                .message
                .contains("STEP record #1 has an invalid product identifier string")
    }));
}

#[test]
fn edition_three_direct_utf8_text_uses_the_file_description_level() {
    let mut source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('test'),'4;1');\nFILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=PRODUCT('P\xC3\xA9','N\xC3\xB8','',());\nENDSEC;\nEND-ISO-10303-21;\n".to_vec();
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(&mut source), &DecodeOptions::default())
        .expect("decode edition-three UTF-8 product");
    let product = decoded
        .ir()
        .model
        .product_definitions
        .first()
        .expect("product definition");
    assert_eq!(product.source_name.as_deref(), Some("Nø"));
    assert_eq!(product.part_number.as_deref(), Some("Pé"));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message.contains("invalid product identifier string")
            || loss.message.contains("invalid product name string")
    }));
}

#[test]
fn edition_three_utf8_and_legacy_escape_fixtures_keep_the_same_text() {
    let mut edition_three = Cursor::new(&include_bytes!("tests/data/el01_edition3_utf8.p21")[..]);
    let decoded_edition_three = StepCodec::default()
        .decode(&mut edition_three, &DecodeOptions::default())
        .expect("decode edition-three UTF-8 fixture");
    let product = decoded_edition_three
        .ir()
        .model
        .product_definitions
        .first()
        .expect("edition-three product");
    assert_eq!(product.part_number.as_deref(), Some("Pé"));
    assert_eq!(product.source_name.as_deref(), Some("Nø"));

    let mut legacy = Cursor::new(&include_bytes!("tests/data/el01_legacy_escaped.p21")[..]);
    let decoded_legacy = StepCodec::default()
        .decode(&mut legacy, &DecodeOptions::default())
        .expect("decode legacy escaped fixture");
    let product = decoded_legacy
        .ir()
        .model
        .product_definitions
        .first()
        .expect("legacy product");
    assert_eq!(product.part_number.as_deref(), Some("Pé"));
    assert_eq!(product.source_name.as_deref(), Some("Nø"));
}

#[test]
fn legacy_direct_single_byte_text_uses_cadir_iso_8859_1_salvage() {
    // `0xE9` is outside the legacy direct repertoire; this pins the documented
    // recovery path for malformed historical input.
    let mut source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('test'),'3;1');\nFILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=PRODUCT('P\xE9','N','',());\nENDSEC;\nEND-ISO-10303-21;\n".to_vec();
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(&mut source), &DecodeOptions::default())
        .expect("decode legacy ISO-8859-1 product");
    let product = decoded
        .ir()
        .model
        .product_definitions
        .first()
        .expect("product definition");
    assert_eq!(product.part_number.as_deref(), Some("Pé"));
}
