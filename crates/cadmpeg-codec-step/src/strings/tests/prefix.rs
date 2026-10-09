// SPDX-License-Identifier: Apache-2.0
//! Direct-text callback admission under the original work budget.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parse::implementation_level::ImplementationLevel;
use crate::strings::{decode_chars, StringDecodeFailure, StringError};

fn callback_refusal(input: &[u8], level: ImplementationLevel, first: char) {
    // One outer visit, every byte of the direct-span scan, optional UTF-8
    // validation, and the first actual character visit precede the callback.
    let span_bytes = u64::try_from(input.len()).expect("fixture length");
    let prior_work = 1 + span_bytes + if level.is_edition3() { span_bytes } else { 0 };
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = prior_work + 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
        let mut calls = 0;
        let error = decode_chars(&ctx, input, level, |character| {
            assert_eq!(character, first);
            calls += 1;
            ctx.charge_work(1, "test direct-text callback")?;
            Ok(())
        }).expect_err("first callback refuses without visiting the suffix");
        let StringDecodeFailure::Resource(CodecError::ResourceLimit(refusal)) = error else {
            panic!("callback resource refusal");
        };
        assert_eq!(calls, 1);
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "test direct-text callback");
        assert_eq!(refusal.used, prior_work + 1);
        assert_eq!(refusal.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn legacy_direct_text_callback_refuses_before_unused_visits() {
    callback_refusal(&[b'A'; 8193], ImplementationLevel::LegacyEdition2, 'A');
}

#[test]
fn utf8_direct_text_callback_refuses_before_unused_visits() {
    let mut input = String::from("🙂");
    input.push_str(&"A".repeat(8192));
    callback_refusal(input.as_bytes(), ImplementationLevel::Edition3Class1, '🙂');
}

#[test]
fn direct_text_semantic_callback_error_precedes_unused_visits() {
    for level in [ImplementationLevel::LegacyEdition2, ImplementationLevel::Edition3Class1] {
        let input = [b'A'; 8193];
        let bytes = u64::try_from(input.len()).expect("fixture length");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1 + bytes + if level.is_edition3() { bytes } else { 0 } + 1;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut calls = 0;
            let error = decode_chars(ctx, &input, level, |_| {
                calls += 1;
                Err(StringDecodeFailure::Invalid(StringError { offset: 0, message: String::new() }))
            }).expect_err("semantic callback error stops traversal");
            assert!(matches!(error, StringDecodeFailure::Invalid(StringError { offset: 0, .. })));
            assert_eq!(calls, 1);
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn direct_text_exact_work_has_no_terminal_charge() {
    for (input, level, work, expected) in [
        (b"AB".as_slice(), ImplementationLevel::LegacyEdition2, 5, "AB"),
        ("é🙂".as_bytes(), ImplementationLevel::Edition3Class1, 15, "é🙂"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut output = String::new();
            decode_chars(ctx, input, level, |character| {
                output.push(character);
                Ok(())
            }).expect("exact actual span and character work");
            assert_eq!(output, expected);
            assert_eq!(ctx.resource_refusal(), None);
            ctx.charge_work(0, "test completed direct-text route").expect("no terminal refusal");
        });
    }
}

#[test]
fn empty_direct_text_preserves_original_sticky_refusal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let CodecError::ResourceLimit(original) = ctx.charge_work(1, "test original string refusal")
            .expect_err("original caller refuses") else { panic!("resource refusal"); };
        // decode_chars performs no cursor step for empty input. Its public
        // decode caller observes the original fuse in retained_string.
        assert!(matches!(crate::strings::decode_with_context(
            b"", ImplementationLevel::Edition3Class1, ctx),
            Err(StringDecodeFailure::Resource(CodecError::ResourceLimit(refusal))) if refusal == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}
