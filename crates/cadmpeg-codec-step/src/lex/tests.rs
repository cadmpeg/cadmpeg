// SPDX-License-Identifier: Apache-2.0
//! Part 21 lexer tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn binary_value_copy_refuses_collection_limit() {
    let value = super::BinaryValue {
        unused_bits: 0,
        data: vec![0x12, 0x34].into_boxed_slice(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        value.try_clone_for_decode(&ctx, "step_binary_value_copy"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_binary_value_copy"
    ));
}

fn lex_under_policy(
    input: &[u8],
    policy: DecodePolicy,
    transient: bool,
) -> Result<super::TokenKind, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)?;
    // The caller owns all transient literal buffers in one scoped reservation.
    let mut transient_storage = ctx.reserve_scoped(0, "test transient literal storage")?;
    let read = || {
        let mut lexer = super::Lexer::new(input, &ctx);
        if transient {
            lexer.set_transient_literals();
        }
        let token = lexer
            .next_token()
            .map_err(super::LexError::into_codec_error)?;
        Ok(token.expect("nonempty token input").kind)
    };
    if transient {
        transient_storage.with_storage(read)
    } else {
        read()
    }
}

#[test]
fn normalized_name_refuses_retained_byte_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let error = lex_under_policy(b"ABC", policy, false)
        .expect_err("three name bytes exceed two retained bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "step_lex_normalized_retained"
    ));
}

#[test]
fn normalized_number_refuses_temporary_byte_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 2;
    let error = lex_under_policy(b"123", policy, false)
        .expect_err("three number bytes exceed two temporary bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "step_lex_normalized_temp"
    ));
}

#[test]
fn quoted_string_refuses_collection_item_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let error = lex_under_policy(b"'abc'", policy, false)
        .expect_err("three string bytes exceed two collection items");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_string_lexeme_items"
    ));
}

#[test]
fn binary_packed_bytes_refuse_collection_limit() {
    let input = b"\"0A1F2\"";
    let service = DecodePolicy::service();
    assert!(lex_under_policy(input, service, false).is_ok());
    // Two packed bytes; scanning hexadecimal digits allocates no collection slots.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems, "step_binary_packed_bytes", |cap| {
            let mut limited = service;
            limited.limits.max_collection_items = cap;
            lex_under_policy(input, limited, false)
        },
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_binary_packed_bytes"
    ));
}

#[test]
fn uri_lexeme_bytes_refuse_collection_limit() {
    let input = b"<part/path>";
    let service = DecodePolicy::service();
    assert!(lex_under_policy(input, service, false).is_ok());
    let mut limited = service;
    limited.limits.max_collection_items = 8;
    let error = lex_under_policy(input, limited, false)
        .expect_err("nine URI bytes exceed eight collection items");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_uri_lexeme_bytes"
    ));
}

#[test]
fn binary_lexeme_charges_packed_bytes_before_retention() {
    let input = b"\"0A1F2\"";
    let service = DecodePolicy::service();
    assert!(lex_under_policy(input, service, false).is_ok());
    // Two retained packed bytes; the hexadecimal digits have no retained storage.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes, "step_binary_lexeme_retained", |cap| {
            let mut limited = service;
            limited.limits.max_retained_bytes = cap;
            lex_under_policy(input, limited, false)
        },
    );
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
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_uri_lexeme_bytes")
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
    // Two live packed bytes; no digit reservation or duplicate slot storage.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes, "step_binary_packed_temp", |cap| {
            let mut limited = service;
            limited.limits.max_materialized_bytes = cap;
            lex_under_policy(input, limited, true)
        },
    );
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
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_uri_lexeme_bytes")
    );
}

