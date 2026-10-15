// SPDX-License-Identifier: Apache-2.0
//! Part 21 string codec tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::StepLossCode;
use crate::test_support::exchange::decode_inline;
use crate::StepCodec;

#[test]
pub(crate) fn string_codec_decodes_all_part21_escape_forms_and_round_trips_unicode() {
    use crate::parse::implementation_level::ImplementationLevel;
    use crate::strings::encode;

    assert_eq!(
        crate::strings::decode(
            b"it''s",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "it's"
    );
    assert_eq!(
        crate::strings::decode(
            b"a\\\\b",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "a\\b"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\X\\E9",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "é"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\X2\\03A9\\X0\\",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "Ω"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\X4\\0001F642\\X0\\",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "🙂"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\S\\D",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "Ä"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\PA\\\\S\\D",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "Ä"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\PB\\\\S\\A",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "Á"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\PC\\\\S\\!",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "Ħ"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\PD\\\\S\\!",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "Ą"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\PE\\\\S\\0",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "А"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\PF\\\\S\\G",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "ا"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\PG\\\\S\\A",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "Α"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\PH\\\\S\\`",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "א"
    );
    assert_eq!(
        crate::strings::decode(
            b"\\PI\\\\S\\P",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "Ğ"
    );
    assert_eq!(
        crate::strings::decode(
            b"line\\N\\text\\F\\tail",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "linetexttail"
    );
    assert_eq!(
        crate::strings::decode(b"caf\xC3\xA9", ImplementationLevel::Edition3Class1).unwrap(),
        "café"
    );
    assert_eq!(
        crate::strings::decode(
            b"caf\xC3\xA9\\X2\\03A9\\X0\\",
            ImplementationLevel::Edition3Class1
        )
        .unwrap(),
        "caféΩ"
    );
    let error =
        crate::strings::decode(b"caf\xE9", ImplementationLevel::Edition3Class1).unwrap_err();
    assert_eq!(error.message, "invalid UTF-8 direct string bytes");

    for text in ["ASCII", "it's \\ quoted", "café Ω 🙂"] {
        assert_eq!(
            crate::strings::decode(
                encode(text).as_bytes(),
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
            )
            .unwrap(),
            text
        );
    }
}

#[test]
fn writer_and_lexer_preserve_apostrophes_and_backslashes_once() {
    use crate::lex::TokenKind;

    let source = "O'Brien \\ fixtures";
    let encoded = crate::writer::string(source);
    let tokens =
        crate::test_support::with_service_context(encoded.as_bytes(), crate::test_support::lex)
            .expect("lex encoded string");
    let TokenKind::String(bytes) = &tokens[0].kind else {
        panic!("encoded text did not lex as a string")
    };
    assert_eq!(
        crate::strings::decode(
            bytes,
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        source
    );
    assert!(encoded.contains("O''Brien"));
    assert!(encoded.contains("\\\\"));
}

#[test]
fn wide_escape_streams_surrogate_pairs_and_rejects_isolated_surrogates() {
    assert_eq!(
        crate::strings::decode(
            b"\\X2\\D83DDE42\\X0\\",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .expect("valid UTF-16 surrogate pair"),
        "🙂"
    );
    let error = crate::strings::decode(
        b"\\X2\\D83D\\X0\\",
        crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
    )
    .expect_err("isolated high surrogate");
    assert_eq!(error.message, "wide escape contains an isolated surrogate");
    let error = crate::strings::decode(
        b"\\X2\\DE42\\X0\\",
        crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
    )
    .expect_err("isolated low surrogate");
    assert_eq!(error.message, "wide escape contains an isolated surrogate");
}

#[test]
fn decoded_string_reports_a_malformed_byte_escape() {
    let malformed = crate::strings::decode(
        b"\\X\\GG",
        crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
    );
    assert!(matches!(
        malformed,
        Err(error) if error.message == "byte escape contains non-hexadecimal digits"
    ));
}

#[test]
fn invalid_step_string_escape_is_reported_as_metadata_loss() {
    let decoded = decode_inline(r"#1=PRODUCT('\X\GG','valid name','',());");

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::MetadataStringInvalid.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Warning
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
