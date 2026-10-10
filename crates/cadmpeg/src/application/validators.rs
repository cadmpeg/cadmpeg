// SPDX-License-Identifier: Apache-2.0
//! Native validation over a decoded document.
//!
//! Codec-owned validators run over a decoded document's native namespaces.
//! Unlike the codec registry, this is an application concern: it belongs to
//! `cadmpeg check`, not to the four questions an embedder asks of a file.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::validate::{
    validate_neutral_for_decode, validate_neutral_with_source_fidelity_for_decode,
};
use cadmpeg_ir::{
    report::check::{Finding, ValidationReport},
    CadIr,
};
use cadmpeg_registry::InputCatalog;

use super::document::LoadedDocument;
use super::refusal::{ApplicationError, RefusalStage};

/// Validate a loaded document under one session and retain check-stage refusals.
pub(crate) fn validate_loaded(
    inputs: &InputCatalog,
    document: &LoadedDocument,
    policy: &DecodePolicy,
) -> Result<ValidationReport, ApplicationError> {
    let arena = DecodeArena::new();
    let result = (|| {
        let ctx = DecodeContext::for_loaded_input(&arena, policy, document.input_bytes)?;
        let losses = document
            .decode_report()
            .map_or_else(Vec::new, |report| report.losses.clone());
        let mut report = match document.fidelity() {
            Some(fidelity) => validate_neutral_with_source_fidelity_for_decode(
                &ctx,
                &document.ir,
                fidelity,
                losses,
            ),
            None => validate_neutral_for_decode(&ctx, &document.ir, losses),
        }?;
        report
            .findings
            .extend(validate_native(&ctx, inputs, &document.ir)?);
        ctx.finish_session()?;
        Ok::<_, CodecError>(report)
    })();
    result.map_err(|error| ApplicationError::from(error).at_stage(RefusalStage::Check))
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
    fn validation_resource_refusal_names_the_check_stage() {
        let document = super::LoadedDocument::neutral(CadIr::empty(), 5);
        let mut policy = DecodePolicy::service();
        policy.limits.max_input_bytes = 4;
        let error = super::validate_loaded(&InputCatalog::with_builtins(), &document, &policy)
            .expect_err("validation must admit the loaded input length");
        assert_eq!(error.exit_code(), 2);
        let report =
            serde_json::to_value(error.refusal().expect("typed resource refusal").report())
                .expect("serialize resource evidence");
        assert_eq!(report["stage"], "check");
        assert_eq!(report["resource"]["dimension"], "input_bytes");
        assert_eq!(report["resource"]["operation"], "admit loaded input length");
        assert_eq!(report["resource"]["limit"], 4);
        assert_eq!(report["resource"]["used"], 0);
        assert_eq!(report["resource"]["requested"], 5);
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