#[test]
fn lexer_decodes_binary_literals_and_rejects_invalid_bit_boundaries() {
    use crate::lex::{BinaryValue, TokenKind};

    assert_eq!(
        crate::test_support::with_service_context(b"\"0A1F\"", crate::lex::lex_with_context)
            .unwrap()[0]
            .kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 4,
            data: vec![0xa1, 0xf0].into_boxed_slice(),
        })
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\"17E\"", crate::lex::lex_with_context)
            .unwrap()[0]
            .kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 1,
            data: vec![0x7e].into_boxed_slice(),
        })
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\"0\\N\\A\"", crate::lex::lex_with_context)
            .unwrap()[0]
            .kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 4,
            data: vec![0xa0].into_boxed_slice(),
        })
    );
    for invalid in [b"\"\"".as_slice(), b"\"4FF\"", b"\"17F\"", b"\"3A7\""] {
        assert!(
            crate::test_support::with_service_context(invalid, crate::lex::lex_with_context)
                .is_err(),
            "accepted {invalid:?}"
        );
    }
}

#[test]
fn lexer_ignores_controls_inside_tokens_and_print_controls_between_tokens() {
    use crate::lex::TokenKind;

    assert_eq!(
        crate::test_support::with_service_context(
            b"END-ISO-\n10303-21;",
            crate::lex::lex_with_context
        )
        .unwrap()[0]
            .kind,
        TokenKind::Name("END-ISO-10303-21".into())
    );
    assert_eq!(
        crate::test_support::with_service_context(b"#\r\n001", crate::lex::lex_with_context)
            .unwrap()[0]
            .kind,
        TokenKind::Instance(1)
    );
    assert_eq!(
        crate::test_support::with_service_context(b"1\n.5", crate::lex::lex_with_context).unwrap()
            [0]
        .kind,
        TokenKind::Real(cadmpeg_ir::scalar::FiniteReal::new(1.5).expect("finite fixture"))
    );

    let tokens =
        crate::test_support::with_service_context(b"1\\N\\2", crate::lex::lex_with_context)
            .expect("print control separator");
    assert_eq!(tokens.len(), 2);
    assert!(matches!(tokens[0].kind, TokenKind::Integer(1)));
    assert!(matches!(tokens[1].kind, TokenKind::Integer(2)));
    let error =
        crate::test_support::with_service_context(b"<a\\N\\b>", crate::lex::lex_with_context)
            .expect_err("resource print control");
    assert!(error.message.contains("resource"));
}

