// SPDX-License-Identifier: Apache-2.0
//! Typed prepare/write conversion workflow.

use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use cadmpeg_ir::codec::{DecodeOptions, Encoder, ExportPlan};
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::{validate_neutral, validate_neutral_with_source_fidelity, CadIr, SourceFidelity};

use crate::application::{
    ArtifactStore, ConversionRefusal, ForcedInput, InputCatalog, LoadedDocument,
    NativeValidatorCatalog, SidecarPersistOutcome,
};
use crate::loader::{self, LoadNotice};
use crate::Format;

/// Input path and decode options for one conversion.
pub struct SourceRequest<'a> {
    /// Path to the CADIR or native CAD file.
    pub path: &'a Path,
    /// Explicit input format selection.
    pub forced: Option<ForcedInput>,
    /// Decode options.
    pub options: DecodeOptions,
}

/// Output format with an encoder already constructed at the CLI boundary.
pub struct ExportTarget {
    /// Selected output format.
    pub format: Format,
    /// Encoder for that format.
    pub encoder: Box<dyn Encoder>,
}

/// Policy controlling validation, loss refusal, and destination rules.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // mirrors the CLI flag surface
pub struct ConversionPolicy {
    /// Replace an existing output or report file.
    pub force: bool,
    /// Stream a binary output format to standard output instead of refusing.
    pub binary_stdout: bool,
    /// Write output even if validation finds errors.
    pub allow_invalid: bool,
    /// Export a geometry format when decoding transferred no geometry.
    pub allow_empty: bool,
    /// Refuse to export when decode or export planning reported any loss.
    pub reject_lossy: bool,
    /// Destination path; `None` writes the artifact to standard output.
    pub destination: Option<PathBuf>,
}

/// A conversion that has passed every refusal check and is ready to write.
pub struct PreparedConversion {
    /// Loaded source document.
    pub document: LoadedDocument,
    /// Notices for the presentation layer.
    pub notices: Vec<LoadNotice>,
    /// Validation report.
    pub validation: Option<ValidationReport>,
    format: Format,
    encoder: Box<dyn Encoder>,
    destination: Option<PathBuf>,
    input: PathBuf,
    force: bool,
}

/// Application workflow that prepares and writes conversions.
pub struct Transcoder<'a> {
    /// Input detection and codec lookup.
    pub inputs: &'a InputCatalog,
    /// Native namespace validators.
    pub validators: &'a NativeValidatorCatalog,
}

impl<'a> Transcoder<'a> {
    /// Creates a transcoder over the given catalogs.
    pub const fn new(inputs: &'a InputCatalog, validators: &'a NativeValidatorCatalog) -> Self {
        Self { inputs, validators }
    }

