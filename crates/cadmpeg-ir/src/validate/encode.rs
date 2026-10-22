// SPDX-License-Identifier: Apache-2.0
//! Validator ownership of encode input-refusal evidence.

use std::fmt;

use crate::report::check::Finding;

/// Evidence reserved for a non-finite document field that a writer writes.
///
/// Only the validator can construct this proof. Neither a public `Finding`
/// nor a deserialized validation report is evidence of encode eligibility.
/// There is currently no issuance API: the later validator/writer integration
/// must establish both the actual non-finite input and its written field
/// before making an `InvalidInput` refusal available.
///
/// A caller cannot wrap a finding, including an Error-severity finding.
///
/// ```compile_fail
/// use cadmpeg_ir::codec::write::ValidatedFinding;
/// use cadmpeg_ir::report::check::Finding;
///
/// fn forge(finding: Finding) -> ValidatedFinding {
///     ValidatedFinding { finding }
/// }
/// ```
///
/// A caller cannot turn a fabricated finding into a proof through conversion.
///
/// ```compile_fail
/// use cadmpeg_ir::codec::write::ValidatedFinding;
/// use cadmpeg_ir::report::check::Finding;
///
/// fn forge(finding: Finding) -> ValidatedFinding {
///     finding.into()
/// }
/// ```
///
/// Serialized input cannot mint a validation proof.
///
/// ```compile_fail
/// use cadmpeg_ir::codec::write::ValidatedFinding;
///
/// let _: ValidatedFinding = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedFinding {
    // Visibility is restricted to validate and its children, never write.
    pub(super) finding: Finding,
}

impl ValidatedFinding {
    /// The validator's finding, without mutable access to its evidence.
    #[must_use]
    pub const fn finding(&self) -> &Finding {
        &self.finding
    }
}

impl fmt::Display for ValidatedFinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.finding.message)
    }
}
