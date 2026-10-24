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
    codec::Codec,
    report::check::{Finding, ValidationReport},
    source_fidelity::SourceFidelity,
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
        report.findings.extend(validate_native(
            &ctx,
            inputs,
            &document.ir,
            document.fidelity(),
        )?);
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
    fidelity: Option<&SourceFidelity>,
) -> Result<Vec<Finding>, CodecError> {
    validate_codecs(
        ctx,
        inputs
            .descriptors()
            .filter_map(cadmpeg_registry::InputDescriptor::codec),
        ir,
        fidelity,
    )
}

/// Runs each codec's native validator over the namespace it owns and, when
/// the document carries its decode sidecar, the codec's source-layout
/// validator over that sidecar. Without a sidecar the document holds no
/// source offsets, so no source-layout check applies.
fn validate_codecs<'c>(
    ctx: &DecodeContext<'_>,
    codecs: impl IntoIterator<Item = &'c dyn Codec>,
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
) -> Result<Vec<Finding>, CodecError> {
    let mut findings = Vec::new();
    for codec in codecs {
        if ir.native.namespace(codec.id().as_str()).is_none() {
            continue;
        }
        findings.extend(codec.validate_native(ctx, ir)?);
        if let Some(fidelity) = fidelity {
            findings.extend(codec.validate_source_layout(ctx, ir, fidelity)?);
        }
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::validate_native;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, Decoded, FormatId};
    use cadmpeg_ir::report::check::{Check, Finding};
    use cadmpeg_ir::report::Severity;
    use cadmpeg_ir::source_fidelity::SourceFidelity;
    use cadmpeg_ir::{CadIr, ContainerSummary};
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

    /// A codec whose source-layout validator reports one finding per call,
    /// so a test sees whether the application ran it.
    struct SourceLayoutProbe;

    impl CodecBackend for SourceLayoutProbe {
        const FORMAT: FormatId = FormatId::new("probe");

        fn validate_source_layout(
            _ctx: &DecodeContext<'_>,
            _ir: &CadIr,
            _fidelity: &SourceFidelity,
        ) -> Result<Vec<Finding>, CodecError> {
            Ok(vec![Finding {
                related: Vec::new(),
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "probe source layout checked".into(),
                entity: None,
            }])
        }

        fn detect_impl(
            &self,
            _ctx: &DecodeContext<'_>,
            _prefix: View<'_>,
        ) -> Result<Confidence, CodecError> {
            Ok(Confidence::No)
        }

        fn inspect_impl(
            &self,
            _ctx: &DecodeContext<'_>,
            _root: View<'_>,
        ) -> Result<ContainerSummary, CodecError> {
            Err(CodecError::NotImplemented("probe inspects nothing".into()))
        }

        fn decode_impl(
            &self,
            _ctx: &DecodeContext<'_>,
            _root: View<'_>,
        ) -> Result<Decoded, CodecError> {
            Err(CodecError::NotImplemented("probe decodes nothing".into()))
        }
    }

    fn probe_findings(ir: &CadIr, fidelity: Option<&SourceFidelity>) -> Vec<Finding> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("validation context");
        super::validate_codecs(&ctx, [&SourceLayoutProbe as &dyn Codec], ir, fidelity)
            .expect("validation fits service policy")
    }

    #[test]
    fn the_source_layout_validator_runs_only_with_a_decode_sidecar() {
        let mut ir = CadIr::empty();
        let _ = ir.native.namespace_mut("probe");
        let fidelity = SourceFidelity::default();
        let findings = probe_findings(&ir, Some(&fidelity));
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].message, "probe source layout checked");
        assert!(probe_findings(&ir, None).is_empty());
    }

    #[test]
    fn the_source_layout_validator_needs_its_codec_namespace() {
        let fidelity = SourceFidelity::default();
        assert!(probe_findings(&CadIr::empty(), Some(&fidelity)).is_empty());
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
        // The typed GUI document is readable, but its required GuiDocument.xml
        // archive entry is absent. This violates FCStd's native graph contract;
        // an empty namespace alone has no such inconsistency.
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("native fixture context");
        ir.native
            .namespace_mut("fcstd")
            .set_arena(
                &ctx,
                "gui_documents",
                &[serde_json::json!({
                    "id": "fcstd:gui:document#0",
                    "attributes": {},
                    "states": [],
                })],
            )
            .expect("native GUI document fixture");
        let findings = findings(&inputs, &ir);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let finding = &findings[0];
        assert_eq!(finding.check, cadmpeg_ir::report::check::Check::Counts);
        assert_eq!(finding.severity, cadmpeg_ir::report::Severity::Error);
        assert_eq!(
            finding.message,
            "FCStd GUI document record does not match GuiDocument.xml presence"
        );
        assert_eq!(finding.entity, None);
        assert!(finding.related.is_empty());
    }
}
