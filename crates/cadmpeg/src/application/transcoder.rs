// SPDX-License-Identifier: Apache-2.0
//! Typed prepare/write conversion workflow.

use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result as AnyResult};
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, ExportPlan, TargetRequest};
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::SourceFidelity;

use cadmpeg_registry::{ForcedInput, Format, InputCatalog};

use crate::application::refusal::ApplicationError;
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
    /// Encoder for that format.
    pub encoder: Box<dyn Encoder>,
    /// What the command line asked that encoder to write.
    pub selection: TargetSelection,
}

/// The output format and target token selected by the command line.
///
/// `--to` names a dialect outright. A `--to` that names only a format, or no
/// `--to` at all, leaves `request` unstated so the encoder inherits from the
/// source. The encoder resolves explicit tokens, source-dependent
/// preservation, and delivery during planning.
#[derive(Debug, Clone)]
pub struct TargetSelection {
    /// Selected output format.
    pub format: Format,
    /// Dialect token from `--to`, as a local id or catalog alias.
    pub(crate) request: Option<String>,
}

impl TargetSelection {
    /// Creates an owned output selection at the command-line boundary.
    #[must_use]
    pub fn new(format: Format, request: Option<String>) -> Self {
        Self { format, request }
    }

    /// Resolves the format half of the `--to` grammar against the output path.
    /// The encoder admits the dialect half during planning.
    pub fn resolve(to: Option<&str>, out: Option<&Path>) -> Result<Self, ApplicationError> {
        let inferred = format_from_path(out);
        Ok(match to {
            None => Self::new(
                inferred.ok_or_else(|| {
                    anyhow!("cannot infer format from the output path; pass --to FORMAT")
                })?,
                None,
            ),
            Some(value) => Self::resolve_value(value, inferred)?,
        })
    }

    fn resolve_value(value: &str, inferred: Option<Format>) -> Result<Self, ApplicationError> {
        if let Some((left, right)) = value.split_once(':') {
            let format = Format::from_name(left).ok_or_else(|| {
                ConversionRefusal::UnsupportedOutputFormat {
                    message: format!(
                        "--to {value}: {left} is not an output format of this build; available: {}",
                        Format::vocabulary()
                    ),
                }
            })?;
            if right.is_empty() {
                return Err(anyhow!(
                    "--to {value}: nothing after the colon; write --to {left} for the format alone"
                )
                .into());
            }
            warn_on_extension_disagreement(format, inferred);
            return Ok(Self::new(format, Some(right.to_owned())));
        }
        if let Some(format) = Format::from_name(value) {
            warn_on_extension_disagreement(format, inferred);
            return Ok(Self::new(format, None));
        }
        if Format::is_known_name(value) {
            return Err(ConversionRefusal::UnsupportedOutputFormat {
                message: format!(
                    "--to {value}: {value} is not an output format of this build; available: {}",
                    Format::vocabulary()
                ),
            }
            .into());
        }
        let format = inferred.ok_or_else(|| {
            anyhow!(
                "--to {value}: not an output format of this build ({}), and no output path to read a format from; write --to FORMAT:{value}",
                Format::vocabulary()
            )
        })?;
        Ok(Self::new(format, Some(value.to_owned())))
    }

    /// Builds the encoder request.
    ///
    /// Flag absence is [`TargetRequest::Inherit`] unconditionally. What that
    /// resolves to is the encoder's answer, not the command line's:
    /// [`Encoder::plan`](cadmpeg_ir::codec::write::Encoder::plan) preserves the source
    /// dialect within one format and selects the catalog default when there is
    /// nothing to inherit: no source or a source of another format. Deciding
    /// the cross-format default here as well would write the same rule twice.
    fn request(&self) -> TargetRequest<'_> {
        match self.request.as_deref() {
            Some(id) => TargetRequest::Explicit(id),
            None => TargetRequest::Inherit,
        }
    }
}

fn format_from_path(path: Option<&Path>) -> Option<Format> {
    path.and_then(Path::extension)
        .and_then(|extension| extension.to_str())
        .and_then(Format::from_extension)
}