#[test]
fn lexer_ignores_controls_inside_escaped_literals_and_directives() {
    use crate::lex::{BinaryValue, TokenKind};

    let token =
        crate::test_support::with_service_context(b"'it'\x01''", crate::lex::lex_with_context)
            .expect("apostrophe escape with ignored control")[0]
            .kind
            .clone();
    let TokenKind::String(bytes) = token else {
        panic!("expected string token");
    };
    assert_eq!(
        crate::test_support::with_service_context(&bytes, |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "it'"
    );

    let token = crate::test_support::with_service_context(
        b"'a\\\x01N\x02\\b'",
        crate::lex::lex_with_context,
    )
    .expect("string print control with ignored controls")[0]
        .kind
        .clone();
    let TokenKind::String(bytes) = token else {
        panic!("expected string token");
    };
    assert_eq!(
        crate::test_support::with_service_context(&bytes, |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "ab"
    );

    assert_eq!(
        crate::test_support::with_service_context(
            b"\"0\\\x01F\x02\\A\"",
            crate::lex::lex_with_context
        )
        .unwrap()[0]
            .kind,
        TokenKind::Binary(BinaryValue {
            unused_bits: 4,
            data: vec![0xa0].into_boxed_slice(),
        })
    );

    let tokens =
        crate::test_support::with_service_context(b"1\\\x01N\x02\\2", crate::lex::lex_with_context)
            .expect("print control separator with ignored controls");
    assert_eq!(tokens.len(), 2);
    assert!(matches!(tokens[0].kind, TokenKind::Integer(1)));
    assert!(matches!(tokens[1].kind, TokenKind::Integer(2)));

    let error = crate::test_support::with_service_context(
        b"<a\\\x01N\x02\\b>",
        crate::lex::lex_with_context,
    )
    .expect_err("resource print control");
    assert!(error.message.contains("resource"));
}

#[test]
fn lexer_accepts_exponent_before_trailing_decimal_point() {
    let token = crate::test_support::with_service_context(b"6E-16.", crate::lex::lex_with_context)
        .expect("real with trailing decimal point")[0]
        .kind
        .clone();
    let crate::lex::TokenKind::Real(value) = token else {
        panic!("expected a real token");
    };
    assert!(value.get().abs() < 1e-15);
}

#[test]
fn lexer_rejects_strings_that_exceed_the_stored_length_limit() {
    let mut source = Vec::with_capacity(32_770);
    source.push(b'\'');
    source.extend(std::iter::repeat_n(b'x', 32_768));
    source.push(b'\'');
    let error = crate::test_support::with_service_context(&source, crate::lex::lex_with_context)
        .expect_err("oversized string");
    assert!(error.message.contains("maximum stored length"));
}

#[test]
fn lexer_accepts_underscores_and_rejects_hyphens_in_enumeration_names() {
    assert_eq!(
        crate::test_support::with_service_context(b"._USER2.", crate::lex::lex_with_context)
            .unwrap()[0]
            .kind,
        crate::lex::TokenKind::Enumeration("_USER2".into())
    );
    assert!(crate::test_support::with_service_context(
        b".USER-DEFINED.",
        crate::lex::lex_with_context
    )
    .is_err());
}

#[test]
fn lexer_distinguishes_entity_and_value_occurrence_names() {
    use crate::lex::TokenKind;

    let tokens = crate::test_support::with_service_context(
        b"#001 @002 #pi_value @_LIMIT",
        crate::lex::lex_with_context,
    )
    .expect("occurrence names");
    assert_eq!(tokens[0].kind, TokenKind::Instance(1));
    assert_eq!(tokens[1].kind, TokenKind::ValueInstance(2));
    assert_eq!(tokens[2].kind, TokenKind::ConstantEntity("PI_VALUE".into()));
    assert_eq!(tokens[3].kind, TokenKind::ConstantValue("_LIMIT".into()));

    for input in [b"#0".as_slice(), b"@00"] {
        let error = crate::test_support::with_service_context(input, crate::lex::lex_with_context)
            .expect_err("zero occurrence name");
        assert_eq!(error.message, "instance name must not be zero");
    }
}

#[test]
fn real_lexeme_rejects_binary64_overflow() {
    for source in [b"1.E9999".as_slice(), b"-1.E9999".as_slice()] {
        let error = crate::test_support::with_service_context(source, crate::lex::lex_with_context)
            .expect_err("overflow cannot enter a real token");
        assert!(error.message.contains("finite binary64 range"));
        assert!(matches!(error.into_codec_error(), CodecError::Malformed(_)));
    }
}

#[test]
fn real_lexeme_preserves_finite_bits() {
    for number in [-0.0, f64::MAX, f64::from_bits(1)] {
        let source = format!("{number:.17e}");
        let tokens = crate::test_support::with_service_context(
            source.as_bytes(),
            crate::lex::lex_with_context,
        )
        .expect("finite real token");
        let crate::lex::TokenKind::Real(real) = tokens[0].kind else {
            panic!("real token required")
        };
        assert_eq!(real.get().to_bits(), number.to_bits());
    }
}

#[test]
fn lexer_error_message_copy_refusal_reaches_codec_result() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error =
        lex_under_policy(b"?", policy, false).expect_err("error message has no retained storage");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "STEP lexer error message"));
}

