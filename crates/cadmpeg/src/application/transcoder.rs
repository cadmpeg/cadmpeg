// SPDX-License-Identifier: Apache-2.0
//! Typed prepare/write conversion workflow.

use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result as AnyResult};
use cadmpeg_core::decode::{DecodeDepth, ResourceLimits};
use cadmpeg_ir::codec::write::{
    target::TargetRequest, EncodeError, EncodeInput, Encoder, ExportPlan, MissingStateForm,
    OutputNames, Outputs,
};
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::report::{
    check::ValidationReport, decode::DecodeReport, export::ExportReport, strictness::Strictness,
    Severity,
};
use clap::ValueEnum;

use cadmpeg_registry::{ForcedInput, Format, InputCatalog};

use crate::application::artifact_store::{self, FileDestination, SidecarPersistOutcome};
use crate::application::document::{LoadOrigin, LoadedDocument};
use crate::application::refusal::{ApplicationError, ConversionRefusal, RefusalStage};
use crate::application::validators::validate_loaded;
use crate::loader;

/// Input path and decode options for one conversion.
pub(crate) struct SourceRequest<'a> {
    /// Path to the CADIR or native CAD file.
    pub(crate) path: &'a Path,
    /// Explicit input format selection.
    pub(crate) forced: Option<ForcedInput>,
    /// Decode options.
    pub(crate) options: DecodeOptions,
    /// The work profile of the check's measurement findings.
    pub(crate) work: cadmpeg_measure::WorkProfile,
}

/// Output format with an encoder already constructed at the CLI boundary.
pub(crate) struct ExportTarget {
    encoder: Box<dyn Encoder>,
    selection: TargetSelection,
}

impl ExportTarget {
    /// The decode depth the selected encoder plans from.
    #[must_use]
    pub(crate) fn request(&self) -> DecodeDepth {
        self.encoder.request()
    }
}

/// The output format and target token selected by the command line.
///
/// `--to` names a dialect outright. A `--to` that names only a format, or no
/// `--to` at all, takes the dialect that the output extension names, such as
/// `usdz` for a `.usdz` path; otherwise it leaves `request` unstated so the
/// encoder inherits from the source. The encoder resolves explicit tokens, source-dependent
/// preservation, and delivery during planning.
#[derive(Debug, Clone)]
pub(crate) struct TargetSelection {
    /// Selected output format.
    format: Format,
    /// Dialect token from `--to` or from the output extension, as a local id
    /// or catalog alias.
    request: Option<String>,
}

impl TargetSelection {
    /// Creates an owned output selection at the command-line boundary.
    #[must_use]
    fn new(format: Format, request: Option<String>) -> Self {
        Self { format, request }
    }

    /// Resolves the format half of the `--to` grammar against the output path.
    /// The encoder admits the dialect half during planning.
    pub(crate) fn resolve(to: Option<&str>, out: Option<&Path>) -> Result<Self, ApplicationError> {
        let inferred = format_from_path(out);
        let extension_dialect = inferred
            .zip(extension(out))
            .and_then(|(format, extension)| format.dialect_for_extension(extension))
            .map(str::to_owned);
        Ok(match to {
            None => Self::new(
                inferred.ok_or_else(|| {
                    anyhow!("cannot infer format from the output path; pass --to FORMAT")
                })?,
                extension_dialect,
            ),
            Some(value) => Self::resolve_value(value, inferred, extension_dialect)?,
        })
    }

