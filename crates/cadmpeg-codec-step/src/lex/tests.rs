// SPDX-License-Identifier: Apache-2.0
//! Part 21 lexer tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn lex_under_policy(
    input: &[u8],
    policy: DecodePolicy,
    transient: bool,
) -> Result<super::TokenKind, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)?;
    let mut lexer = super::Lexer::new(input);
    lexer.set_context(Some(&ctx));
    if transient {
        lexer.set_transient_literals();
    }
    let token = lexer
        .next_token()
        .map_err(super::LexError::into_codec_error)?;
    Ok(token.expect("nonempty token input").kind)
}

#[test]
fn binary_lexeme_reserves_temporary_digits_before_allocation() {
    let input = b"\"0A1F2\"";
    let service = DecodePolicy::service();
    assert!(lex_under_policy(input, service, false).is_ok());
    let mut limited = service;
    limited.limits.max_materialized_bytes = 4;
    let error = lex_under_policy(input, limited, false)
        .expect_err("five digits exceed four temporary bytes");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_binary_lexeme_temp")
    );
}

#[test]
fn binary_lexeme_charges_packed_bytes_before_retention() {
    let input = b"\"0A1F2\"";
    let service = DecodePolicy::service();
    assert!(lex_under_policy(input, service, false).is_ok());
    let mut limited = service;
    limited.limits.max_retained_bytes = 1;
    let error = lex_under_policy(input, limited, false)
        .expect_err("two packed bytes exceed one retained byte");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_binary_lexeme_retained")
    );
}

#[test]
fn uri_lexeme_reserves_temporary_bytes_before_allocation() {
    let input = b"<part/path>";
    let service = DecodePolicy::service();
    assert!(lex_under_policy(input, service, false).is_ok());
    let mut limited = service;
    limited.limits.max_materialized_bytes = 8;
    let error = lex_under_policy(input, limited, false)
        .expect_err("nine URI bytes exceed eight temporary bytes");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_uri_lexeme_temp")
    );
}

#[test]
fn uri_lexeme_charges_bytes_before_retention() {
    let input = b"<part/path>";
    let service = DecodePolicy::service();
    assert!(lex_under_policy(input, service, false).is_ok());
    let mut limited = service;
    limited.limits.max_retained_bytes = 8;
    let error = lex_under_policy(input, limited, false)
        .expect_err("nine URI bytes exceed eight retained bytes");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_uri_lexeme_retained")
    );
}

#[test]
fn transient_binary_lexeme_reserves_packed_bytes_without_retention() {
    let input = b"\"0A1F2\"";
    let mut service = DecodePolicy::service();
    service.limits.max_retained_bytes = 0;
    assert!(lex_under_policy(input, service, true).is_ok());
    let mut limited = service;
    limited.limits.max_materialized_bytes = 6;
    let error = lex_under_policy(input, limited, true)
        .expect_err("five digits plus two packed bytes exceed six temporary bytes");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_binary_packed_temp")
    );
}

#[test]
fn transient_uri_lexeme_uses_only_temporary_bytes() {
    let input = b"<part/path>";
    let mut service = DecodePolicy::service();
    service.limits.max_retained_bytes = 0;
    assert!(lex_under_policy(input, service, true).is_ok());
    let mut limited = service;
    limited.limits.max_materialized_bytes = 8;
    let error = lex_under_policy(input, limited, true)
        .expect_err("nine URI bytes exceed eight temporary bytes");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_uri_lexeme_temp")
    );
}

