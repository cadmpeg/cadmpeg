// SPDX-License-Identifier: Apache-2.0
//! Native validation over a decoded document.
//!
//! Codec-owned validators run over a decoded document's native namespaces.
//! Unlike the codec registry, this is an application concern: it belongs to
//! `cadmpeg check`, not to the four questions an embedder asks of a file.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    report::check::{Finding, ValidationReport},
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
    };
    report.findings.extend(validate_native(ctx, inputs, ir)?);
    Ok(report)
}

/// Runs every registered codec's native validator over the namespace it owns.
fn validate_native(
    ctx: &DecodeContext<'_>,
    inputs: &InputCatalog,
    ir: &CadIr,
) -> Result<Vec<Finding>, CodecError> {
    let mut findings = Vec::new();
    for descriptor in inputs.descriptors() {
        let Some(codec) = descriptor.codec() else {
            continue;
        };
        if ir.native.namespace(codec.id().as_str()).is_some() {
            findings.extend(codec.validate_native(ctx, ir)?);
        }
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::validate_native;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::CadIr;
    use cadmpeg_registry::InputCatalog;

    fn findings(inputs: &InputCatalog, ir: &CadIr) -> Vec<cadmpeg_ir::report::check::Finding> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("validation context");
        validate_native(&ctx, inputs, ir).expect("validation fits service policy")
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
}
