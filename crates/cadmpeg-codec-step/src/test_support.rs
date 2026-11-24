// SPDX-License-Identifier: Apache-2.0
//! Shared STEP Part 21 byte-fixture helpers for `#[cfg(test)]` suites.

#![allow(clippy::unwrap_used)]

pub(crate) mod box_brep;
pub(crate) mod curve_bounded;
pub(crate) mod exchange;
pub(crate) mod revolved_wedge;
pub(crate) mod validated_part;

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

pub(crate) mod pmi_limits;

/// The `name` attribute the decoder states for the carrier `id`.
pub(crate) fn carrier_name<'a>(ir: &'a cadmpeg_ir::document::CadIr, id: &str) -> Option<&'a str> {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    ir.model.attributes.iter().find_map(|attribute| {
        let target = match &attribute.target.entity {
            AttributeTarget::Curve(curve) => curve.as_str(),
            AttributeTarget::Point(point) => point.as_str(),
            AttributeTarget::Surface(surface) => surface.as_str(),
            _ => return None,
        };
        if attribute.name != "name" || target != id {
            return None;
        }
        match attribute.values.as_slice() {
            [Some(AttributeValue::String(name))] => Some(name.as_str()),
            _ => None,
        }
    })
}
