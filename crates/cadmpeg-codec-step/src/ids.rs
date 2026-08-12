// SPDX-License-Identifier: Apache-2.0
//! Codec-owned STEP identity builders.
//!
//! Every mint path validates `<format>:<scope>:<kind>#<key>` at construction so
//! a two-component id such as `step:signature#0` cannot be produced.

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

    /// DATA-section opaque or geometry kind: `step:data:{kind}#{key}`.
    ///
    /// Empty `kind` (malformed zero-partial records) uses `opaque`.
    #[must_use]
    pub fn data(kind: &str, key: impl std::fmt::Display) -> String {
        let kind = if kind.is_empty() { "opaque" } else { kind };
        format_identity("step", "data", kind, key)
            .unwrap_or_else(|error| panic!("step data identity: {error}"))
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