#[test]
fn occurrence_number_parse_preserves_refusal() {
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "STEP occurrence number parse",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(b"#7", &arena, &policy)
                    .unwrap();
            let result = super::lex_with_context(b"#7", &ctx)
                .map(|_| ())
                .map_err(super::LexError::into_codec_error);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn real_number_parse_preserves_refusal() {
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "STEP real number parse",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(b"2.5", &arena, &policy)
                    .unwrap();
            let result = super::lex_with_context(b"2.5", &ctx)
                .map(|_| ())
                .map_err(super::LexError::into_codec_error);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn integer_number_parse_preserves_refusal() {
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "STEP integer number parse",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(b"7", &arena, &policy)
                    .unwrap();
            let result = super::lex_with_context(b"7", &ctx)
                .map(|_| ())
                .map_err(super::LexError::into_codec_error);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn token_tag_cost_is_exact_and_excludes_payloads() {
    use cadmpeg_core::decode::cost::DecodeCost;
    let ctx = cadmpeg_test_support::service_decode_context();
    let first = super::TokenKind::Name(String::from("FIRST")).tag();
    let second = super::TokenKind::Name(String::from("different payload")).tag();
    let width = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
        std::mem::Discriminant<super::TokenKind>,
    >());
    assert_eq!(<super::TokenTag as DecodeCost>::FIXED_BYTES, Some(width));
    assert_eq!(
        first.decode_cost(&ctx, "test token category").unwrap(),
        width
    );
    assert_eq!(
        second.decode_cost(&ctx, "test token category").unwrap(),
        width
    );
    assert_eq!(first, second);
    assert_ne!(first, super::TokenKind::Comma.tag());
}

#[test]
fn normalized_retained_character_preserves_refusal() {
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "step_lex_normalized_retained",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"AB", &arena, &policy).unwrap();
            let lexer = super::Lexer::new(b"AB", &ctx);
            let result = lexer
                .normalized(0, 2, super::LiteralStorage::Retained)
                .map(|_| ())
                .map_err(super::LexError::into_codec_error);
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn normalized_temp_character_preserves_refusal() {
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "step_lex_normalized_temp",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"AB", &arena, &policy).unwrap();
            let lexer = super::Lexer::new(b"AB", &ctx);
            let result = lexer
                .normalized(0, 2, super::LiteralStorage::Transient)
                .map(|_| ())
                .map_err(super::LexError::into_codec_error);
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn lexer_cursor_and_control_lookahead_preserve_refusal() {
    for (input, operation) in [
        (b" /* comment */ NAME".as_slice(), "STEP lexer cursor traversal"),
        (b"/* comment */NAME".as_slice(), "STEP lexer comment traversal"),
        (b"\\\x01N\\".as_slice(), "STEP lexer control lookahead"),
        (b"<name>".as_slice(), "STEP resource UTF-8 validation"),
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation, |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            lex_under_policy(input, policy, false)
        });
    }
}

#[test]
fn transient_token_storage_stays_live_until_the_next_token() {
    crate::test_support::with_service_context(b"\"0A1F2\"", |input, ctx| {
        let mut lexer = super::Lexer::new(input, ctx);
        lexer.set_transient_literals();
        let token = lexer.next_token().unwrap().unwrap();
        let CodecError::ResourceLimit(limit) = ctx.reserve_scoped(u64::MAX, "live token probe").unwrap_err() else { panic!("probe must refuse"); };
        assert_eq!(limit.used, 2);
        assert!(matches!(token.kind, super::TokenKind::Binary(_)));
    });
}

#[test]
fn control_run_lookahead_is_linear_and_preserves_resource_error_offset() {
    fn work(source: &[u8]) -> u64 {
        crate::test_support::with_service_context(source, |input, ctx| {
            super::lex_with_context(input, ctx).expect("control run input lexes");
            let CodecError::ResourceLimit(refusal) = ctx.charge_work(u64::MAX, "test completed lexer work").expect_err("work probe refuses") else { panic!("work refusal required"); };
            refusal.used
        })
    }
    for (prefix, suffix) in [(b"<a".as_slice(), b"b>".as_slice()), (b"SIGNATURE;AAAA".as_slice(), b"ENDSEC;".as_slice())] {
        let short = [prefix, &[0; 16], suffix].concat();
        let long = [prefix, &[0; 128], suffix].concat();
        // One cursor visit and one normalized-byte/signature validation visit per control;
        // the run skipper has one additional outer visit, independent of run length.
        assert_eq!(work(&long) - work(&short), 2 * (128 - 16));
    }
    let invalid = [b"<a".as_slice(), &[0; 16], b"\\N\\>".as_slice()].concat();
    let error = crate::test_support::with_service_context(&invalid, super::lex_with_context).expect_err("print directives are forbidden in resources");
    assert_eq!(error.offset, 2);
    assert_eq!(error.message, "print control directive is not allowed in a resource");
}
