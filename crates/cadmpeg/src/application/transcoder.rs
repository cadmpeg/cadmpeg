// SPDX-License-Identifier: Apache-2.0
//! Typed prepare/write conversion workflow.

use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use cadmpeg_ir::codec::{DecodeOptions, EncodeInput, Encoder, ExportPlan, TargetRequest};
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::SourceFidelity;

use cadmpeg_registry::{ForcedInput, Format, InputCatalog};

use crate::application::refusal::classify_decode_failure;
use crate::application::validators::validate_ir;
use crate::application::{
    ArtifactStore, ConversionRefusal, LoadedDocument, NativeValidatorCatalog, SidecarPersistOutcome,
};
use crate::loader::{self, LoadNotice};

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
/// `--to` names a dialect outright. A `--to` that names only a format, or no
/// `--to` at all, is not a target: it means "the same kind of file", and which
/// file that is depends on the source, which the encoder reads. This type is
/// an owned-string adapter for the explicit case and nothing more; it decides
/// no default.
#[derive(Debug, Clone)]
pub enum TargetSelection {
    /// `--to` named this dialect, as a registry id or a catalog alias.
    Explicit(String),
    /// `--to` named no dialect.
    Unstated,
}

impl TargetSelection {
    /// Builds the encoder request.
    ///
    /// Flag absence is [`TargetRequest::Inherit`] unconditionally. What that
    /// resolves to is the encoder's answer, not the command line's:
    /// `resolve_write_request` preserves the source dialect within one format
    /// and selects the catalog default when there is nothing to inherit: no
    /// source or a source of another format. Deciding the cross-format default here as well would be the
    /// same rule written twice, in two places that can drift.
    fn request(&self) -> TargetRequest<'_> {
        match self {
            Self::Explicit(id) => TargetRequest::Explicit(id),
            Self::Unstated => TargetRequest::Inherit,
        }
    }
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
    /// Refuse to export when the decode reported any loss.
    pub reject_decode_losses: bool,
    /// Refuse to export when export planning reported any loss.
    pub reject_export_losses: bool,
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
    reject_export_losses: bool,
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

    /// Loads and validates a conversion without planning or writing it.
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
        if let Some(destination) = &policy.destination {
            ArtifactStore::check_output_path(source.path, destination, policy.force)?;
        }

        let outcome =
            loader::load_artifact(self.inputs, source.path, source.options, source.forced)
                .map_err(classify_decode_failure)?;
        let loaded = outcome.document;
        let notices = outcome.notices;
        let decode_report = loaded.decode_report().cloned();

        if let Some(refusal) =
            decode_lossy_refusal(policy.reject_decode_losses, decode_report.as_ref(), format)
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

        Ok(PreparedConversion {
            document: loaded,
            notices,
            validation,
            format,
            encoder: target.encoder,
            selection: target.selection,
            destination: policy.destination,
            reject_export_losses: policy.reject_export_losses,
        })
    }
}

impl PreparedConversion {
    /// Plans the export and applies the plan-time refusals.
    ///
    /// The plan borrows the loaded document, so it cannot be stored beside it
    /// in one owned value; it lives in the caller's scope instead, between
    /// this call and [`PlannedConversion::write`]. That is what makes one plan
    /// serve both the refusal checks and the write.
    pub fn plan(&self) -> Result<PlannedConversion<'_>> {
        // Resolution is the encoder's, and so is its refusal: the message
        // already names the requested id and the whole catalog, and it
        // reflects this build's feature set. Restating it here would be a
        // second vocabulary to keep in step with the first.
        let plan = match self.encoder.plan(
            EncodeInput::new(&self.document.ir, self.document.fidelity()),
            self.selection.request(),
        ) {
            Ok(plan) => plan,
            Err(error) => {
                return Err(plan_refusal(
                    error,
                    self.document.decode_report().cloned(),
                    self.validation.clone(),
                ))
            }
        };
        if self.reject_export_losses && !plan.report().losses.is_empty() {
            return Err(ConversionRefusal::ExportLossRejected {
                message: format!(
                    "export planning reported {} loss(es): {}; refusing to write a lossy {} (omit --reject-lossy to allow)",
                    plan.report().losses.len(),
                    plan.report()
                        .losses
                        .iter()
                        .map(|loss| loss.message.as_str())
                        .collect::<Vec<_>>()
                        .join("; "),
                    self.format.name()
                ),
                decode_report: self.document.decode_report().cloned(),
                validation: self.validation.clone(),
            }
            .into());
        }
        Ok(PlannedConversion {
            plan,
            prepared: self,
        })
    }
}

/// One planned export, borrowed from the document it was planned against.
pub struct PlannedConversion<'a> {
    plan: ExportPlan<'a>,
    prepared: &'a PreparedConversion,
}

impl PlannedConversion<'_> {
    /// Writes the destination artifact and optional CADIR sidecar.
    pub fn write(self) -> Result<ExportReport> {
        let prepared = self.prepared;
        emit_export_plan(
            self.plan,
            prepared.format,
            prepared.destination.as_deref(),
            prepared.document.decode_report(),
            prepared.document.fidelity(),
        )
    }
}

/// Restates an unsupported target as a typed refusal.
///
/// Every other plan failure stays operational. Export-loss refusal is made
/// only from a completed plan's typed loss rows.
fn plan_refusal(
    error: cadmpeg_core::CodecError,
    decode_report: Option<DecodeReport>,
    validation: Option<ValidationReport>,
) -> anyhow::Error {
    match error {
        cadmpeg_core::CodecError::UnsupportedTarget { .. } => {
            ConversionRefusal::UnsupportedTarget {
                message: error.to_string(),
                decode_report,
                validation,
            }
            .into()
        }
        _ => error.into(),
    }
}

fn losses(report: Option<&DecodeReport>) -> Vec<cadmpeg_ir::LossNote> {
    report
        .map(|report| report.losses.clone())
        .unwrap_or_default()
}

fn decode_lossy_refusal(
    reject_decode_losses: bool,
    report: Option<&DecodeReport>,
    format: Format,
) -> Option<ConversionRefusal> {
    if !reject_decode_losses {
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

/// Emits one built plan to its destination and maintains CADIR sidecars.
pub(crate) fn emit_export_plan(
    plan: ExportPlan<'_>,
    format: Format,
    out: Option<&Path>,
    decode_report: Option<&DecodeReport>,
    source_fidelity: Option<&SourceFidelity>,
) -> Result<ExportReport> {
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

/// Builds an [`ExportTarget`] for one output format and its named dialect.
///
/// `dialect` is the dialect half of `--to`, unresolved: a registry id or a
/// catalog alias, whichever the caller typed. It is not checked here. Whether
/// the encoder can produce it is the encoder's question and is answered after
/// the read, by `plan`, because an inherit request cannot be resolved without
/// the source's dialect and because a single refusal path is what keeps the
/// catalog out of the CLI. The format half is already resolved by the time
/// this runs, so a `--to` naming a format this build cannot write has failed
/// before the input is opened.
///
/// `losses` is a policy, never a target: reading it as one would turn
/// `convert a.step -o b.step --reject-lossy=export` into an explicit AP214
/// request, silently rewriting the schema of a file the caller only asked to
/// check for losses.
#[must_use]
pub fn export_target(format: Format, dialect: Option<&str>) -> ExportTarget {
    ExportTarget {
        format,
        encoder: cadmpeg_registry::build_encoder(format),
        selection: match dialect {
            Some(dialect) => TargetSelection::Explicit(dialect.to_owned()),
            None => TargetSelection::Unstated,
        },
    }
}

#[cfg(test)]
mod tests;
