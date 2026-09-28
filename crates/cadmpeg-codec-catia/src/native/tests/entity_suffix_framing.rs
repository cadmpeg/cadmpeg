// SPDX-License-Identifier: Apache-2.0
//! Framing recognition for entity-suffix byte sequences.

use super::super::{
    entity_suffix_framing, CatiaEntitySuffixEscapedWord, CatiaEntitySuffixEscapedWordState,
    CatiaEntitySuffixFraming,
};

fn framing(bytes: &[u8]) -> Option<CatiaEntitySuffixFraming> {
    crate::test_support::with_service_context(|ctx| entity_suffix_framing(ctx, bytes))
        .expect("test suffix fits the service profile")
}

#[test]
fn fixed_suffix_payload_refuses_retained_and_collection_limits() {
    let suffix = [0xfe, 0xf6, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let retained = crate::test_support::with_retained_limit(15, |ctx| {
        entity_suffix_framing(ctx, &suffix)
    });
    assert!(matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_fixed_suffix_payload"));
    let collection = crate::test_support::with_collection_limit(15, |ctx| {
        entity_suffix_framing(ctx, &suffix)
    });
    assert!(matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_fixed_suffix_payload"));
    assert_eq!(framing(&suffix), Some(CatiaEntitySuffixFraming::FixedFeF6 {
        payload: (0..=15).collect(),
    }));
}

#[test]
fn decodes_each_escaped_word_state() {
    for (code, state) in [
        (0x00, CatiaEntitySuffixEscapedWordState::State00),
        (0x01, CatiaEntitySuffixEscapedWordState::State01),
        (0x03, CatiaEntitySuffixEscapedWordState::State03),
        (0x04, CatiaEntitySuffixEscapedWordState::State04),
        (0x09, CatiaEntitySuffixEscapedWordState::State09),
    ] {
        assert_eq!(
            framing(&[0x80, 0x78, 0x56, 0x34, 0x12, code]),
            Some(CatiaEntitySuffixFraming::EscapedWord(
                CatiaEntitySuffixEscapedWord {
                    word: 0x1234_5678,
                    state,
                }
            ))
        );
    }
}

#[test]
fn decodes_each_complete_non_value_framing() {
    assert_eq!(
        framing(&[0x81, 0x49]),
        Some(CatiaEntitySuffixFraming::Token8149)
    );
    assert_eq!(
        framing(&[
            0xfe, 0xf6, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
            0x0c, 0x0d, 0x0e, 0x0f,
        ]),
        Some(CatiaEntitySuffixFraming::FixedFeF6 {
            payload: (0x00..=0x0f).collect(),
        })
    );
    assert_eq!(
        framing(&[0xd2, 0x2d, 0x01]),
        Some(CatiaEntitySuffixFraming::PagedAtomState01 { value: 302 })
    );
}

#[test]
fn rejects_other_framing() {
    assert_eq!(framing(&[0x80, 0x78, 0x56, 0x34, 0x12]), None);
    assert_eq!(
        framing(&[0x81, 0x78, 0x56, 0x34, 0x12, 0x00]),
        None
    );
    assert_eq!(
        framing(&[0x80, 0x78, 0x56, 0x34, 0x12, 0x02]),
        None
    );
    assert_eq!(
        framing(&[0x80, 0x78, 0x56, 0x34, 0x12, 0x00, 0x00]),
        None
    );
    assert_eq!(framing(&[0xfe, 0xf6, 0x00]), None);
    assert_eq!(framing(&[0xd2, 0x2d, 0x00]), None);
}
