// SPDX-License-Identifier: Apache-2.0
//! Part 21 lexer tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

#[test]
fn quoted_string_is_one_byte_payload() {
    assert_eq!(
        crate::test_support::with_service_context(b"'abc'", crate::test_support::lex)
            .expect("one string token")[0]
            .kind,
        super::TokenKind::String(b"abc".to_vec())
    );
}

#[test]
fn lexer_decodes_binary_literals_and_rejects_invalid_bit_boundaries() {
    use crate::lex::{BinaryValue, TokenKind};

    assert_eq!(
        crate::test_support::with_service_context(b"\"0A1F\"", crate::test_support::lex).unwrap()
            [0]
        .kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 4,
            data: vec![0xa1, 0xf0].into_boxed_slice(),
        })
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\"17E\"", crate::test_support::lex).unwrap()[0]
            .kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 1,
            data: vec![0x7e].into_boxed_slice(),
        })
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\"0\\N\\A\"", crate::test_support::lex)
            .unwrap()[0]
            .kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 4,
            data: vec![0xa0].into_boxed_slice(),
        })
    );
    for invalid in [b"\"\"".as_slice(), b"\"4FF\"", b"\"17F\"", b"\"3A7\""] {
        assert!(
            crate::test_support::with_service_context(invalid, crate::test_support::lex).is_err(),
            "accepted {invalid:?}"
        );
    }
}

#[test]
fn lexer_ignores_controls_inside_tokens_and_print_controls_between_tokens() {
    use crate::lex::TokenKind;

    assert_eq!(
        crate::test_support::with_service_context(b"END-ISO-\n10303-21;", crate::test_support::lex)
            .unwrap()[0]
            .kind,
        TokenKind::Name("END-ISO-10303-21".into())
    );
    assert_eq!(
        crate::test_support::with_service_context(b"#\r\n001", crate::test_support::lex).unwrap()
            [0]
        .kind,
        TokenKind::Instance(1)
    );
    assert_eq!(
        crate::test_support::with_service_context(b"1\n.5", crate::test_support::lex).unwrap()[0]
            .kind,
        TokenKind::Real(cadmpeg_ir::scalar::FiniteReal::new(1.5).expect("finite fixture"))
    );

    let tokens = crate::test_support::with_service_context(b"1\\N\\2", crate::test_support::lex)
        .expect("print control separator");
    assert_eq!(tokens.len(), 2);
    assert!(matches!(tokens[0].kind, TokenKind::Integer(1)));
    assert!(matches!(tokens[1].kind, TokenKind::Integer(2)));
    let error = crate::test_support::with_service_context(b"<a\\N\\b>", crate::test_support::lex)
        .expect_err("resource print control");
    assert!(error.message.contains("resource"));
}

#[test]
fn lexer_ignores_controls_inside_escaped_literals_and_directives() {
    use crate::lex::{BinaryValue, TokenKind};

    let token = crate::test_support::with_service_context(b"'it'\x01''", crate::test_support::lex)
        .expect("apostrophe escape with ignored control")[0]
        .kind
        .clone();
    let TokenKind::String(bytes) = token else {
        panic!("expected string token");
    };
    assert_eq!(
        crate::strings::decode(
            &bytes,
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "it'"
    );

    let token =
        crate::test_support::with_service_context(b"'a\\\x01N\x02\\b'", crate::test_support::lex)
            .expect("string print control with ignored controls")[0]
            .kind
            .clone();
    let TokenKind::String(bytes) = token else {
        panic!("expected string token");
    };
    assert_eq!(
        crate::strings::decode(
            &bytes,
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1
        )
        .unwrap(),
        "ab"
    );

    assert_eq!(
        crate::test_support::with_service_context(b"\"0\\\x01F\x02\\A\"", crate::test_support::lex)
            .unwrap()[0]
            .kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 4,
            data: vec![0xa0].into_boxed_slice(),
        })
    );

    let tokens =
        crate::test_support::with_service_context(b"1\\\x01N\x02\\2", crate::test_support::lex)
            .expect("print control separator with ignored controls");
    assert_eq!(tokens.len(), 2);
    assert!(matches!(tokens[0].kind, TokenKind::Integer(1)));
    assert!(matches!(tokens[1].kind, TokenKind::Integer(2)));

    let error =
        crate::test_support::with_service_context(b"<a\\\x01N\x02\\b>", crate::test_support::lex)
            .expect_err("resource print control");
    assert!(error.message.contains("resource"));
}

