// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::lex::{Lexer, LiteralStorage, TokenKind};

#[test]
fn empty_lexer_has_no_input_visit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let mut lexer = Lexer::new(b"", &ctx);
    assert!(lexer.next_token().unwrap().is_none());
    assert!(lexer.next_token().unwrap().is_none());
    drop(lexer);
    ctx.finish_session().unwrap();
}

#[test]
fn empty_lexer_preserves_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "original lexer refusal").unwrap_err() else {
        panic!("original work refusal");
    };
    let mut lexer = Lexer::new(b"", &ctx);
    assert!(matches!(lexer.next_token().unwrap_err().into_resource_error(), Some(CodecError::ResourceLimit(limit)) if limit == original));
    drop(lexer);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn lexer_admits_actual_semicolon_and_no_exhaustion_visit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One actual trivia classification and one actual token admission.
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b";", &arena, &policy).unwrap();
    let mut lexer = Lexer::new(b";", &ctx);
    let token = lexer.next_token().unwrap().unwrap();
    assert_eq!(token.kind, TokenKind::Semicolon);
    assert_eq!(token.span, 0..1);
    assert!(lexer.next_token().unwrap().is_none());
    drop(token);
    drop(lexer);
    ctx.finish_session().unwrap();
}

#[test]
fn trailing_trivia_admits_only_its_actual_scans() {
    for (input, work) in [(b" ".as_slice(), 1), (b"\\N\\", 1), (b"/*x*/", 3), (b"\x01", 2)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // A comment has one cursor visit and two actual delimiter windows.
        // A control has one outer classification and one run-skip visit.
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy).unwrap();
        let mut lexer = Lexer::new(input, &ctx);
        assert!(lexer.next_token().unwrap().is_none());
        assert_eq!(lexer.at, input.len());
        assert!(lexer.next_token().unwrap().is_none());
        drop(lexer);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn normalized_text_admits_only_actual_bytes_and_characters() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two length-measurement visits, two emission visits, two ASCII writes.
    policy.limits.max_work_units = 6;
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"AB", &arena, &policy).unwrap();
    let lexer = Lexer::new(b"AB", &ctx);
    let (text, storage) = lexer.normalized(0, 2, LiteralStorage::Retained).unwrap();
    assert_eq!(text, "AB");
    assert!(storage.is_none());
    drop(text);
    drop(storage);
    drop(lexer);
    ctx.finish_session().unwrap();
}

#[test]
fn normalized_first_character_preserves_refusal_before_the_suffix() {
    let data = [b'A'; 8193];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The required complete length pass and first actual emission-byte visit.
    policy.limits.max_work_units = data.len() as u64 + 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let lexer = Lexer::new(&data, &ctx);
    let error = lexer.normalized(0, data.len(), LiteralStorage::Retained).unwrap_err();
    let Some(CodecError::ResourceLimit(limit)) = error.into_resource_error() else {
        panic!("original character refusal");
    };
    assert_eq!(limit.operation, "step_lex_normalized_retained");
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!((limit.used, limit.additional), (data.len() as u64 + 1, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(lexer);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn empty_normalized_text_has_no_visit_and_preserves_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let lexer = Lexer::new(b"", &ctx);
    let (text, storage) = lexer.normalized(0, 0, LiteralStorage::Retained).unwrap();
    assert!(text.is_empty());
    assert!(storage.is_none());
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "original lexer refusal").unwrap_err() else {
        panic!("original work refusal");
    };
    assert!(matches!(lexer.normalized(0, 0, LiteralStorage::Retained).unwrap_err().into_resource_error(), Some(CodecError::ResourceLimit(limit)) if limit == original));
    drop(text);
    drop(storage);
    drop(lexer);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn empty_unterminated_string_admits_its_real_diagnostic_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = "unterminated string".len() as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(b"'", &arena, &policy).unwrap();
    let mut lexer = Lexer::new(b"'", &ctx);
    let error = lexer.string().unwrap_err();
    assert_eq!(error.offset, 0);
    assert_eq!(error.message, "unterminated string");
    assert!(error.resource.is_none());
    drop(error);
    drop(lexer);
    ctx.finish_session().unwrap();
}

#[test]
fn unterminated_string_keeps_maximum_length_error_priority() {
    let mut data = vec![b'x'; super::super::MAX_STORED_STRING_OCTETS];
    data[0] = b'\'';
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut lexer = Lexer::new(&data, &ctx);
    let error = lexer.string().unwrap_err();
    assert_eq!(error.offset, 0);
    assert_eq!(error.message, "string exceeds maximum stored length");
    assert!(error.resource.is_none());
    drop(error);
    drop(lexer);
    ctx.finish_session().unwrap();
}
