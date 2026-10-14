// SPDX-License-Identifier: Apache-2.0
//! Narrow admissibility predicates as documented subsets of [`Check`].
//!
//! Decoder and export gates must not depend on the full final-document
//! validator. Each route names the [`Check`] variants that may reject a
//! candidate; findings outside that set do not affect admission.

pub(crate) mod incremental;

use crate::annotations::Annotations;
use crate::document::CadIr;
use crate::report::{
    check::{Check, ValidationReport},
    loss::LossNote,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

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

/// Drop findings whose [`Check`] is outside `allowed`, after admitting the scan.
pub fn filter_checks(mut report: ValidationReport, allowed: &[Check]) -> ValidationReport {
    report
        .findings
        .retain(|finding| allowed.contains(&finding.check));
    report
}

/// Validate under the caller's live session, then retain findings in `allowed`.
pub fn admit(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    allowed: &[Check],
    losses: Vec<LossNote>,
) -> Result<ValidationReport, CodecError> {
    Ok(filter_checks(
        super::validate_model(ctx, ir, losses)?,
        allowed,
    ))
}

/// Admit with borrowed annotations under the caller's live session.
pub fn admit_with_annotations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    annotations: &Annotations,
    allowed: &[Check],
    losses: Vec<LossNote>,
) -> Result<ValidationReport, CodecError> {
    Ok(filter_checks(
        super::validate_model_with_annotations(ctx, ir, annotations, losses)?,
        allowed,
    ))
}

/// Admit while treating staged native identities as resolvable in scoped indexes.
pub fn admit_with_additional_native_identities<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    additional: impl IntoIterator<Item = &'a str>,
    allowed: &[Check],
    losses: Vec<LossNote>,
) -> Result<ValidationReport, CodecError> {
    let index = crate::index::ModelIndex::with_additional_native_identities(ir, additional, ctx)?;
    Ok(filter_checks(
        super::validate_model_with_index(ctx, ir, losses, &index)?,
        allowed,
    ))
}

/// Admit source records through a borrowed replacement of one native unknown arena.
/// Resource refusals remain outside source-product validation failures.
pub fn admit_with_native_unknowns(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    (format, records): (&str, &[crate::unknown::UnknownRecord]),
    annotations: Option<&Annotations>,
    allowed: &[Check],
    losses: Vec<LossNote>,
) -> Result<Result<ValidationReport, crate::native::NativeConvertError>, CodecError> {
    let positions = match native_unknown_order(records) {
        Ok(positions) => positions,
        Err(error) => return Ok(Err(error)),
    };
    let index =
        crate::index::ModelIndex::with_native_unknowns(ir, format, records, &positions, ctx)?;
    let mut report = super::validate_model_with_index(ctx, ir, losses, &index)?;
    if let Some(annotations) = annotations {
        super::validate_annotations(
            ctx,
            &index,
            annotations,
            std::iter::empty(),
            &mut report.findings,
        )?;
    }
    Ok(Ok(filter_checks(report, allowed)))
}

/// Validate a sparse replacement against the combined document with one model index.
pub(crate) fn admit_annotation_delta(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    (format, records): (&str, &[crate::unknown::UnknownRecord]),
    annotations: &crate::annotations::SparseAnnotationTransaction<'_, '_>,
    allowed: &[Check],
) -> Result<Result<ValidationReport, crate::native::NativeConvertError>, CodecError> {
    let positions = match native_unknown_order(records) {
        Ok(positions) => positions,
        Err(error) => return Ok(Err(error)),
    };
    let index =
        crate::index::ModelIndex::with_native_unknowns(ir, format, records, &positions, ctx)?;
    let mut report = super::validate_model_with_index(ctx, ir, Vec::new(), &index)?;
    let mut old = Vec::new();
    super::validate_annotations(
        ctx,
        &index,
        annotations.base(),
        std::iter::empty(),
        &mut old,
    )?;
    for finding in old {
        if let Some(id) = &finding.entity {
            if annotations.selected(id) {
                continue;
            }
        }
        report.findings.push(finding);
    }
    super::validate_annotations(
        ctx,
        &index,
        annotations.annotations(),
        std::iter::empty(),
        &mut report.findings,
    )?;
    Ok(Ok(filter_checks(report, allowed)))
}

/// Check source-product link grammar and distinct identities without validating the model.
/// Raw images and link text remain borrowed throughout the scoped identity sort.
pub fn validate_native_unknowns(
    records: &[crate::unknown::UnknownRecord],
) -> Result<(), crate::native::NativeConvertError> {
    native_unknown_order(records).map(|_| ())
}