#[test]
fn lexer_decodes_binary_literals_and_rejects_invalid_bit_boundaries() {
    use crate::lex::{lex, BinaryValue, TokenKind};

    assert_eq!(
        lex(b"\"0A1F\"").unwrap()[0].kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 4,
            data: vec![0xa1, 0xf0].into_boxed_slice(),
        })
    );
    assert_eq!(
        lex(b"\"17E\"").unwrap()[0].kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 1,
            data: vec![0x7e].into_boxed_slice(),
        })
    );
    assert_eq!(
        lex(b"\"0\\N\\A\"").unwrap()[0].kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 4,
            data: vec![0xa0].into_boxed_slice(),
        })
    );
    for invalid in [b"\"\"".as_slice(), b"\"4FF\"", b"\"17F\"", b"\"3A7\""] {
        assert!(lex(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn lexer_ignores_controls_inside_tokens_and_print_controls_between_tokens() {
    use crate::lex::{lex, TokenKind};

    assert_eq!(
        lex(b"END-ISO-\n10303-21;").unwrap()[0].kind,
        TokenKind::Name("END-ISO-10303-21".into())
    );
    assert_eq!(lex(b"#\r\n001").unwrap()[0].kind, TokenKind::Instance(1));
    assert_eq!(lex(b"1\n.5").unwrap()[0].kind, TokenKind::Real(1.5));

    let tokens = lex(b"1\\N\\2").expect("print control separator");
    assert_eq!(tokens.len(), 2);
    assert!(matches!(tokens[0].kind, TokenKind::Integer(1)));
    assert!(matches!(tokens[1].kind, TokenKind::Integer(2)));
    let error = lex(b"<a\\N\\b>").expect_err("resource print control");
    assert!(error.message.contains("resource"));
}

#[test]
fn lexer_ignores_controls_inside_escaped_literals_and_directives() {
    use crate::lex::{lex, BinaryValue, TokenKind};

    let token = lex(b"'it'\x01''").expect("apostrophe escape with ignored control")[0]
        .kind
        .clone();
    let TokenKind::String(bytes) = token else {
        panic!("expected string token");
    };
    assert_eq!(crate::strings::decode(&bytes).unwrap(), "it'");

    let token = lex(b"'a\\\x01N\x02\\b'").expect("string print control with ignored controls")[0]
        .kind
        .clone();
    let TokenKind::String(bytes) = token else {
        panic!("expected string token");
    };
    assert_eq!(crate::strings::decode(&bytes).unwrap(), "ab");

    assert_eq!(
        lex(b"\"0\\\x01F\x02\\A\"").unwrap()[0].kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 4,
            data: vec![0xa0].into_boxed_slice(),
        })
    );

    let tokens = lex(b"1\\\x01N\x02\\2").expect("print control separator with ignored controls");
    assert_eq!(tokens.len(), 2);
    assert!(matches!(tokens[0].kind, TokenKind::Integer(1)));
    assert!(matches!(tokens[1].kind, TokenKind::Integer(2)));

    let error = lex(b"<a\\\x01N\x02\\b>").expect_err("resource print control");
    assert!(error.message.contains("resource"));
}

#[test]
fn lexer_accepts_exponent_before_trailing_decimal_point() {
    let token = crate::lex::lex(b"6E-16.").expect("real with trailing decimal point")[0]
        .kind
        .clone();
    let crate::lex::TokenKind::Real(value) = token else {
        panic!("expected a real token");
    };
    assert!(value.abs() < 1e-15);
}

#[test]
fn lexer_rejects_strings_that_exceed_the_stored_length_limit() {
    let mut source = Vec::with_capacity(32_770);
    source.push(b'\'');
    source.extend(std::iter::repeat_n(b'x', 32_768));
    source.push(b'\'');
    let error = crate::lex::lex(&source).expect_err("oversized string");
    assert!(error.message.contains("maximum stored length"));
}

#[test]
fn lexer_accepts_underscores_and_rejects_hyphens_in_enumeration_names() {
    assert_eq!(
        crate::lex::lex(b"._USER2.").unwrap()[0].kind,
        crate::lex::TokenKind::Enumeration("_USER2".into())
    );
    assert!(crate::lex::lex(b".USER-DEFINED.").is_err());
}

#[test]
fn lexer_distinguishes_entity_and_value_occurrence_names() {
    use crate::lex::{lex, TokenKind};

    let tokens = lex(b"#001 @002 #pi_value @_LIMIT").expect("occurrence names");
    assert_eq!(tokens[0].kind, TokenKind::Instance(1));
    assert_eq!(tokens[1].kind, TokenKind::ValueInstance(2));
    assert_eq!(tokens[2].kind, TokenKind::ConstantEntity("PI_VALUE".into()));
    assert_eq!(tokens[3].kind, TokenKind::ConstantValue("_LIMIT".into()));

    for input in [b"#0".as_slice(), b"@00"] {
        let error = lex(input).expect_err("zero occurrence name");
        assert_eq!(error.message, "instance name must not be zero");
    }
}
