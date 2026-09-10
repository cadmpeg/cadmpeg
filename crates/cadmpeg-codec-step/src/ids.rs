// SPDX-License-Identifier: Apache-2.0
//! Codec-owned STEP identity builders.
//!
//! Every mint path validates `<format>:<scope>:<kind>#<key>` at construction so
//! a two-component id such as `step:signature#0` cannot be produced.

use std::borrow::Cow;
use std::fmt::Display;

use cadmpeg_ir::ids::{format_identity, Identity, UnknownId};

/// The `<kind>` component of a STEP identity, checked at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityKind(Cow<'static, str>);

impl IdentityKind {
    /// Builds a kind from a source literal, rejected at compile time by [`kind!`].
    ///
    /// Crate-private so `kind!`, whose `static` forces const evaluation, is the
    /// only door: there is no runtime call site where the assert could fire.
    pub(crate) const fn from_literal(literal: &'static str) -> Self {
        assert!(
            literal_is_valid(literal.as_bytes()),
            "a STEP identity kind is nonempty and free of ':', '#' and whitespace"
        );
        Self(Cow::Borrowed(literal))
    }

    /// Builds a kind from file-derived text, or `None` when the text cannot be one.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        (!value.is_empty()
            && !value.contains(':')
            && !value.contains('#')
            && !value.chars().any(char::is_whitespace))
        .then(|| Self(Cow::Owned(value.to_owned())))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

const fn literal_is_valid(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b':' | b'#' | b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c => return false,
            _ => index += 1,
        }
    }
    true
}

/// Builds an [`IdentityKind`] from a string literal, checked when the crate compiles.
macro_rules! kind {
    ($literal:literal) => {{
        static KIND: $crate::ids::IdentityKind = $crate::ids::IdentityKind::from_literal($literal);
        &KIND
    }};
}

pub(crate) use kind;

/// File-level signature opaque record: `step:file:signature#{index}`.
#[must_use]
pub fn signature(index: usize) -> UnknownId {
    UnknownId::from(
        format_identity("step", "file", "signature", index)
            .expect("step:file:signature#N is always valid"),
    )
}

/// DATA-section geometry or opaque kind: `step:data:{kind}#{key}`.
#[must_use]
pub fn data(kind: &IdentityKind, key: impl Display) -> Identity {
    mint("data", kind, key)
}

/// Product structure identity: `step:product:{kind}#{key}`.
#[must_use]
pub fn product(kind: &IdentityKind, key: impl Display) -> Identity {
    mint("product", kind, key)
}

/// Presentation / PMI identity: `step:presentation:{kind}#{key}`.
#[must_use]
pub fn presentation(kind: &IdentityKind, key: impl Display) -> Identity {
    mint("presentation", kind, key)
}

/// Construction / procedural identity: `step:construction:{kind}#{key}`.
#[must_use]
pub fn construction(kind: &IdentityKind, key: impl Display) -> Identity {
    mint("construction", kind, key)
}

/// Tessellation identity: `step:tessellation:{kind}#{key}`.
#[must_use]
pub fn tessellation(kind: &IdentityKind, key: impl Display) -> Identity {
    mint("tessellation", kind, key)
}

/// Drawing graph identity: `step:drawing:{kind}#{key}`.
#[must_use]
pub fn drawing(kind: &IdentityKind, key: impl Display) -> Identity {
    mint("drawing", kind, key)
}

fn mint(scope: &str, kind: &IdentityKind, key: impl Display) -> Identity {
    format_identity("step", scope, kind.as_str(), key)
        .unwrap_or_else(|error| panic!("step {scope} identity: {error}"))
}

#[cfg(test)]
mod tests;