    /// Loads, validates, and plans a conversion without writing the destination.
    ///
    /// Typed refusals are returned as [`ConversionRefusal`] inside `anyhow`.
    /// Operational load failures are plain `anyhow` errors. Pure with respect
    /// to presentation and destination artifact writes; an explicitly requested
    /// command `--report` may still be written by the CLI after a
    /// loss/validation/empty-geometry refusal.
    pub fn prepare(
        &self,
        source: &SourceRequest<'_>,
        target: ExportTarget,
        policy: ConversionPolicy,
    ) -> Result<PreparedConversion> {
        let format = target.format;
        if format.is_binary_container() && policy.destination.is_none() && !policy.binary_stdout {
            return Err(ConversionRefusal::BinaryStdoutRejected {
                message: format!(
                    "refusing to write binary {name} to standard output; pass -o FILE.{name}, or \
                     --input-format {name} (alias --from) if you meant to force how the INPUT is \
                     read; pass --binary-stdout to stream the bytes anyway",
                    name = format.name()
                ),
            }
            .into());
        }

        let outcome =
            loader::load_artifact(self.inputs, source.path, source.options, source.forced)?;
        let loaded = outcome.document;
        let notices = outcome.notices;
        let decode_report = loaded.decode_report().cloned();

        if let Some(refusal) =
            decode_lossy_refusal(policy.reject_lossy, decode_report.as_ref(), format)
        {
            return Err(refusal.into());
        }

        let validation = {
            let validation = validate_ir(
                self.validators,
                &loaded.ir,
                loaded.fidelity(),
                losses(decode_report.as_ref()),
            );
            if !validation.is_ok() && !policy.allow_invalid {
                return Err(ConversionRefusal::ValidationFailed {
                    message: format!(
                        "validation found {} error(s); refusing to export (use --allow-invalid to override)",
                        validation.error_count()
                    ),
                    decode_report,
                    validation,
                }
                .into());
            }
            Some(validation)
        };

        if format.is_geometry_export()
            && decode_report
                .as_ref()
                .is_some_and(|report| !report.geometry_transferred)
            && !policy.allow_empty
        {
            return Err(ConversionRefusal::EmptyGeometry {
                message: format!(
                    "decode transferred no geometry; refusing to write an empty {} (use --allow-empty to override)",
                    format.name()
                ),
                decode_report,
                validation,
            }
            .into());
        }

        let plan = target.encoder.plan(cadmpeg_ir::codec::EncodeInput {
            ir: &loaded.ir,
            fidelity: loaded.fidelity(),
        })?;
        if policy.reject_lossy && !plan.report().losses.is_empty() {
            return Err(ConversionRefusal::ExportLossRejected {
                message: format!(
                    "export planning reported {} loss(es); refusing to write a lossy {} (omit --reject-lossy to allow)",
                    plan.report().losses.len(),
                    format.name()
                ),
                decode_report,
                validation,
            }
            .into());
        }
        drop(plan);

        Ok(PreparedConversion {
            document: loaded,
            notices,
            validation,
            format,
            encoder: target.encoder,
            destination: policy.destination,
            input: source.path.to_path_buf(),
            force: policy.force,
        })
    }
}

impl PreparedConversion {
    /// Writes the destination artifact and optional CADIR sidecar.
    pub fn write(self) -> Result<ExportReport> {
        let plan = self.encoder.plan(cadmpeg_ir::codec::EncodeInput {
            ir: &self.document.ir,
            fidelity: self.document.fidelity(),
        })?;
        write_export_plan(
            plan,
            self.format,
            self.destination.as_deref(),
            &self.input,
            self.force,
            self.document.decode_report(),
            self.document.fidelity(),
        )
    }
}

fn validate_ir(
    validators: &NativeValidatorCatalog,
    ir: &CadIr,
    source_fidelity: Option<&SourceFidelity>,
    losses: Vec<cadmpeg_ir::LossNote>,
) -> ValidationReport {
    let mut report = match source_fidelity {
        Some(source_fidelity) => validate_neutral_with_source_fidelity(ir, source_fidelity, losses),
        None => validate_neutral(ir, losses),
    };
    report.findings.extend(validators.validate(ir));
    report
}

fn losses(report: Option<&DecodeReport>) -> Vec<cadmpeg_ir::LossNote> {
    report
        .map(|report| report.losses.clone())
        .unwrap_or_default()
}

fn decode_lossy_refusal(
    reject_lossy: bool,
    report: Option<&DecodeReport>,
    format: Format,
) -> Option<ConversionRefusal> {
    if !reject_lossy {
        return None;
    }
    let report = report?;
    let count = report.losses.len();
    (count > 0).then(|| ConversionRefusal::DecodeLossRejected {
        message: format!(
            "decode reported {count} loss(es); refusing to write a lossy {} (omit --reject-lossy to allow)",
            format.name()
        ),
        decode_report: report.clone(),
    })
}