    fn resolve_value(
        value: &str,
        inferred: Option<Format>,
        extension_dialect: Option<String>,
    ) -> Result<Self, ApplicationError> {
        if let Some((left, right)) = value.split_once(':') {
            let format = Format::from_name(left)?.ok_or_else(|| {
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
        if let Some(format) = Format::from_name(value)? {
            warn_on_extension_disagreement(format, inferred);
            let dialect = extension_dialect.filter(|_| inferred == Some(format));
            return Ok(Self::new(format, dialect));
        }
        if Format::is_known_name(value)? {
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

fn extension(path: Option<&Path>) -> Option<&str> {
    path.and_then(Path::extension)
        .and_then(|extension| extension.to_str())
}

fn format_from_path(path: Option<&Path>) -> Option<Format> {
    extension(path).and_then(Format::from_extension)
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

/// The reports `--reject-lossy` judges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum LossyScope {
    /// This invocation's decode, or the CADIR parse.
    Decode,
    /// The write.
    Export,
    /// The decode and the write.
    Any,
}

/// The severity floor `--reject-lossy` sets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum LossyFloor {
    /// Every entry.
    #[default]
    Info,
    /// Warnings and above.
    Warning,
    /// Errors and above.
    Error,
    /// Blocking entries.
    Blocking,
}

impl From<LossyFloor> for Severity {
    fn from(floor: LossyFloor) -> Self {
        match floor {
            LossyFloor::Info => Self::Info,
            LossyFloor::Warning => Self::Warning,
            LossyFloor::Error => Self::Error,
            LossyFloor::Blocking => Self::Blocking,
        }
    }
}

/// The strictness flags of `convert`. Each sets a field of the decode's or
/// the write's [`Strictness`] (CADIR v11 §3.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct StrictnessFlags {
    /// `--reject-errors`: the write refuses `export:record.invalid`, and a
    /// check that cannot run to its end refuses the conversion.
    pub(crate) reject_errors: bool,
    /// `--reject-empty`: the write refuses `export:output.empty`.
    pub(crate) reject_empty: bool,
    /// `--reject-lossy`: the reports whose floor `lossy_floor` sets.
    pub(crate) reject_lossy: Option<LossyScope>,
    /// `--reject-lossy-floor`.
    pub(crate) lossy_floor: LossyFloor,
    /// `--no-salvage`: the decode refuses an entry that reaches its strict
    /// floor.
    pub(crate) no_salvage: bool,
}

impl StrictnessFlags {
    /// The decode's strictness.
    #[must_use]
    pub(crate) fn decode(self) -> Strictness {
        Strictness {
            pinned: self.no_salvage,
            floor: matches!(
                self.reject_lossy,
                Some(LossyScope::Decode | LossyScope::Any)
            )
            .then(|| self.lossy_floor.into()),
            codes: std::collections::BTreeSet::new(),
        }
    }

    /// The write's strictness.
    #[must_use]
    pub(crate) fn write(self) -> Strictness {
        let mut strictness = Strictness {
            pinned: false,
            floor: matches!(
                self.reject_lossy,
                Some(LossyScope::Export | LossyScope::Any)
            )
            .then(|| self.lossy_floor.into()),
            codes: std::collections::BTreeSet::new(),
        };
        if self.reject_errors {
            strictness = strictness.with_code("export:record.invalid");
        }
        if self.reject_empty {
            strictness = strictness.with_code("export:output.empty");
        }
        strictness
    }

    /// Whether the conversion decodes the whole source. A `--reject-lossy`
    /// scope judges the source's design intent too, and a decode cut to the
    /// depth the output needs reports no loss for the design records it
    /// skips.
    #[must_use]
    pub(crate) const fn needs_full_decode(self) -> bool {
        self.reject_lossy.is_some()
    }
}

/// Destination and overwrite rules for a conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DestinationPolicy {
    /// Write to standard output.
    Stdout {
        /// Permit a binary format on standard output.
        allow_binary: bool,
    },
    /// Write to a file.
    File(FileDestination),
}

/// Conversion output and report destinations admitted from command arguments.
#[derive(Debug)]
pub(crate) struct ConversionDestinations {
    /// Primary output destination.
    pub(crate) destination: DestinationPolicy,
    /// Optional command report output.
    pub(crate) report: Option<FileDestination>,
}

impl clap::Args for ConversionDestinations {
    fn augment_args(command: clap::Command) -> clap::Command {
        artifact_store::OutputDestinations::augment_args(command).arg(
            clap::Arg::new("binary_stdout")
                .long("binary-stdout")
                .hide(true)
                .help("Stream a binary output format to standard output anyway")
                .action(clap::ArgAction::SetTrue),
        )
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        Self::augment_args(command)
    }
}

impl clap::FromArgMatches for ConversionDestinations {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        let outputs = artifact_store::OutputDestinations::from_arg_matches(matches)?;
        Ok(Self {
            destination: match outputs.output.0 {
                Some(file) => DestinationPolicy::File(file),
                None => DestinationPolicy::Stdout {
                    allow_binary: matches.get_flag("binary_stdout"),
                },
            },
            report: outputs.report,
        })
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl DestinationPolicy {
    /// Returns the output path used for format inference, if any.
    #[must_use]
    pub(crate) fn path(&self) -> Option<&Path> {
        match self {
            Self::Stdout { .. } => None,
            Self::File(destination) => Some(&destination.path),
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
            Self::Stdout { .. } | Self::File(..) => Ok(()),
        }
    }

    /// Resolves filesystem-dependent destination rules. A missing source is
    /// left for the loader; an existing destination still refuses before the
    /// potentially expensive decode.
    pub(crate) fn resolve(&self, source: &Path) -> AnyResult<ResolvedDestination> {
        match self {
            Self::Stdout { .. } => Ok(ResolvedDestination::Stdout),
            Self::File(destination) => {
                destination.check(source)?;
                Ok(ResolvedDestination::File(destination.path.clone()))
            }
        }
    }
}

/// Policy controlling the independent conversion phases.
#[derive(Debug, Clone)]
pub(crate) struct ConversionPolicy {
    /// The decode's and the write's strictness.
    pub(crate) strictness: StrictnessFlags,
    /// The form of features whose states the target needs and the document
    /// lacks.
    pub(crate) missing_states: MissingStateForm,
    /// Output destination rules.
    pub(crate) destination: DestinationPolicy,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedDestination {
    Stdout,
    File(PathBuf),
}

impl ResolvedDestination {
    pub(crate) fn path(&self) -> Option<&Path> {
        match self {
            Self::Stdout => None,
            Self::File(path) => Some(path),
        }
    }
}

/// A loaded and validated conversion ready for export planning.
/// The decode's strictness is applied. The write's applies after planning.
pub(crate) struct PreparedConversion {
    /// Loaded source document.
    pub(crate) document: LoadedDocument,
    /// Validation report, or the resource limit that stopped the check
    /// before its end.
    pub(crate) validation: Validation,
    encoder: Box<dyn Encoder>,
    selection: TargetSelection,
    /// The files that external assets of the document name.
    assets: loader::companions::Directory,
    destination: ResolvedDestination,
    write_strictness: Strictness,
    missing_states: MissingStateForm,
    /// The session ceilings, which the write takes as the decode does.
    limits: ResourceLimits,
}

/// The outcome of the conversion's check.
#[derive(Debug, Clone)]
pub(crate) enum Validation {
    /// The check ran to its end.
    Complete(ValidationReport),
    /// A ceiling of the caller stopped the check; the message names it.
    Incomplete(String),
}

impl Validation {
    /// The validation report of a complete check.
    #[must_use]
    pub(crate) const fn report(&self) -> Option<&ValidationReport> {
        match self {
            Self::Complete(report) => Some(report),
            Self::Incomplete(_) => None,
        }
    }
}

/// Loads and validates a conversion without planning or writing it.
///
/// Applies destination admission. The load applies the decode's strictness. Runs
/// validation. Plan refusals and the write's strictness apply in `plan`.
/// Prints only the load's notices (see `loader::load_artifact`); does not
/// print reports or write destination artifacts.
pub(crate) fn prepare(
    inputs: &InputCatalog,
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

    let loaded = loader::load_artifact(inputs, source.path, &source.options, source.forced)?;

    let validation = match validate_loaded(inputs, &loaded, &source.options.policy, source.work) {
        Ok(report) => Validation::Complete(report),
        Err(error) if !policy.strictness.reject_errors && is_resource_limit(&error) => {
            Validation::Incomplete(error.to_string())
        }
        Err(error) => return Err(error),
    };
    Ok(PreparedConversion {
        document: loaded,
        validation,
        encoder: target.encoder,
        selection: target.selection,
        assets: loader::companions::Directory::beside(source.path),
        destination,
        write_strictness: policy.strictness.write(),
        missing_states: policy.missing_states,
        limits: source.options.policy.limits,
    })
}

fn is_resource_limit(error: &ApplicationError) -> bool {
    matches!(
        error.refusal(),
        Some(ConversionRefusal::ResourceLimit { .. })
    )
}

impl PreparedConversion {
    /// Plans the export with the check's report and the write's strictness:
    /// the encoder charges `export:record.invalid` for each check Error in
    /// what the output writes and refuses where the strictness refuses.
    pub(crate) fn plan(self) -> Result<PlannedConversion, ApplicationError> {
        let names = self.output_names()?;
        // Resolution is the encoder's, and so is its refusal: the message
        // already names the requested id and the whole catalog, and it
        // reflects this build's feature set. Restating it here would be a
        // second vocabulary to keep in step with the first.
        let mut input = EncodeInput::new(&self.document.ir, self.document.fidelity())
            .with_assets(&self.assets)
            .with_target(self.selection.request())
            .with_limits(self.limits)
            .with_missing_states(self.missing_states)
            .with_strictness(&self.write_strictness);
        if let Some(report) = self.validation.report() {
            input = input.with_validation(report);
        }
        if let Some(names) = &names {
            input = input.with_outputs(Outputs::Named(names));
        }
        let plan = match self.encoder.plan(input) {
            Ok(plan) => plan,
            Err(error) => {
                return Err(plan_refusal(
                    error,
                    self.document.decode_report().cloned(),
                    self.validation.report().cloned(),
                ));
            }
        };
        let carried = self.validation.report().map_or(0, |report| {
            plan.carried_errors(&self.document.ir, report).len()
        });
        Ok(PlannedConversion {
            plan,
            prepared: self,
            carried_errors: carried,
        })
    }

    /// The names of an encode that writes more than one file: the
    /// destination's file name for the root definition's output, and
    /// `<stem>-<n>.<extension>` beside it for the n-th other output. `None`
    /// where the encode writes one file, which the destination names.
    fn output_names(&self) -> Result<Option<OutputNames>, ApplicationError> {
        let definitions = self.encoder.output_definitions(&self.document.ir);
        if definitions.len() < 2 {
            return Ok(None);
        }
        let ResolvedDestination::File(path) = &self.destination else {
            return Err(ApplicationError::from(anyhow!(
                "the {} encode writes {} files, and standard output takes one; name an output file",
                self.selection.format.name(),
                definitions.len()
            ))
            .at_stage(RefusalStage::Plan));
        };
        let text =
            |part: Option<&std::ffi::OsStr>| part.map(|part| part.to_string_lossy().into_owned());
        let file_name = text(path.file_name()).unwrap_or_default();
        let stem = text(path.file_stem()).unwrap_or_default();
        let extension = text(path.extension());
        let mut names = OutputNames::new();
        for (position, definition) in definitions.into_iter().enumerate() {
            let Some(definition) = definition else {
                continue;
            };
            let name = match (position, &extension) {
                (0, _) => file_name.clone(),
                (_, Some(extension)) => format!("{stem}-{position}.{extension}"),
                (_, None) => format!("{stem}-{position}"),
            };
            if let Some(name) = cadmpeg_core::text::NonBlankString::new(name) {
                names.insert(definition, name);
            }
        }
        Ok(Some(names))
    }

    /// Returns the selected output format.
    pub(crate) const fn format(&self) -> Format {
        self.selection.format
    }
}

/// One planned export and the source state from which it was produced.
pub(crate) struct PlannedConversion {
    plan: ExportPlan,
    prepared: PreparedConversion,
    carried_errors: usize,
}

impl PlannedConversion {
    /// Returns the number of error findings carried by the output.
    pub(crate) const fn carried_errors(&self) -> usize {
        self.carried_errors
    }

    /// Returns the loaded source state retained for reporting.
    #[must_use]
    pub(crate) const fn prepared(&self) -> &PreparedConversion {
        &self.prepared
    }

    /// The path each output of the plan is written to; none for standard
    /// output.
    pub(crate) fn output_paths(&self) -> Vec<PathBuf> {
        match &self.prepared.destination {
            ResolvedDestination::File(path) => {
                artifact_store::output_paths(path, self.plan.outputs())
            }
            ResolvedDestination::Stdout => Vec::new(),
        }
    }

    /// Writes the destination artifact and optional CADIR sidecar.
    pub(crate) fn write(self) -> AnyResult<ExportEmission> {
        emit_export_plan(
            self.plan,
            self.prepared.selection.format,
            &self.prepared.destination,
            &self.prepared.document.origin,
            &self.prepared.document.ir,
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
        /// The other files of an encode that writes one per definition.
        others: Vec<PathBuf>,
    },
    /// Standard output with no sidecar requirement.
    Stdout,
    /// CADIR on standard output, where the required sidecar cannot travel.
    StdoutWithoutSidecar,
}

/// Restates an unsupported target and a strictness refusal as typed
/// refusals.
///
/// Resource failures retain their typed evidence; other plan failures stay
/// operational.
fn plan_refusal(
    error: EncodeError,
    decode_report: Option<DecodeReport>,
    validation: Option<ValidationReport>,
) -> ApplicationError {
    match error {
        EncodeError::UnsupportedTarget(refusal) => ConversionRefusal::UnsupportedTarget {
            refusal,
            decode_report,
            validation,
        }
        .into(),
        EncodeError::StrictRejected(rejection) => ConversionRefusal::StrictExportRejected {
            decode_report,
            validation,
            rejection: Box::new(rejection),
        }
        .into(),
        error => ApplicationError::from(error).at_stage(RefusalStage::Plan),
    }
}

/// Emits one built plan to its destination and maintains CADIR sidecars.
///
/// This operation performs artifact I/O only. The returned outcome carries
/// every fact the command layer needs for presentation.
pub(crate) fn emit_export_plan(
    plan: ExportPlan,
    format: Format,
    destination: &ResolvedDestination,
    origin: &LoadOrigin,
    ir: &cadmpeg_ir::CadIr,
) -> AnyResult<ExportEmission> {
    let needs_sidecar = format == Format::Cadir
        && matches!(
            origin,
            LoadOrigin::Decoded { .. } | LoadOrigin::Restored { .. } | LoadOrigin::Read { .. }
        );
    if let ResolvedDestination::File(path) = destination {
        let (report, cadir_sha256, others) = artifact_store::write_plan_atomic(path, plan)?;
        let sidecar = if format == Format::Cadir {
            artifact_store::persist_decode_sidecar(path, &cadir_sha256, origin, ir)?
        } else {
            SidecarPersistOutcome::Absent
        };
        Ok(ExportEmission {
            report,
            artifact: EmittedArtifact::File {
                path: path.to_owned(),
                sidecar,
                others,
            },
        })
    } else {
        let stdout = io::stdout().lock();
        let mut writer = BufWriter::with_capacity(64 * 1024, stdout);
        let report = plan.write_to(&mut writer)?;
        writer.flush()?;
        Ok(ExportEmission {
            report,
            artifact: if needs_sidecar {
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
/// The strictness flags are a policy, never a target: reading one as a target
/// would turn `convert a.step -o b.step --reject-lossy=export` into an
/// explicit AP214 request, silently rewriting the schema of a file the caller
/// only asked to check for losses.
pub(crate) fn export_target(selection: TargetSelection) -> ExportTarget {
    let encoder = cadmpeg_registry::build_encoder(selection.format);
    ExportTarget { encoder, selection }
}

#[cfg(test)]
mod tests;