fn warn_on_extension_disagreement(named: Format, inferred: Option<Format>) {
    if let Some(inferred) = inferred.filter(|inferred| *inferred != named) {
        eprintln!(
            "warning: explicit format {} disagrees with output extension format {}; using {}",
            named.name(),
            inferred.name(),
            named.name()
        );
    }
}

/// Which conversion losses refuse the conversion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LossPolicy {
    /// Permit losses at both phases.
    #[default]
    Allow,
    /// Refuse decode losses only.
    RejectDecode,
    /// Refuse export losses only.
    RejectExport,
    /// Refuse losses at either phase.
    RejectAny,
}

impl LossPolicy {
    /// Whether a decode loss refuses the conversion.
    #[must_use]
    pub const fn rejects_decode(self) -> bool {
        matches!(self, Self::RejectDecode | Self::RejectAny)
    }

    /// Whether an export loss refuses the conversion.
    #[must_use]
    pub const fn rejects_export(self) -> bool {
        matches!(self, Self::RejectExport | Self::RejectAny)
    }
}

/// Validation and geometry admission for a conversion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ValidationAdmission {
    /// Require a valid document with transferred geometry.
    #[default]
    Strict,
    /// Admit validation errors.
    AllowErrors,
    /// Admit a geometry export with no transferred geometry.
    AllowEmpty,
    /// Admit both validation errors and empty geometry.
    AllowErrorsAndEmpty,
}

impl ValidationAdmission {
    /// Constructs the admission mode from the independent CLI overrides.
    #[must_use]
    pub const fn new(allow_errors: bool, allow_empty: bool) -> Self {
        match (allow_errors, allow_empty) {
            (false, false) => Self::Strict,
            (true, false) => Self::AllowErrors,
            (false, true) => Self::AllowEmpty,
            (true, true) => Self::AllowErrorsAndEmpty,
        }
    }

    const fn admits_errors(self) -> bool {
        matches!(self, Self::AllowErrors | Self::AllowErrorsAndEmpty)
    }

    const fn admits_empty(self) -> bool {
        matches!(self, Self::AllowEmpty | Self::AllowErrorsAndEmpty)
    }
}

/// Destination and overwrite rules for a conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestinationPolicy {
    /// Write to standard output.
    Stdout {
        /// Permit a binary format on standard output.
        allow_binary: bool,
    },
    /// Write to a file.
    File {
        /// Output path.
        path: PathBuf,
        /// Replace an existing output.
        overwrite: bool,
    },
}

impl DestinationPolicy {
    /// Resolves CLI destination flags into a destination-specific policy.
    #[must_use]
    pub fn new(destination: Option<PathBuf>, overwrite: bool, binary_stdout: bool) -> Self {
        match destination {
            Some(path) => Self::File { path, overwrite },
            None => Self::Stdout {
                allow_binary: binary_stdout,
            },
        }
    }

    /// Returns the output path used for format inference, if any.
    #[must_use]
    pub(crate) fn path(&self) -> Option<&Path> {
        match self {
            Self::Stdout { .. } => None,
            Self::File { path, .. } => Some(path),
        }
    }

    /// Applies format-only destination admission before input decode.
    fn admit_format(&self, format: Format) -> Result<(), ApplicationError> {
        match self {
            Self::Stdout {
                allow_binary: false,
            } if format.is_binary() => Err(ConversionRefusal::BinaryStdoutRejected {
                message: format!(
                    "refusing to write binary {name} to standard output; pass -o FILE.{name}, or \
                     --input-format {name} (alias --from) if you meant to force how the INPUT is \
                     read; pass --binary-stdout to stream the bytes anyway",
                    name = format.name()
                ),
            }
            .into()),
            Self::Stdout { .. } | Self::File { .. } => Ok(()),
        }
    }

