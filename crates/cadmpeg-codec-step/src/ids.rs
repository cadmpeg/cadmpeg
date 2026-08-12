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
        UnknownId(
            format_identity("step", "file", "signature", index)
                .expect("step:file:signature#N is always valid"),
        )
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
mod tests {
    use super::StepIdentity;
    use cadmpeg_ir::{format_identity, is_valid_identity, IdentityError};

    #[test]
    fn signature_uses_three_component_grammar() {
        let id = StepIdentity::signature(0);
        assert_eq!(id.0, "step:file:signature#0");
        assert!(is_valid_identity(&id.0));
    }

    #[test]
    fn data_and_opaque_preserve_existing_forms() {
        assert_eq!(StepIdentity::data("surface", 12u64), "step:data:surface#12");
        assert_eq!(StepIdentity::opaque(2u64), "step:data:opaque#2");
        assert_eq!(StepIdentity::data("", 1u64), "step:data:opaque#1");
        assert!(is_valid_identity(&StepIdentity::data("edge", "3-shell-4")));
    }

    #[test]
    fn scoped_builders_preserve_existing_forms() {
        assert_eq!(
            StepIdentity::product("occurrence", "definition-9"),
            "step:product:occurrence#definition-9"
        );
        assert_eq!(
            StepIdentity::presentation("pmi", 4u64),
            "step:presentation:pmi#4"
        );
        assert_eq!(
            StepIdentity::construction("trimmed_curve", 9u64),
            "step:construction:trimmed_curve#9"
        );
        assert_eq!(
            StepIdentity::tessellation("mesh", 1u64),
            "step:tessellation:mesh#1"
        );
        assert_eq!(
            StepIdentity::drawing("drawing_definition", 2u64),
            "step:drawing:drawing_definition#2"
        );
    }

    #[test]
    fn empty_scope_is_rejected_at_construction() {
        // The Phase 1 regression was `step:signature#0` (missing scope).
        let err = format_identity("step", "", "signature", 0u8).expect_err("empty scope");
        assert!(matches!(
            err,
            IdentityError::InvalidComponent { label: "scope", .. }
        ));
        assert!(!is_valid_identity("step:signature#0"));
    }
}
