// SPDX-License-Identifier: Apache-2.0
//! Narrow admissibility predicates as documented subsets of [`Check`].
//!
//! Decoder and export gates must not depend on the full final-document
//! validator. Each route names the [`Check`] variants that may reject a
//! candidate; findings outside that set do not affect admission.

use crate::annotations::Annotations;
use crate::document::CadIr;
use crate::report::{
    check::{Check, ValidationReport},
    loss::LossNote,
};

/// Expand [`DRAFT_CORE_CHECKS`], optionally appending extra [`Check`] variants.
macro_rules! with_draft_core {
    ($($extra:expr),* $(,)?) => {
        &[
            Check::Identity,
            Check::ReferentialIntegrity,
            Check::NativeLinks,
            Check::CoedgePairing,
            Check::ShellTopology,
            Check::WireTopology,
            Check::CarrierReachability,
            Check::ParameterDomain,
            Check::GeometricConsistency,
            $($extra),*
        ]
    };
}

/// Shared draft/topology core for decoder and export-precondition gates.
///
/// [`Check::Identity`], [`Check::ReferentialIntegrity`], [`Check::NativeLinks`],
/// [`Check::CoedgePairing`], [`Check::ShellTopology`],
/// [`Check::WireTopology`], [`Check::CarrierReachability`],
/// [`Check::ParameterDomain`], and
/// [`Check::GeometricConsistency`].
pub const DRAFT_CORE_CHECKS: &[Check] = with_draft_core!();

/// Rhino draft-candidate gate: [`DRAFT_CORE_CHECKS`] plus Annotations.
///
/// `ArenaOrder` is excluded — candidates are judged before
/// [`CadIr::finalize`](crate::CadIr::finalize).
pub const RHINO_DRAFT_CHECKS: &[Check] = with_draft_core!(Check::Annotations);

/// Rhino instance-expansion gate: [`DRAFT_CORE_CHECKS`] only.
///
/// `ArenaOrder` is excluded. Mid-expansion candidates are not finalized; order
/// findings must not roll back a structurally sound expansion.
pub const RHINO_INSTANCE_CHECKS: &[Check] = DRAFT_CORE_CHECKS;

/// CATIA topology admission after [`Model::finalize`](crate::document::Model::finalize).
///
/// Pending native identities are supplied through
/// [`admit_with_additional_native_identities`]; `ArenaOrder` is not in the set
/// because finalize runs first.
pub const CATIA_ADMISSION_CHECKS: &[Check] = DRAFT_CORE_CHECKS;

/// Documented draft/topology floor for the SLDPRT export precondition.
///
/// The production writer input gate keeps full `validate_neutral` because
/// refusal depends on non-core Checks (for example `Counts`). Narrowing onto
/// this set requires additional reject-fixture coverage.
pub const SLDPRT_EXPORT_PRECONDITION_CHECKS: &[Check] = DRAFT_CORE_CHECKS;

/// Drop findings whose [`Check`] is outside `allowed`.
pub fn filter_checks(mut report: ValidationReport, allowed: &[Check]) -> ValidationReport {
    report
        .findings
        .retain(|finding| allowed.contains(&finding.check));
    report
}

/// Run full neutral validation, then retain only findings in `allowed`.
pub fn admit(ir: &CadIr, allowed: &[Check], losses: Vec<LossNote>) -> ValidationReport {
    filter_checks(super::validate_neutral(ir, losses), allowed)
}

/// Admit with borrowed annotations, retaining only findings in `allowed`.
pub fn admit_with_annotations(
    ir: &CadIr,
    annotations: &Annotations,
    allowed: &[Check],
    losses: Vec<LossNote>,
) -> ValidationReport {
    filter_checks(
        super::validate_neutral_with_annotations(ir, annotations, losses),
        allowed,
    )
}