    /// Resolves filesystem-dependent destination rules. A missing source is
    /// left for the loader; an existing destination still refuses before the
    /// potentially expensive decode.
    fn resolve(&self, source: &Path) -> AnyResult<ResolvedDestination> {
        match self {
            Self::Stdout { .. } => Ok(ResolvedDestination::Stdout),
            Self::File { path, overwrite } => {
                ArtifactStore::check_output_path(source, path, *overwrite)?;
                Ok(ResolvedDestination::File(path.clone()))
            }
        }
    }
}

/// Policy controlling the independent conversion phases.
#[derive(Debug, Clone)]
pub struct ConversionPolicy {
    /// Decode and export loss refusal.
    pub losses: LossPolicy,
    /// Validation and empty-geometry admission.
    pub admission: ValidationAdmission,
    /// Output destination rules.
    pub destination: DestinationPolicy,
}

#[derive(Debug, Clone)]
enum ResolvedDestination {
    Stdout,
    File(PathBuf),
}

impl ResolvedDestination {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::Stdout => None,
            Self::File(path) => Some(path),
        }
    }
}

/// A conversion that has passed every refusal check and is ready to write.
pub struct PreparedConversion {
    /// Loaded source document.
    pub document: LoadedDocument,
    /// Notices for the presentation layer.
    pub notices: Vec<LoadNotice>,
    /// Validation report.
    pub validation: Option<ValidationReport>,
    encoder: Box<dyn Encoder>,
    selection: TargetSelection,
    destination: ResolvedDestination,
    loss_policy: LossPolicy,
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
    /// Typed refusals and operational failures remain distinct. Pure with
    /// respect to presentation and destination artifact writes; an explicitly
    /// requested command `--report` may still be written by the CLI after a
    /// loss/validation/empty-geometry refusal.
    pub fn prepare(
        &self,
        source: &SourceRequest<'_>,
        target: ExportTarget,
        policy: &ConversionPolicy,
    ) -> Result<PreparedConversion, ApplicationError> {
        let format = target.selection.format;
        policy.destination.admit_format(format)?;
        let destination = policy
            .destination
            .resolve(source.path)
            .map_err(ApplicationError::from)?;

        let outcome =
            loader::load_artifact(self.inputs, source.path, source.options, source.forced)?;
        let loaded = outcome.document;
        let notices = outcome.notices;
        let decode_report = loaded.decode_report().cloned();

        if let Some(refusal) = decode_lossy_refusal(policy.losses, decode_report.as_ref(), format) {
            return Err(refusal.into());
        }

        let validation = {
            let validation = validate_ir(
                self.validators,
                &loaded.ir,
                loaded.fidelity(),
                losses(decode_report.as_ref()),
            );
            if !validation.is_ok() && !policy.admission.admits_errors() {
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

        if format.transfers_geometry()
            && decode_report
                .as_ref()
                .is_some_and(|report| !report.geometry_transferred())
            && !policy.admission.admits_empty()
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
            encoder: target.encoder,
            selection: target.selection,
            destination,
            loss_policy: policy.losses,
        })
    }
}

impl PreparedConversion {
    /// Plans the export and applies the plan-time refusals.
    pub fn plan(self) -> Result<PlannedConversion, ApplicationError> {
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
        if self.loss_policy.rejects_export() && !plan.report().losses.is_empty() {
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
                    self.selection.format.name()
                ),
                decode_report: self.document.decode_report().cloned(),
                validation: self.validation.clone(),
                export_report: plan.report().clone(),
            }
            .into());
        }
        Ok(PlannedConversion {
            plan,
            prepared: self,
        })
    }
}

/// One planned export and the source state from which it was produced.
pub struct PlannedConversion {
    plan: ExportPlan,
    prepared: PreparedConversion,
}

impl PlannedConversion {
    /// Returns the loaded source state retained for reporting.
    #[must_use]
    pub const fn prepared(&self) -> &PreparedConversion {
        &self.prepared
    }

    /// Writes the destination artifact and optional CADIR sidecar.
    pub fn write(self) -> AnyResult<ExportEmission> {
        emit_export_plan(
            self.plan,
            self.prepared.selection.format,
            self.prepared.destination.path(),
            self.prepared.document.decode_report(),
            self.prepared.document.fidelity(),
        )
    }
}

