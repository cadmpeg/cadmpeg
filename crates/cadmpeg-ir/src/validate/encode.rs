// SPDX-License-Identifier: Apache-2.0
//! Validator ownership of encode input-refusal evidence.

use std::fmt;

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};

use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::CadIr;

/// Evidence reserved for a non-finite document field that a writer writes.
///
/// Only the validator can construct this proof. Neither a public `Finding`
/// nor a deserialized validation report is evidence of encode eligibility.
/// The validator issues this evidence for a non-finite field in CADIR output.
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
    pub(crate) fn for_cadir(
        ctx: &DecodeContext<'_>,
        document: &CadIr,
    ) -> Result<Option<Self>, ResourceLimit> {
        match crate::hash::finite_json::check_cadir_finite(ctx, document) {
            Err(crate::hash::finite_json::CanonicalJsonError::Resource(limit)) => Err(limit),
            result => Ok(Self::from_cadir_check(result)),
        }
    }

    pub(crate) fn for_cadir_encode(document: &CadIr) -> Option<Self> {
        Self::from_cadir_check(crate::hash::finite_json::check_cadir_finite_for_encode(
            document,
        ))
    }

    fn from_cadir_check(
        result: Result<(), crate::hash::finite_json::CanonicalJsonError>,
    ) -> Option<Self> {
        match result {
            Err(crate::hash::finite_json::CanonicalJsonError::NonFinite { value }) => Some(Self {
                finding: Finding {
                    check: Check::FiniteValue,
                    severity: Severity::Error,
                    message: format!("document contains a non-finite number: {value}"),
                    entity: None,
                    related: Vec::new(),
                },
            }),
            _ => None,
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repro::{BundleMetadata, NumberBand, Signature, Stage};

    #[test]
    fn cadir_proof_is_the_neutral_validation_finding() {
        let mut document = CadIr::empty();
        document.fragment = true;
        let seed = Finding {
            check: Check::Identity,
            severity: Severity::Error,
            message: "test".into(),
            entity: None,
            related: Vec::new(),
        };
        let mut signature = Signature::new(&seed, "cadir", Stage::Check);
        signature.numbers.push(NumberBand {
            value: f64::NAN,
            lower: 0.0,
            upper: 1.0,
        });
        document.repro = Some(Box::new(BundleMetadata {
            signature,
            roots: Vec::new(),
            context: serde_json::Map::default(),
            manifest: Vec::new(),
            incomplete_reason: None,
        }));

        let ctx = cadmpeg_test_support::service_decode_context();
        let proof = ValidatedFinding::for_cadir(&ctx, &document)
            .unwrap()
            .expect("non-finite document value");
        let report = crate::validate::validate_neutral(&document, Vec::new()).unwrap();
        assert!(report.findings.contains(proof.finding()));
        assert_eq!(proof.finding().check, Check::FiniteValue);
        assert_eq!(proof.finding().severity, Severity::Error);

        document.repro.as_mut().unwrap().signature.numbers[0].value = 0.5;
        assert!(ValidatedFinding::for_cadir(&ctx, &document)
            .unwrap()
            .is_none());
    }
}
