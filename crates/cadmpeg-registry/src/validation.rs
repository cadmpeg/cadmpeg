// SPDX-License-Identifier: Apache-2.0
//! Shared neutral, fidelity, native, and measurement validators.
use crate::InputCatalog;
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::validate::{
    validate_neutral_for_decode, validate_neutral_with_source_fidelity_for_decode,
};
use cadmpeg_ir::{
    report::{
        check::{Finding, ValidationReport},
        loss::LossNote,
    },
    CadIr, SourceFidelity,
};
use cadmpeg_measure::{MeasureOptions, WorkProfile};

/// Compose every validator used by document checks under the caller's session.
pub fn check_with_context(
    ctx: &DecodeContext<'_>,
    inputs: &InputCatalog,
    document: &CadIr,
    source: Option<&SourceFidelity>,
    losses: Vec<LossNote>,
    work: WorkProfile,
) -> Result<ValidationReport, CodecError> {
    let mut report = match source {
        Some(source) => {
            validate_neutral_with_source_fidelity_for_decode(ctx, document, source, losses)
        }
        None => validate_neutral_for_decode(ctx, document, losses),
    }?;
    report
        .findings
        .extend(validate_native(ctx, inputs, document, source)?);
    let options = MeasureOptions {
        work,
        density: None,
    };
    report
        .findings
        .extend(cadmpeg_measure::findings(ctx, document, options)?);
    report
        .findings
        .extend(cadmpeg_measure::declared_findings(ctx, document, options)?);
    Ok(report)
}

/// Run the validator for each registered native namespace present in the document.
pub fn validate_native(
    ctx: &DecodeContext<'_>,
    inputs: &InputCatalog,
    document: &CadIr,
    source: Option<&SourceFidelity>,
) -> Result<Vec<Finding>, CodecError> {
    let mut findings = Vec::new();
    for descriptor in inputs.descriptors() {
        let Some(codec) = descriptor.codec() else {
            continue;
        };
        if document.native.namespace(codec.id().as_str()).is_some() {
            findings.extend(codec.validate_native(ctx, document, source)?);
        }
    }
    Ok(findings)
}
