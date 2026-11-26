// SPDX-License-Identifier: Apache-2.0
//! Shared neutral, fidelity, native, and measurement validators.
use crate::InputCatalog;
use cadmpeg_core::{
    decode::{DecodeArena, DecodeContext},
    CodecError,
};
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
    let options = MeasureOptions {
        work,
        ..MeasureOptions::default()
    };
    let has_native = inputs
        .descriptors()
        .filter_map(|descriptor| descriptor.codec())
        .any(|codec| document.native.namespace(codec.id().as_str()).is_some());
    let has_measure = !document.geometry.faces.is_empty() || !document.geometry.bodies.is_empty();
    let run_parallel = has_native || has_measure;
    if !run_parallel {
        let mut report = match source {
            Some(source) => {
                validate_neutral_with_source_fidelity_for_decode(ctx, document, source, losses)
            }
            None => validate_neutral_for_decode(ctx, document, losses),
        }?;
        report
            .findings
            .extend(validate_native(ctx, inputs, document, source)?);
        report
            .findings
            .extend(cadmpeg_measure::check_findings(ctx, document, options)?);
        return Ok(report);
    }

    let checkpoint = ctx.checkpoint();
    let losses_for_neutral = losses.clone();
    let (neutral_outcome, native_outcome, measure_outcome) = std::thread::scope(|scope| {
        let neutral_handle = {
            let cp = checkpoint.clone();
            scope.spawn(move || {
                let arena = DecodeArena::new();
                let local_ctx = cp.resume(&arena);
                let res = match source {
                    Some(source) => validate_neutral_with_source_fidelity_for_decode(
                        &local_ctx,
                        document,
                        source,
                        losses_for_neutral,
                    ),
                    None => validate_neutral_for_decode(&local_ctx, document, losses_for_neutral),
                };
                (res, local_ctx.take_trace())
            })
        };
        let measure_handle = has_measure.then(|| {
            let cp = checkpoint.clone();
            let options = options.clone();
            scope.spawn(move || {
                let arena = DecodeArena::new();
                let local_ctx = cp.resume(&arena);
                let res = cadmpeg_measure::check_findings(&local_ctx, document, options);
                (res, local_ctx.take_trace())
            })
        });
        let native = has_native.then(|| {
            let arena = DecodeArena::new();
            let local_ctx = checkpoint.resume(&arena);
            let res = validate_native(&local_ctx, inputs, document, source);
            (res, local_ctx.take_trace())
        });
        let neutral = neutral_handle.join().ok();
        let measure = measure_handle.map(|handle| handle.join().ok());
        (neutral, native, measure)
    });

    let mut report = match neutral_outcome {
        Some((Ok(report), trace)) if ctx.accept_trace(&trace) => report,
        _ => match source {
            Some(source) => {
                validate_neutral_with_source_fidelity_for_decode(ctx, document, source, losses)?
            }
            None => validate_neutral_for_decode(ctx, document, losses)?,
        },
    };
    match native_outcome {
        Some((Ok(findings), trace)) if ctx.accept_trace(&trace) => {
            report.findings.extend(findings);
        }
        Some(_) => {
            report
                .findings
                .extend(validate_native(ctx, inputs, document, source)?);
        }
        None => {}
    }
    match measure_outcome {
        Some(Some((Ok(findings), trace))) if ctx.accept_trace(&trace) => {
            report.findings.extend(findings);
        }
        Some(_) => {
            report
                .findings
                .extend(cadmpeg_measure::check_findings(ctx, document, options)?);
        }
        None => {
            report
                .findings
                .extend(cadmpeg_measure::check_findings(ctx, document, options)?);
        }
    }
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