/// Positions of `records` in identity order, or the first grammar or
/// duplicate-identity refusal.
fn native_unknown_order(
    records: &[crate::unknown::UnknownRecord],
) -> Result<Vec<usize>, crate::native::NativeConvertError> {
    native_unknown_grammar(records)?;
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_unstable_by(|left, right| {
        records[*left]
            .id()
            .as_str()
            .cmp(records[*right].id().as_str())
    });
    for pair in order.windows(2) {
        let first = records[pair[0]].id().as_str();
        if first == records[pair[1]].id().as_str() {
            return Err(crate::native::NativeConvertError::InvalidCollection(
                format!("duplicate native unknown record {first}"),
            ));
        }
    }
    Ok(order)
}

fn native_unknown_grammar(
    records: &[crate::unknown::UnknownRecord],
) -> Result<(), crate::native::NativeConvertError> {
    for record in records {
        for (position, link) in record.links().iter().enumerate() {
            if !crate::ids::is_valid_identity(link) {
                return Err(crate::native::NativeConvertError::InvalidCollection(
                    format!(
                        "native unknown {} link {position}: identity is invalid: {link:?}",
                        record.id()
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Cached source identity admission. Link edits do not change identity order.
#[derive(Debug)]
pub(crate) struct NativeUnknownAdmission {
    dirty: std::collections::BTreeSet<usize>,
}

impl NativeUnknownAdmission {
    pub(crate) fn build(
        records: &[crate::unknown::UnknownRecord],
    ) -> Result<Self, crate::native::NativeConvertError> {
        native_unknown_order(records)?;
        Ok(Self {
            dirty: std::collections::BTreeSet::new(),
        })
    }

    pub(crate) fn dirty(&mut self, position: usize) {
        self.dirty.insert(position);
    }

    pub(crate) fn check(
        &mut self,
        records: &[crate::unknown::UnknownRecord],
    ) -> Result<Result<(), crate::native::NativeConvertError>, CodecError> {
        for &position in &self.dirty {
            let record = records
                .get(position)
                .ok_or_else(|| CodecError::malformed("changed source record is absent"))?;
            if let Err(error) = native_unknown_grammar(std::slice::from_ref(record)) {
                return Ok(Err(error));
            }
        }
        self.dirty.clear();
        Ok(Ok(()))
    }
}

#[cfg(test)]
mod native_unknown_tests;

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
        assert!(super::super::validate_neutral(&accepted, Vec::new())
            .expect("resource allocation did not fail")
            .is_ok());
        assert!(admit(
            &cadmpeg_test_support::service_decode_context(),
            &accepted,
            DRAFT_CORE_CHECKS,
            Vec::new()
        )
        .expect("resource allocation did not fail")
        .is_ok());

        let missing_point = fixture(rejected_missing_point("test:model").expect("valid identity"));
        assert!(!super::super::validate_neutral(&missing_point, Vec::new())
            .expect("resource allocation did not fail")
            .is_ok());
        assert!(!admit(
            &cadmpeg_test_support::service_decode_context(),
            &missing_point,
            DRAFT_CORE_CHECKS,
            Vec::new()
        )
        .expect("resource allocation did not fail")
        .is_ok());

        let missing_region =
            fixture(rejected_missing_region("test:model").expect("valid identity"));
        assert!(!super::super::validate_neutral(&missing_region, Vec::new())
            .expect("resource allocation did not fail")
            .is_ok());
        assert!(!admit(
            &cadmpeg_test_support::service_decode_context(),
            &missing_region,
            DRAFT_CORE_CHECKS,
            Vec::new()
        )
        .expect("resource allocation did not fail")
        .is_ok());
    }

    #[test]
    fn filter_checks_drops_out_of_set_findings() {
        let ir = fixture(rejected_missing_point("test:model").expect("valid identity"));
        let filtered = admit(
            &cadmpeg_test_support::service_decode_context(),
            &ir,
            &[Check::Identity],
            Vec::new(),
        )
        .expect("resource allocation did not fail");
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
            assert!(admit(
                &cadmpeg_test_support::service_decode_context(),
                &accepted,
                allowed,
                Vec::new()
            )
            .expect("resource allocation did not fail")
            .is_ok());
            assert!(!admit(
                &cadmpeg_test_support::service_decode_context(),
                &rejected,
                allowed,
                Vec::new()
            )
            .expect("resource allocation did not fail")
            .is_ok());
        }
    }

    #[test]
    fn admission_routes_preserve_live_depth_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let ir = crate::CadIr::empty();
        let annotations = crate::annotations::Annotations::default();
        for route in 0..3 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = match route {
                0 => admit(&ctx, &ir, DRAFT_CORE_CHECKS, Vec::new()),
                1 => super::admit_with_annotations(
                    &ctx,
                    &ir,
                    &annotations,
                    RHINO_DRAFT_CHECKS,
                    Vec::new(),
                ),
                _ => super::admit_with_additional_native_identities(
                    &ctx,
                    &ir,
                    std::iter::empty(),
                    DRAFT_CORE_CHECKS,
                    Vec::new(),
                ),
            };
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("live session must refuse admission");
            };
            assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
    }
}
