// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parse::{validate_header_data_references, HeaderDataReferences, ValidationError};

fn assert_section_lookup_refusal(reference: HeaderDataReferences, visits: u64) {
    let names = BTreeSet::from([String::from("section")]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = visits;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let result = validate_header_data_references(&ctx, &[reference], &names);
    let Err(ValidationError::Resource(CodecError::ResourceLimit(refusal))) = result else {
        panic!("section-name comparison must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP DATA section name lookup");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn file_population_section_lookup_preserves_work_refusal() {
    // One reference visit and one section visit fit; the key comparisons refuse.
    assert_section_lookup_refusal(
        HeaderDataReferences::FilePopulation(BTreeSet::from([String::from("section")])),
        2,
    );
}

#[test]
fn direct_header_section_lookup_preserves_work_refusal() {
    // One reference visit fits; the key comparisons refuse.
    assert_section_lookup_refusal(HeaderDataReferences::Section(String::from("section")), 1);
}

#[test]
fn header_section_membership_keeps_validation_results() {
    let names = BTreeSet::from([String::from("section")]);
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(validate_header_data_references(&ctx, &[
        HeaderDataReferences::FilePopulation(names.clone()),
        HeaderDataReferences::Section(String::from("section")),
    ], &names).is_ok());
    assert!(matches!(validate_header_data_references(&ctx, &[
        HeaderDataReferences::FilePopulation(BTreeSet::from([String::from("absent")]))
    ], &names), Err(ValidationError::Invalid("FILE_POPULATION names an unknown DATA section"))));
    assert!(matches!(validate_header_data_references(&ctx, &[
        HeaderDataReferences::Section(String::from("absent"))
    ], &names), Err(ValidationError::Invalid("header section reference names an unknown DATA section"))));
}

#[test]
fn parser_expected_name_comparison_preserves_work_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let mut parser = crate::parse::Parser {
        lexer: crate::lex::Lexer::new(b"", &ctx),
        current: Some(crate::lex::Token { kind: crate::lex::TokenKind::Name(String::from("HEADER")), span: 0..6 }),
        last_end: 0, depth: 0, diagnostics: Vec::new(), omitted_entity_names: None, budget: &ctx,
    };
    let error = parser.name("HEADER").unwrap_err();
    let crate::parse::ParseError::Resource(CodecError::ResourceLimit(refusal)) = error else {
        panic!("name comparison must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP expected parser name comparison");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn parser_lookahead_name_preserves_equality_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let parser = crate::parse::Parser {
        lexer: crate::lex::Lexer::new(b"", &ctx),
        current: Some(crate::lex::Token { kind: crate::lex::TokenKind::Name(String::from("HEADER")), span: 0..6 }),
        last_end: 0, depth: 0, diagnostics: Vec::new(), omitted_entity_names: None, budget: &ctx,
    };
    let error = parser.peek_name("HEADER").unwrap_err();
    let crate::parse::ParseError::Resource(CodecError::ResourceLimit(refusal)) = error else {
        panic!("lookahead comparison must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP parser lookahead name equality");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}