#[test]
fn lexer_accepts_exponent_before_trailing_decimal_point() {
    let token = crate::test_support::with_service_context(b"6E-16.", crate::test_support::lex)
        .expect("real with trailing decimal point")[0]
        .kind
        .clone();
    let crate::lex::TokenKind::Real(value) = token else {
        panic!("expected a real token");
    };
    assert!(value.get().abs() < 1e-15);
}

#[test]
fn lexer_bounds_oversized_strings_before_string_admission() {
    let mut source = Vec::with_capacity(32_770);
    source.push(b'\'');
    source.extend(std::iter::repeat_n(b'x', 32_768));
    source.push(b'\'');
    let tokens = crate::test_support::with_service_context(&source, crate::test_support::lex)
        .expect("closed string has bounded framing");
    let crate::lex::TokenKind::String(raw) = &tokens[0].kind else {
        panic!("string token");
    };
    let error = crate::strings::decode(
        raw,
        crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
    )
    .expect_err("oversized string remains inadmissible as a string value");
    assert!(error.to_string().contains("maximum stored length"));
}

#[test]
fn lexer_accepts_underscores_and_rejects_hyphens_in_enumeration_names() {
    assert_eq!(
        crate::test_support::with_service_context(b"._USER2.", crate::test_support::lex).unwrap()
            [0]
        .kind,
        crate::lex::TokenKind::Enumeration("_USER2".into())
    );
    assert!(
        crate::test_support::with_service_context(b".USER-DEFINED.", crate::test_support::lex)
            .is_err()
    );
}

#[test]
fn lexer_distinguishes_entity_and_value_occurrence_names() {
    use crate::lex::TokenKind;

    let tokens = crate::test_support::with_service_context(
        b"#001 @002 #pi_value @_LIMIT",
        crate::test_support::lex,
    )
    .expect("occurrence names");
    assert_eq!(tokens[0].kind, TokenKind::Instance(1));
    assert_eq!(tokens[1].kind, TokenKind::ValueInstance(2));
    assert_eq!(tokens[2].kind, TokenKind::ConstantEntity("PI_VALUE".into()));
    assert_eq!(tokens[3].kind, TokenKind::ConstantValue("_LIMIT".into()));

    for input in [b"#0".as_slice(), b"@00"] {
        let error = crate::test_support::with_service_context(input, crate::test_support::lex)
            .expect_err("zero occurrence name");
        assert_eq!(error.message, "instance name must not be zero");
    }
}

#[test]
fn real_lexeme_rejects_binary64_overflow() {
    for source in [b"1.E9999".as_slice(), b"-1.E9999".as_slice()] {
        let error = crate::test_support::with_service_context(source, crate::test_support::lex)
            .expect_err("overflow cannot enter a real token");
        assert!(error.message.contains("finite binary64 range"));
        assert!(matches!(
            error.into_codec_error(),
            cadmpeg_core::CodecError::Malformed(_)
        ));
    }
}

#[test]
fn real_lexeme_preserves_finite_bits() {
    for number in [-0.0, f64::MAX, f64::from_bits(1)] {
        let source = format!("{number:.17e}");
        let tokens =
            crate::test_support::with_service_context(source.as_bytes(), crate::test_support::lex)
                .expect("finite real token");
        let crate::lex::TokenKind::Real(real) = tokens[0].kind else {
            panic!("real token required")
        };
        assert_eq!(real.get().to_bits(), number.to_bits());
    }
}
