// SPDX-License-Identifier: Apache-2.0
//! Typed prepare/write conversion workflow.

use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use cadmpeg_ir::codec::{DecodeOptions, EncodeInput, Encoder, ExportPlan, TargetRequest};
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
    /// What the command line asked that encoder to write.
    pub selection: TargetSelection,
}

/// The target the command line named, before the source is known.
///
/// A target flag names a dialect outright. Flag absence is not a target: it
/// means "the same kind of file", which is preservation within one format and
/// the catalog default across formats — and which of the two applies is only
/// known once the source has been read.
#[derive(Debug, Clone)]
pub enum TargetSelection {
    /// A target flag named this dialect id.
    Explicit(String),
    /// No target flag was given.
    Unstated,
}

impl TargetSelection {
    /// Builds the encoder request for a source in `source_format`.
    ///
    /// Flag absence within one format is [`TargetRequest::Inherit`], the
    /// identity default: a no-op round trip keeps the dialect the file already
    /// is instead of silently rewriting it as the encoder's newest. Across
    /// formats there is nothing to inherit, so it is the catalog default. An
    /// encoder with no synthesis catalog — CADIR, which has no dialect at all —
    /// takes `Inherit` either way.
    fn request<'a>(
        &'a self,
        encoder: &dyn Encoder,
        source_format: Option<&str>,
    ) -> TargetRequest<'a> {
        match self {
            Self::Explicit(id) => TargetRequest::Explicit(id),
            Self::Unstated => {
                if source_format == Some(encoder.id()) {
                    return TargetRequest::Inherit;
                }
                match cadmpeg_ir::codec::default_target(encoder.targets()) {
                    Some(id) => TargetRequest::Explicit(id),
                    None => TargetRequest::Inherit,
                }
            }
        }
    }
}

/// The format the document came from, which decides whether flag absence
/// inherits.
fn source_format(ir: &CadIr) -> Option<&str> {
    ir.source.as_ref().map(|source| source.format.as_str())
}

/// Policy controlling validation, loss refusal, and destination rules.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // mirrors the CLI flag surface
pub struct ConversionPolicy {
    /// Replace an existing output or report file.
    pub force: bool,
    /// Stream a binary output format to standard output instead of refusing.
    pub binary_stdout: bool,
    /// Write even if the check finds errors.
    pub allow_errors: bool,
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
    selection: TargetSelection,
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
            if !validation.is_ok() && !policy.allow_errors {
                return Err(ConversionRefusal::CheckFailed {
                    message: format!(
                        "check found {} error(s); refusing to export (use --allow-errors to override)",
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

        let request = target
            .selection
            .request(target.encoder.as_ref(), source_format(&loaded.ir));
        let plan = target
            .encoder
            .plan(EncodeInput::new(&loaded.ir, loaded.fidelity()), request)?;
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
            selection: target.selection,
            destination: policy.destination,
            input: source.path.to_path_buf(),
            force: policy.force,
        })
    }
}

impl PreparedConversion {
    /// Writes the destination artifact and optional CADIR sidecar.
    pub fn write(self) -> Result<ExportReport> {
        let request = self
            .selection
            .request(self.encoder.as_ref(), source_format(&self.document.ir));
        let plan = self.encoder.plan(
            EncodeInput::new(&self.document.ir, self.document.fidelity()),
            request,
        )?;
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
/// Wrong-target flags still fail before the input is read: a flag that names
/// the wrong format is wrong whatever the source turns out to be. Whether the
/// resolved target is available is a different question, and it is answered
/// after the read, by the encoder, because an inherit request cannot be
/// resolved without the source's dialect.
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
            message: "--rhino-target requires Rhino output".into(),
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

    // The selection restates the flag as the dialect id the encoder's catalog
    // uses. A flag that was not given stays unstated: it is the source, not the
    // command line, that decides between preservation and the catalog default.
    let selection = match format {
        Format::Cadir => TargetSelection::Unstated,
        #[cfg(feature = "step")]
        Format::Step => step_options.as_ref().map_or(
            TargetSelection::Unstated,
            |options: &cadmpeg_codec_step::StepWriteOptions| {
                TargetSelection::Explicit(options.schema.target().to_owned())
            },
        ),
        #[cfg(feature = "fcstd")]
        Format::Fcstd => TargetSelection::Unstated,
        #[cfg(feature = "f3d")]
        Format::F3d => TargetSelection::Unstated,
        #[cfg(feature = "sldprt")]
        Format::Sldprt => TargetSelection::Unstated,
        #[cfg(feature = "rhino")]
        Format::Rhino => rhino_version.map_or(TargetSelection::Unstated, |version| {
            TargetSelection::Explicit(version.target().to_owned())
        }),
        #[cfg(feature = "iges")]
        Format::Iges => iges_options.map_or(TargetSelection::Unstated, |options| {
            TargetSelection::Explicit(options.version.target().to_owned())
        }),
    };

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
    Ok(ExportTarget {
        format,
        encoder,
        selection,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::codec::CadirEncoder;

    /// Flag absence is read against the source, not against the format table.
    ///
    /// Within one format it inherits, which is what keeps a no-op round trip
    /// byte-identical. Across formats there is nothing to inherit, so it is the
    /// catalog default. An encoder with no catalog inherits either way.
    #[test]
    fn flag_absence_inherits_only_within_one_format() {
        #[cfg(feature = "iges")]
        {
            let iges = cadmpeg_codec_iges::IgesEncoder::default();
            assert_eq!(
                TargetSelection::Unstated.request(&iges, Some("iges")),
                TargetRequest::Inherit
            );
            assert_eq!(
                TargetSelection::Unstated.request(&iges, Some("step")),
                TargetRequest::Explicit("iges:5.3-fixed-ascii")
            );
            assert_eq!(
                TargetSelection::Unstated.request(&iges, None),
                TargetRequest::Explicit("iges:5.3-fixed-ascii")
            );
            let named = TargetSelection::Explicit("iges:5.1-fixed-ascii".to_owned());
            assert_eq!(
                named.request(&iges, Some("iges")),
                TargetRequest::Explicit("iges:5.1-fixed-ascii")
            );
        }
        assert_eq!(
            TargetSelection::Unstated.request(&CadirEncoder, Some("iges")),
            TargetRequest::Inherit
        );
        assert_eq!(
            TargetSelection::Unstated.request(&CadirEncoder, Some("cadir")),
            TargetRequest::Inherit
        );
    }
}
