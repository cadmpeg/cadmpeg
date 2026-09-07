// SPDX-License-Identifier: Apache-2.0
//! Native validation over a decoded document.
//!
//! Codec-owned validators run over a decoded document's native namespaces.
//! Unlike the codec registry, this is an application concern: it belongs to
//! `cadmpeg check`, not to the four questions an embedder asks of a file.

use cadmpeg_ir::{
    validate_neutral, validate_neutral_with_source_fidelity, CadIr, Finding, SourceFidelity,
    ValidationReport,
};
use cadmpeg_registry::InputCatalog;

pub(crate) fn validate_ir(
    inputs: &InputCatalog,
    ir: &CadIr,
    source_fidelity: Option<&SourceFidelity>,
    losses: Vec<cadmpeg_ir::LossNote>,
) -> ValidationReport {
    let mut report = match source_fidelity {
        Some(source_fidelity) => validate_neutral_with_source_fidelity(ir, source_fidelity, losses),
        None => validate_neutral(ir, losses),
    };
    report.findings.extend(validate_native(inputs, ir));
    report
}

/// Runs every registered codec's native validator over the namespace it owns.
pub fn validate_native(inputs: &InputCatalog, ir: &CadIr) -> Vec<Finding> {
    inputs
        .descriptors()
        .filter_map(|descriptor| {
            let codec = descriptor.codec()?;
            ir.native
                .namespace(codec.id().as_str())
                .is_some()
                .then(|| codec.validate_native(ir))
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::CadIr;

    #[test]
    fn a_document_with_no_native_namespace_has_no_native_findings() {
        let inputs = InputCatalog::with_builtins();
        assert!(validate_native(&inputs, &CadIr::empty()).is_empty());
    }

    #[test]
    fn an_unregistered_namespace_reaches_no_codec_validator() {
        let inputs = InputCatalog::with_builtins();
        let mut ir = CadIr::empty();
        let _ = ir.native.namespace_mut("absent");
        assert!(validate_native(&inputs, &ir).is_empty());
    }

    #[cfg(feature = "fcstd")]
    #[test]
    fn a_registered_namespace_reaches_its_own_codec_validator() {
        let inputs = InputCatalog::with_builtins();
        let mut ir = CadIr::empty();
        let _ = ir.native.namespace_mut("fcstd");
        let findings = validate_native(&inputs, &ir);
        assert!(!findings.is_empty());
        assert!(
            findings
                .iter()
                .any(|finding| finding.message.contains("FCStd")),
            "{findings:?}"
        );
    }
}
