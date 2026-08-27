// SPDX-License-Identifier: Apache-2.0
//! Typed prepare/write conversion workflow.

use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use cadmpeg_ir::codec::{DecodeOptions, EncodeInput, Encoder, ExportPlan, TargetRequest};
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::{validate_neutral, validate_neutral_with_source_fidelity, CadIr, SourceFidelity};

use crate::application::{
    ArtifactStore, ConversionRefusal, ForcedInput, InputCatalog, LoadedDocument, LossPolicy,
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
/// `--to` names a dialect outright. A `--to` that names only a format, or no
/// `--to` at all, is not a target: it means "the same kind of file", which is
/// preservation within one format and the catalog default across formats — and
/// which of the two applies is only known once the source has been read.
#[derive(Debug, Clone)]
pub enum TargetSelection {
    /// `--to` named this dialect, as a registry id or a catalog alias.
    Explicit(String),
    /// `--to` named no dialect.
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

        let request = target
            .selection
            .request(target.encoder.as_ref(), source_format(&loaded.ir));
        // Resolution is the encoder's, and so is its refusal: the message
        // already names the requested id and the whole catalog, and it
        // reflects this build's feature set. Restating it here would be a
        // second vocabulary to keep in step with the first.
        let plan = match target
            .encoder
            .plan(EncodeInput::new(&loaded.ir, loaded.fidelity()), request)
        {
            Ok(plan) => plan,
            Err(error) => {
                return Err(plan_refusal(
                    error,
                    policy.reject_export_losses,
                    decode_report,
                    validation,
                ))
            }
        };
        if policy.reject_export_losses && !plan.report().losses.is_empty() {
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

/// Restates a plan-time codec error as a typed refusal where it is one.
///
/// Two of them are verdicts about the request rather than operational
/// failures, so they exit 1 and reach `--report` with a code:
///
/// * a dialect the encoder cannot write, which the encoder already describes
///   with its whole catalog; and
/// * the writer's own refusal to emit unrepresentable content, which only
///   exists because `--reject-lossy=export` constructed the writer with
///   [`LossPolicy::Reject`]. The gate is the same predicate as the
///   application's own export-loss refusal below, but it fires inside the
///   writer and so never yields a plan to count losses from; `reject_exports`
///   is what makes the attribution safe, because without the flag the CLI
///   never asks a writer to reject.
///
/// Everything else stays operational.
fn plan_refusal(
    error: cadmpeg_core::CodecError,
    reject_exports: bool,
    decode_report: Option<DecodeReport>,
    validation: Option<ValidationReport>,
) -> anyhow::Error {
    match error {
        cadmpeg_core::CodecError::UnsupportedTarget { .. } => {
            ConversionRefusal::UnsupportedTarget {
                message: error.to_string(),
            }
            .into()
        }
        cadmpeg_core::CodecError::NotImplemented(ref message) if reject_exports => {
            ConversionRefusal::ExportLossRejected {
                message: format!(
                    "the writer refused unrepresentable content: {message} (omit --reject-lossy to \
                     write the representable subset and report the loss)"
                ),
                decode_report,
                validation,
            }
            .into()
        }
        _ => error.into(),
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
pub fn export_target(format: Format, dialect: Option<&str>, losses: LossPolicy) -> ExportTarget {
    ExportTarget {
        format,
        encoder: crate::application::build_encoder(format, losses),
        selection: match dialect {
            Some(dialect) => TargetSelection::Explicit(dialect.to_owned()),
            None => TargetSelection::Unstated,
        },
    }
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

    /// `convert old.3dm -o new.3dm` with no target flag writes the archive
    /// version the file already is.
    ///
    /// The whole chain the command line owns, minus argv parsing: no flag makes
    /// [`export_target`] build a Rhino encoder and an `Unstated` selection, the
    /// selection resolves to `Inherit` because the source is Rhino too, and the
    /// encoder resolves `Inherit` against the source's dialect. The source is
    /// archive 50 and the catalog default is archive 80, so the assertion cannot
    /// pass by coincidence.
    ///
    /// Until this change `export_target` substituted archive 80 for flag
    /// absence, so the round trip handed a Rhino 5 user a file their own Rhino
    /// cannot open. `cadmpeg-codec-rhino`'s `writer/tests/targets.rs` covers the
    /// explicit flag, the cross-format default, and the refusal.
    #[cfg(feature = "rhino")]
    #[test]
    fn a_same_format_rhino_convert_keeps_the_source_archive_version() {
        use cadmpeg_core::dialect::DialectId;
        use cadmpeg_ir::codec::Codec;

        let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut archive_50 = Vec::new();
        cadmpeg_codec_rhino::RhinoEncoder::new(cadmpeg_codec_rhino::RhinoArchiveVersion::V5)
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit("rhino:archive-50"),
            )
            .expect("archive 50 is a target")
            .write_to(&mut archive_50)
            .expect("the plan writes");
        let decoded = cadmpeg_codec_rhino::RhinoCodec
            .decode(
                &mut std::io::Cursor::new(archive_50),
                &DecodeOptions::default(),
            )
            .expect("the archive decodes");

        let target = export_target(Format::Rhino, None, LossPolicy::Report);
        let request = target
            .selection
            .request(target.encoder.as_ref(), source_format(decoded.ir()));
        assert_eq!(request, TargetRequest::Inherit);

        let plan = target
            .encoder
            .plan(EncodeInput::new(decoded.ir(), None), request)
            .expect("the inherited target is writable");
        assert_eq!(
            plan.report().target.as_ref().map(DialectId::as_str),
            Some("rhino:archive-50")
        );
    }
    /// The loss policy is not a target.
    ///
    /// `--reject-step-losses` and `--step-target` once shared a wrong-format
    /// guard, and sharing it made them share the selection too: `convert
    /// a.step -o b.step --reject-step-losses` named AP214 explicitly and lost
    /// the identity default. `--reject-lossy=export` is the same predicate one
    /// layer down and must stay outside the selection: it reaches the encoder
    /// as [`LossPolicy`] at construction, and only `--to` may say what to
    /// write.
    #[cfg(feature = "step")]
    #[test]
    fn rejecting_export_losses_does_not_name_a_target() {
        let target = export_target(Format::Step, None, LossPolicy::Reject);

        assert!(
            matches!(target.selection, TargetSelection::Unstated),
            "{:?}",
            target.selection
        );
        assert_eq!(
            target
                .selection
                .request(target.encoder.as_ref(), Some("step")),
            TargetRequest::Inherit
        );

        let named = export_target(Format::Step, Some("step:ap242-e3"), LossPolicy::Reject);
        assert_eq!(
            named
                .selection
                .request(named.encoder.as_ref(), Some("step")),
            TargetRequest::Explicit("step:ap242-e3")
        );
    }

    /// `--to` carries the dialect half verbatim; the encoder resolves it.
    ///
    /// Both spellings the grammar admits reach `plan` unchanged, and both
    /// resolve, because `find_target` matches a catalog row by id or by alias.
    /// Resolving here instead would put a second copy of every catalog in the
    /// CLI, which is the drift the registries exist to kill.
    #[cfg(feature = "rhino")]
    #[test]
    fn an_alias_and_an_id_reach_the_encoder_unresolved_and_both_resolve() {
        use cadmpeg_core::dialect::DialectId;

        let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        for spelling in ["rhino:archive-60", "60"] {
            let target = export_target(Format::Rhino, Some(spelling), LossPolicy::Report);
            assert_eq!(
                target.selection.request(target.encoder.as_ref(), None),
                TargetRequest::Explicit(spelling)
            );
            let plan = target
                .encoder
                .plan(
                    EncodeInput::new(&ir, None),
                    TargetRequest::Explicit(spelling),
                )
                .expect("the catalog carries the row under both spellings");
            assert_eq!(
                plan.report().target.as_ref().map(DialectId::as_str),
                Some("rhino:archive-60")
            );
        }
    }

    /// A dialect outside the catalog is refused by the encoder, with the
    /// catalog in the message. The CLI writes no vocabulary of its own.
    #[cfg(feature = "iges")]
    #[test]
    fn an_unknown_dialect_is_refused_by_the_encoder_with_its_catalog() {
        let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let target = export_target(Format::Iges, Some("ap242e3"), LossPolicy::Report);
        let Err(error) = target.encoder.plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("ap242e3"),
        ) else {
            panic!("a STEP alias is not an IGES target");
        };
        let message = error.to_string();
        assert!(message.contains("ap242e3"), "{message}");
        assert!(message.contains("iges:5.3-fixed-ascii"), "{message}");
    }
}
