// SPDX-License-Identifier: Apache-2.0
//! Application resource session for composed document validation.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::ValidationReport;
use cadmpeg_measure::WorkProfile;
use cadmpeg_registry::InputCatalog;

use super::document::LoadedDocument;
use super::refusal::{ApplicationError, RefusalStage};

/// Validate a loaded document under one session and retain check-stage
/// refusals. `work` sets the allowance of the measurement findings pass.
pub(crate) fn validate_loaded(
    inputs: &InputCatalog,
    document: &LoadedDocument,
    policy: &DecodePolicy,
    work: WorkProfile,
) -> Result<ValidationReport, ApplicationError> {
    let arena = DecodeArena::new();
    let result = (|| {
        let ctx = DecodeContext::for_loaded_input(&arena, policy, document.input_bytes)?;
        let losses = document
            .decode_report()
            .map_or_else(Vec::new, |report| report.losses.clone());
        let report = cadmpeg_registry::check_with_context(
            &ctx,
            inputs,
            &document.ir,
            document.fidelity(),
            losses,
            work,
        )?;
        ctx.finish_session()?;
        Ok::<_, CodecError>(report)
    })();
    result.map_err(|error| ApplicationError::from(error).at_stage(RefusalStage::Check))
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "nx")]
    mod deltas_source;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::CadIr;
    use cadmpeg_registry::validate_native;
    use cadmpeg_registry::InputCatalog;

    fn findings(inputs: &InputCatalog, ir: &CadIr) -> Vec<cadmpeg_ir::report::check::Finding> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("validation context");
        validate_native(&ctx, inputs, ir, None).expect("validation fits service policy")
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
        let error = super::validate_loaded(
            &InputCatalog::with_builtins(),
            &document,
            &policy,
            super::WorkProfile::Service,
        )
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
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let fidelity = cadmpeg_ir::SourceFidelity::default();
        let with_source = validate_native(&ctx, &inputs, &ir, Some(&fidelity)).unwrap();
        assert_eq!(with_source, findings);
    }
}