fn write_export_plan(
    plan: ExportPlan<'_>,
    format: Format,
    out: Option<&Path>,
    input: &Path,
    force: bool,
    decode_report: Option<&DecodeReport>,
    source_fidelity: Option<&SourceFidelity>,
) -> Result<ExportReport> {
    if let Some(path) = out {
        ArtifactStore::check_output_path(input, path, force)?;
    }
    let needs_sidecar_digest =
        format == Format::Cadir && decode_report.is_some() && source_fidelity.is_some();
    let report = if let Some(path) = out {
        let (report, cadir_sha256) =
            ArtifactStore::write_plan_atomic(path, plan, needs_sidecar_digest)?;
        if format == Format::Cadir {
            match ArtifactStore::persist_decode_sidecar(
                path,
                cadir_sha256.as_deref(),
                decode_report,
                source_fidelity,
            )? {
                SidecarPersistOutcome::Wrote(sidecar) => {
                    eprintln!("wrote decode sidecar {}", sidecar.display());
                }
                SidecarPersistOutcome::RemovedStale(sidecar) => {
                    eprintln!("removed stale decode sidecar {}", sidecar.display());
                }
                SidecarPersistOutcome::Absent => {}
            }
        }
        eprintln!(
            "wrote {} ({} entities)",
            path.display(),
            report.census.total()
        );
        report
    } else {
        let stdout = io::stdout().lock();
        let mut writer = BufWriter::with_capacity(64 * 1024, stdout);
        let report = plan.write_to(&mut writer)?;
        writer.flush()?;
        if format == Format::Cadir && decode_report.is_some() && source_fidelity.is_some() {
            eprintln!("note: CADIR written to stdout cannot carry its decode-fidelity sidecar");
        }
        report
    };
    if !report.losses.is_empty() {
        eprintln!("{} export losses:", report.format);
        for loss in &report.losses {
            eprintln!(
                "  [{}/{}] {}",
                loss.severity,
                loss.code.category(),
                loss.message
            );
        }
    }
    Ok(report)
}

/// Builds an [`ExportTarget`] after checking format-specific flags.
///
/// Wrong-target flags fail before the input is read.
#[allow(clippy::result_large_err)] // refusal carries report context for --report
pub fn export_target(
    format: Format,
    #[cfg(feature = "step")] step_options: Option<cadmpeg_codec_step::StepWriteOptions>,
    #[cfg(feature = "step")] step_flag_present: bool,
    #[cfg(feature = "iges")] iges_options: Option<cadmpeg_codec_iges::IgesWriteOptions>,
    #[cfg(feature = "rhino")] rhino_version: Option<cadmpeg_codec_rhino::RhinoArchiveVersion>,
) -> Result<ExportTarget, ConversionRefusal> {
    #[cfg(feature = "rhino")]
    if rhino_version.is_some() && format != Format::Rhino {
        return Err(ConversionRefusal::UnsupportedTarget {
            message: "--rhino-version requires Rhino output".into(),
        });
    }
    #[cfg(feature = "iges")]
    if iges_options.is_some() && format != Format::Iges {
        return Err(ConversionRefusal::UnsupportedTarget {
            message: "--iges-target requires IGES output".into(),
        });
    }
    #[cfg(feature = "step")]
    if step_flag_present && format != Format::Step {
        return Err(ConversionRefusal::UnsupportedTarget {
            message: "--step-target/--reject-step-losses require STEP output".into(),
        });
    }

    let request = match format {
        Format::Cadir => crate::application::EncoderRequest::Neutral,
        #[cfg(feature = "step")]
        Format::Step => crate::application::EncoderRequest::Step(step_options.unwrap_or_default()),
        #[cfg(feature = "fcstd")]
        Format::Fcstd => crate::application::EncoderRequest::Neutral,
        #[cfg(feature = "f3d")]
        Format::F3d => crate::application::EncoderRequest::Neutral,
        #[cfg(feature = "sldprt")]
        Format::Sldprt => crate::application::EncoderRequest::Neutral,
        #[cfg(feature = "rhino")]
        Format::Rhino => crate::application::EncoderRequest::Rhino(
            rhino_version.unwrap_or(cadmpeg_codec_rhino::RhinoArchiveVersion::V8),
        ),
        #[cfg(feature = "iges")]
        Format::Iges => crate::application::EncoderRequest::Iges(iges_options.unwrap_or_default()),
    };
    let encoder = crate::application::build_encoder(format, request).map_err(|error| {
        ConversionRefusal::UnsupportedTarget {
            message: error.to_string(),
        }
    })?;
    Ok(ExportTarget { format, encoder })
}
