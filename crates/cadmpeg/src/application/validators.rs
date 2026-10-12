// SPDX-License-Identifier: Apache-2.0
//! Native validation over a decoded document.
//!
//! Codec-owned validators run over a decoded document's native namespaces.
//! Unlike the codec registry, this is an application concern: it belongs to
//! `cadmpeg check`, not to the four questions an embedder asks of a file.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    report::{
        check::{Finding, ValidationReport},
        Severity,
    },
    validate_neutral, validate_neutral_with_source_fidelity, CadIr, SourceFidelity,
};
use cadmpeg_registry::InputCatalog;

pub(crate) fn validate_ir(
    ctx: &DecodeContext<'_>,
    inputs: &InputCatalog,
    ir: &CadIr,
    source_fidelity: Option<&SourceFidelity>,
    losses: Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<ValidationReport, CodecError> {
    let mut report = match source_fidelity {
        Some(source_fidelity) => validate_neutral_with_source_fidelity(ir, source_fidelity, losses),
        None => validate_neutral(ir, losses),
    }?;
    validate_native(ctx, inputs, ir, &mut report.findings)?;
    Ok(report)
}

/// Runs every registered codec's native validator over the namespace it owns.
fn validate_native(
    ctx: &DecodeContext<'_>,
    inputs: &InputCatalog,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    for descriptor in inputs.descriptors() {
        let Some(codec) = descriptor.codec() else {
            continue;
        };
        if ir.native.namespace(codec.id().as_str()).is_some() {
            ctx.append_vec(
                findings,
                &mut codec.validate_native(ctx, ir)?,
                "native validation findings",
            )?;
        }
    }
    Ok(())
}

/// Tests for an error or blocking finding, admitting only the visited findings.
pub(crate) fn has_errors(
    ctx: &DecodeContext<'_>,
    report: &ValidationReport,
) -> Result<bool, CodecError> {
    ctx.any_by(
        &report.findings,
        |finding| Ok(finding.severity >= Severity::Error),
        "validation verdict",
    )
}

#[cfg(test)]
mod tests {
    use super::{has_errors, validate_native};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::{
        report::{
            check::{Check, Finding, ValidationReport},
            Severity,
        },
        CadIr,
    };
    use cadmpeg_registry::InputCatalog;

    fn findings(inputs: &InputCatalog, ir: &CadIr) -> Vec<cadmpeg_ir::report::check::Finding> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("validation context");
        let mut findings = Vec::new();
        let result = validate_native(&ctx, inputs, ir, &mut findings).map(|()| findings);
        ctx.finish(result).expect("validation fits service policy")
    }

    #[test]
    fn a_document_with_no_native_namespace_has_no_native_findings() {
        let inputs = InputCatalog::with_builtins();
        assert!(findings(&inputs, &CadIr::empty()).is_empty());
    }

    #[test]
    fn an_unregistered_namespace_reaches_no_codec_validator() {
        let inputs = InputCatalog::with_builtins();
        let mut ir = CadIr::empty();
        let _ = ir.native.namespace_mut("absent");
        assert!(findings(&inputs, &ir).is_empty());
    }

    #[cfg(feature = "fcstd")]
    #[test]
    fn a_registered_namespace_reaches_its_own_codec_validator() {
        let inputs = InputCatalog::with_builtins();
        let mut ir = CadIr::empty();
        let _ = ir.native.namespace_mut("fcstd");
        let findings = findings(&inputs, &ir);
        assert!(!findings.is_empty());
        assert!(
            findings
                .iter()
                .any(|finding| finding.message.contains("FCStd")),
            "{findings:?}"
        );
    }

    #[cfg(feature = "fcstd")]
    #[test]
    fn native_report_growth_admits_each_slot_once() {
        let inputs = InputCatalog::with_builtins();
        let mut ir = CadIr::empty();
        let _ = ir.native.namespace_mut("fcstd");
        // The empty namespace fails the physical ledger and byte coverage checks.
        for limit in [2, 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            let result = validate_native(&ctx, &inputs, &ir, &mut findings).map(|()| findings);
            let result = ctx.finish(result);
            if limit == 2 {
                assert_eq!(result.unwrap().len(), 2);
            } else {
                assert!(matches!(result, Err(CodecError::ResourceLimit(ref refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "native validation findings"));
            }
        }
    }

    #[cfg(feature = "fcstd")]
    #[test]
    fn native_report_growth_admits_backing_storage() {
        let inputs = InputCatalog::with_builtins();
        let mut ir = CadIr::empty();
        let _ = ir.native.namespace_mut("fcstd");
        // The destination starts empty; amortized growth reserves four Finding slots.
        let bytes = 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Finding>());
        for limit in [bytes, bytes - 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            let result = validate_native(&ctx, &inputs, &ir, &mut findings).map(|()| findings);
            let result = ctx.finish(result);
            if limit == bytes {
                assert_eq!(result.unwrap().len(), 2);
            } else {
                assert!(matches!(result, Err(CodecError::ResourceLimit(ref refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "native validation findings"));
            }
        }
    }

    #[test]
    fn validation_verdict_admits_only_visited_findings() {
        // A match pays for its visits; a miss also pays for the end probe.
        for (severities, work, invalid) in [
            (vec![], 1, false),
            (
                vec![Severity::Warning, Severity::Error, Severity::Warning],
                2,
                true,
            ),
            (vec![Severity::Blocking, Severity::Warning], 1, true),
            (vec![Severity::Warning, Severity::Warning], 3, false),
        ] {
            let report = ValidationReport {
                entity_counts: std::collections::BTreeMap::default(),
                findings: severities
                    .into_iter()
                    .map(|severity| Finding {
                        check: Check::NativeLinks,
                        severity,
                        message: String::new(),
                        entity: None,
                    })
                    .collect(),
                losses: Vec::new(),
            };
            for limit in [work, work - 1] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_work_units = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = has_errors(&ctx, &report);
                let result = ctx.finish(result);
                if limit == work {
                    assert_eq!(result.unwrap(), invalid);
                } else {
                    assert!(matches!(result, Err(CodecError::ResourceLimit(ref refusal))
                        if refusal.dimension == ResourceDimension::WorkUnits
                        && refusal.operation == "validation verdict"));
                }
            }
        }
    }
}
