// SPDX-License-Identifier: Apache-2.0
//! Codec-owned STEP identity builders.
//!
//! Every mint path validates `<format>:<scope>:<kind>#<key>` at construction so
//! a two-component id such as `step:signature#0` cannot be produced.

use std::fmt::Display;

use cadmpeg_ir::ids::{format_identity, UnknownId};

/// Builders for `step:` entity identities.
pub struct StepIdentity;

impl StepIdentity {
    /// File-level signature opaque record: `step:file:signature#{index}`.
    #[must_use]
    pub fn signature(index: usize) -> UnknownId {
        UnknownId::mint(
            format_identity("step", "file", "signature", index)
                .expect("step:file:signature#N is always valid"),
        )
        .expect("identity grammar")
    }

    /// DATA-section geometry or opaque kind: `step:data:{kind}#{key}`.
    ///
    /// Empty `kind` (malformed zero-partial records) uses [`Self::opaque`].
    #[must_use]
    pub fn data(kind: &str, key: impl Display) -> String {
        if kind.is_empty() {
            return Self::opaque(key);
        }
        Self::mint("data", kind, key)
    }

    /// Named opaque DATA record: `step:data:opaque#{key}`.
    #[must_use]
    pub fn opaque(key: impl Display) -> String {
        Self::mint("data", "opaque", key)
    }

    /// Product structure identity: `step:product:{kind}#{key}`.
    #[must_use]
    pub fn product(kind: &str, key: impl Display) -> String {
        Self::mint("product", kind, key)
    }

    /// Presentation / PMI identity: `step:presentation:{kind}#{key}`.
    #[must_use]
    pub fn presentation(kind: &str, key: impl Display) -> String {
        Self::mint("presentation", kind, key)
    }

    /// Construction / procedural identity: `step:construction:{kind}#{key}`.
    #[must_use]
    pub fn construction(kind: &str, key: impl Display) -> String {
        Self::mint("construction", kind, key)
    }

    /// Tessellation identity: `step:tessellation:{kind}#{key}`.
    #[must_use]
    pub fn tessellation(kind: &str, key: impl Display) -> String {
        Self::mint("tessellation", kind, key)
    }

    /// Drawing graph identity: `step:drawing:{kind}#{key}`.
    #[must_use]
    pub fn drawing(kind: &str, key: impl Display) -> String {
        Self::mint("drawing", kind, key)
    }

    fn mint(scope: &str, kind: &str, key: impl Display) -> String {
        format_identity("step", scope, kind, key)
            .unwrap_or_else(|error| panic!("step {scope} identity: {error}"))
    }
}

#[cfg(test)]
mod tests;