/// Admit while treating staged native identities as resolvable.
pub fn admit_with_additional_native_identities<'a>(
    ir: &'a CadIr,
    additional: impl IntoIterator<Item = &'a str>,
    allowed: &[Check],
    losses: Vec<LossNote>,
) -> ValidationReport {
    filter_checks(
        super::validate_neutral_with_additional_native_identities(ir, additional, losses),
        allowed,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        admit, CATIA_ADMISSION_CHECKS, DRAFT_CORE_CHECKS, RHINO_DRAFT_CHECKS,
        RHINO_INSTANCE_CHECKS, SLDPRT_EXPORT_PRECONDITION_CHECKS,
    };
    use crate::report::check::Check;
    use cadmpeg_test_support::admissibility::{
        accepted_empty, rejected_missing_point, rejected_missing_region,
    };

    // The shared fixture crate links the library instance of CadIr. Unit tests
    // compile a separate instance, so transfer fixture data through its wire form.
    fn fixture(value: impl serde::Serialize) -> crate::CadIr {
        serde_json::from_value(serde_json::to_value(value).expect("fixture serializes"))
            .expect("fixture deserializes into the unit-test IR")
    }

    #[test]
    fn draft_core_agrees_with_full_on_freeze_fixtures() {
        let accepted = fixture(accepted_empty());
        assert!(super::super::validate_neutral(&accepted, Vec::new()).is_ok());
        assert!(admit(&accepted, DRAFT_CORE_CHECKS, Vec::new()).is_ok());

        let missing_point = fixture(rejected_missing_point("test:model").expect("valid identity"));
        assert!(!super::super::validate_neutral(&missing_point, Vec::new()).is_ok());
        assert!(!admit(&missing_point, DRAFT_CORE_CHECKS, Vec::new()).is_ok());

        let missing_region =
            fixture(rejected_missing_region("test:model").expect("valid identity"));
        assert!(!super::super::validate_neutral(&missing_region, Vec::new()).is_ok());
        assert!(!admit(&missing_region, DRAFT_CORE_CHECKS, Vec::new()).is_ok());
    }

    #[test]
    fn filter_checks_drops_out_of_set_findings() {
        let ir = fixture(rejected_missing_point("test:model").expect("valid identity"));
        let filtered = admit(&ir, &[Check::Identity], Vec::new());
        assert!(
            filtered.is_ok(),
            "referential_integrity must not reject under Identity-only set: {filtered:?}"
        );
    }

    #[test]
    fn documented_route_constants_match_admit_subsets() {
        assert_eq!(
            DRAFT_CORE_CHECKS,
            &[
                Check::Identity,
                Check::ReferentialIntegrity,
                Check::NativeLinks,
                Check::CoedgePairing,
                Check::ShellTopology,
                Check::WireTopology,
                Check::CarrierReachability,
                Check::ParameterDomain,
                Check::GeometricConsistency,
            ]
        );
        assert_eq!(RHINO_INSTANCE_CHECKS, DRAFT_CORE_CHECKS);
        assert_eq!(CATIA_ADMISSION_CHECKS, DRAFT_CORE_CHECKS);
        assert_eq!(SLDPRT_EXPORT_PRECONDITION_CHECKS, DRAFT_CORE_CHECKS);
        assert_eq!(
            RHINO_DRAFT_CHECKS.len(),
            DRAFT_CORE_CHECKS.len() + 1,
            "Rhino draft is core plus Annotations"
        );
        assert_eq!(
            &RHINO_DRAFT_CHECKS[..DRAFT_CORE_CHECKS.len()],
            DRAFT_CORE_CHECKS
        );
        assert_eq!(
            RHINO_DRAFT_CHECKS[DRAFT_CORE_CHECKS.len()],
            Check::Annotations
        );
        assert!(!DRAFT_CORE_CHECKS.contains(&Check::ArenaOrder));
        assert!(!DRAFT_CORE_CHECKS.contains(&Check::Counts));
        assert!(!RHINO_DRAFT_CHECKS.contains(&Check::ArenaOrder));

        let accepted = fixture(accepted_empty());
        let rejected = fixture(rejected_missing_point("test:model").expect("valid identity"));
        for allowed in [
            DRAFT_CORE_CHECKS,
            RHINO_DRAFT_CHECKS,
            RHINO_INSTANCE_CHECKS,
            CATIA_ADMISSION_CHECKS,
            SLDPRT_EXPORT_PRECONDITION_CHECKS,
        ] {
            assert!(admit(&accepted, allowed, Vec::new()).is_ok());
            assert!(!admit(&rejected, allowed, Vec::new()).is_ok());
        }
    }
}