/// Artifact state produced by one completed export emission.
pub(crate) struct ExportEmission {
    pub(crate) report: ExportReport,
    pub(crate) artifact: EmittedArtifact,
}

/// Destination-specific facts needed by the presentation layer.
pub(crate) enum EmittedArtifact {
    /// A file and its CADIR sidecar disposition.
    File {
        path: PathBuf,
        sidecar: SidecarPersistOutcome,
    },
    /// Standard output with no sidecar requirement.
    Stdout,
    /// CADIR on standard output, where the required sidecar cannot travel.
    StdoutWithoutSidecar,
}

/// Restates an unsupported target as a typed refusal.
///
/// Every other plan failure stays operational. Export-loss refusal is made
/// only from a completed plan's typed loss rows.
fn plan_refusal(
    error: cadmpeg_core::CodecError,
    decode_report: Option<DecodeReport>,
    validation: Option<ValidationReport>,
) -> ApplicationError {
    match error {
        cadmpeg_core::CodecError::UnsupportedTarget(refusal) => {
            ConversionRefusal::UnsupportedTarget {
                refusal,
                decode_report,
                validation,
            }
            .into()
        }
        _ => ApplicationError::Operational(error.into()),
    }
}

fn losses(report: Option<&DecodeReport>) -> Vec<cadmpeg_ir::LossNote> {
    report
        .map(|report| report.losses.clone())
        .unwrap_or_default()
}

fn decode_lossy_refusal(
    policy: LossPolicy,
    report: Option<&DecodeReport>,
    format: Format,
) -> Option<ConversionRefusal> {
    if !policy.rejects_decode() {
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
///
/// This operation performs artifact I/O only. The returned outcome carries
/// every fact the command layer needs for presentation.
pub(crate) fn emit_export_plan(
    plan: ExportPlan,
    format: Format,
    out: Option<&Path>,
    decode_report: Option<&DecodeReport>,
    source_fidelity: Option<&SourceFidelity>,
) -> AnyResult<ExportEmission> {
    let needs_sidecar_digest =
        format == Format::Cadir && decode_report.is_some() && source_fidelity.is_some();
    if let Some(path) = out {
        let (report, cadir_sha256) =
            ArtifactStore::write_plan_atomic(path, plan, needs_sidecar_digest)?;
        let sidecar = if format == Format::Cadir {
            ArtifactStore::persist_decode_sidecar(
                path,
                cadir_sha256.as_deref(),
                decode_report,
                source_fidelity,
            )?
        } else {
            SidecarPersistOutcome::Absent
        };
        Ok(ExportEmission {
            report,
            artifact: EmittedArtifact::File {
                path: path.to_owned(),
                sidecar,
            },
        })
    } else {
        let stdout = io::stdout().lock();
        let mut writer = BufWriter::with_capacity(64 * 1024, stdout);
        let report = plan.write_to(&mut writer)?;
        writer.flush()?;
        Ok(ExportEmission {
            report,
            artifact: if needs_sidecar_digest {
                EmittedArtifact::StdoutWithoutSidecar
            } else {
                EmittedArtifact::Stdout
            },
        })
    }
}

/// Builds an [`ExportTarget`] from the command-line selection.
///
/// This constructs the selected encoder without interpreting its dialect
/// token. The encoder's sealed `plan` resolves catalog membership,
/// source-dependent inheritance, and input-conditioned delivery once.
///
/// `losses` is a policy, never a target: reading it as one would turn
/// `convert a.step -o b.step --reject-lossy=export` into an explicit AP214
/// request, silently rewriting the schema of a file the caller only asked to
/// check for losses.
pub fn export_target(selection: TargetSelection) -> ExportTarget {
    let encoder = cadmpeg_registry::build_encoder(selection.format);
    ExportTarget { encoder, selection }
}

#[cfg(test)]
mod tests;
