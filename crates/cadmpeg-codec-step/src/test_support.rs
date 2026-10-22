// SPDX-License-Identifier: Apache-2.0
//! Shared STEP Part 21 byte-fixture helpers for `#[cfg(test)]` suites.

#![allow(clippy::unwrap_used)]

pub(crate) mod exchange;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

pub(crate) fn with_service_context<T>(
    bytes: &[u8],
    use_context: impl FnOnce(&[u8], &DecodeContext<'_>) -> T,
) -> T {
    with_policy_context(bytes, &DecodePolicy::service(), use_context)
}

pub(crate) fn with_policy_context<T>(
    bytes: &[u8],
    policy: &DecodePolicy,
    use_context: impl FnOnce(&[u8], &DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy)
        .expect("fixture fits selected policy");
    use_context(bytes, &ctx)
}

/// Tokenize one complete clear-text exchange structure.
pub(crate) fn lex(
    input: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<Vec<crate::lex::Token>, crate::lex::LexError> {
    let mut lexer = crate::lex::Lexer::new(input, ctx);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    Ok(tokens)
}

/// The canonical name attribute projected from a geometric representation item.
pub(crate) fn geometry_name<'a>(
    ir: &'a cadmpeg_ir::CadIr,
    target: &cadmpeg_ir::attributes::AttributeTarget,
) -> Option<&'a str> {
    ir.model.attributes.iter().find_map(|attribute| {
        if attribute.target != *target || attribute.name != "name" {
            return None;
        }
        match attribute.values.as_slice() {
            [cadmpeg_ir::attributes::AttributeValue::String(value)] => Some(value.as_str()),
            _ => None,
        }
    })
}

/// The record ids an omission loss lists after its count (#217 §5.6).
///
/// Asserts that the count equals the number of listed ids and that the ids
/// are sorted.
pub(crate) fn omitted_record_ids(message: &str) -> Vec<&str> {
    let mut offset = 0;
    let (count, count_end) = message
        .split(' ')
        .find_map(|word| {
            let start = offset;
            offset += word.len() + 1;
            word.parse::<usize>()
                .ok()
                .map(|count| (count, start + word.len()))
        })
        .unwrap_or_else(|| panic!("omission loss states no count: {message}"));
    let (_, list) = message[count_end..]
        .split_once(": ")
        .unwrap_or_else(|| panic!("omission loss lists no ids after its count: {message}"));
    let ids: Vec<&str> = list.split(", ").collect();
    assert_eq!(ids.len(), count, "{message}");
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "{message}");
    ids
}
