// SPDX-License-Identifier: Apache-2.0
//! Command execution, artifact writing, and human-readable reports.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::{validate_neutral, validate_neutral_with_source_fidelity, CadIr, SourceFidelity};

pub use crate::application::ValidationMode;
use crate::application::{
    build_encoder, export_target, ArtifactStore, ConversionPolicy, ConversionRefusal,
    EncoderRequest, ForcedInput, InputCatalog, NativeValidatorCatalog, ResolveSourceError,
    ResolvedSource, SidecarPersistOutcome, SourceRequest, Transcoder,
};
use crate::loader::{self, read_prefix, LoadNotice, DETECTION_PREFIX_LEN};
use crate::{DecodeArgs, Format};

/// CLI command-report envelope version.
///
/// Independent of `CadIr.ir_version` and `DECODE_SIDECAR_VERSION`. Version 6
/// adds top-level `status` (`ok` | `refused`) and `refusal` (`{ stage, code,
/// message }` or null).
pub(crate) const CLI_SCHEMA_VERSION: u32 = 6;

/// Catalogs required by CLI command handlers.
pub struct AppCatalogs {
    /// Input detection and codec lookup.
    pub inputs: InputCatalog,
    /// Native namespace validators.
    pub validators: NativeValidatorCatalog,
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

fn print_load_notices(notices: &[LoadNotice]) {
    for notice in notices {
        match notice {
            LoadNotice::LowConfidenceDetection {
                format_id,
                confidence,
            } => {
                eprintln!(
                    "warning: detected {format_id} with {confidence} confidence; use --input-format to override"
                );
            }
        }
    }
}

/// CLI-facing conversion arguments assembled before [`Transcoder::prepare`].
#[allow(clippy::struct_excessive_bools)]
pub struct ConversionPlan {
    /// Replace an existing output or report file.
    pub force: bool,
    /// Optional path for the versioned JSON command report.
    pub report: Option<PathBuf>,
    /// Stream a binary output format to standard output instead of refusing.
    pub binary_stdout: bool,
    /// Neutral validation policy.
    pub validation: ValidationMode,
    /// Export a geometry format when decoding transferred no geometry.
    pub allow_empty: bool,
    /// Refuse to export when the decode reported any loss.
    pub reject_lossy: bool,
    /// Explicit Rhino output archive version when the flag was supplied.
    #[cfg(feature = "rhino")]
    pub rhino_version: Option<cadmpeg_codec_rhino::RhinoArchiveVersion>,
    /// STEP writer options when a STEP-only flag was supplied.
    #[cfg(feature = "step")]
    pub step_options: Option<cadmpeg_codec_step::StepWriteOptions>,
    /// True when `--step-target` or `--reject-step-losses` was present.
    #[cfg(feature = "step")]
    pub step_flag_present: bool,
    /// IGES writer options when `--iges-target` was supplied.
    #[cfg(feature = "iges")]
    pub iges_options: Option<cadmpeg_codec_iges::IgesWriteOptions>,
    /// Explicit input format selected by the user.
    pub forced_input: Option<ForcedInput>,
}

/// One input to a structural diff and its optional format override.
#[derive(Clone, Copy)]
pub(crate) struct DiffInput<'a> {
    /// Model path to load.
    pub(crate) path: &'a Path,
    /// Explicit reader selection for this input.
    pub(crate) forced: Option<ForcedInput>,
}

/// Inspect a native container and print its entries.
pub fn inspect(
    catalogs: &AppCatalogs,
    path: &Path,
    forced: Option<ForcedInput>,
    json: bool,
    report_path: Option<&Path>,
    force: bool,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<()> {
    if matches!(forced, Some(ForcedInput::Cadir)) {
        bail!("inspect requires a container input, not cadir");
    }
    let prefix = read_prefix(path, DETECTION_PREFIX_LEN)?;
    let (codec, confidence) = match catalogs.inputs.resolve_source(&prefix, forced) {
        Ok(ResolvedSource::Native {
            codec, confidence, ..
        }) => (codec, confidence),
        Ok(ResolvedSource::Cadir) => {
            return Err(anyhow!(
                "no codec recognized {}; inspect supports container inputs only, not .cadir.json IR documents; supported: FCStd, f3d, Inventor IPT/IAM, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP; use --input-format to override detection",
                path.display()
            ));
        }
        Err(ResolveSourceError::UnsupportedFormat(id)) => {
            return Err(anyhow!("unsupported input format {id}"));
        }
        Err(error) => return Err(anyhow!(error.to_string())),
    };
    let mut file = File::open(path)?;
    let summary = codec
        .inspect(&mut file, &InspectOptions { limits })
        .with_context(|| format!("inspecting {}", path.display()))?;
    write_json_report(
        path,
        report_path,
        force,
        "inspect",
        &serde_json::json!({
            "confidence": confidence,
            "summary": summary,
        }),
        None,
    )?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": CLI_SCHEMA_VERSION,
                "command": "inspect",
                "status": "ok",
                "refusal": null,
                "confidence": confidence,
                "summary": summary,
            }))?
        );
        return Ok(());
    }
    println!(
        "format: {}{}\ncontainer: {}\nentries: {}",
        summary.format,
        confidence.map_or_else(
            || " (forced)".to_string(),
            |value| format!(" (detected {value})")
        ),
        summary.container_kind,
        summary.entries.len()
    );
    println!();
    for entry in &summary.entries {
        println!(
            "  {:<14} {:>10} → {:<10}  {}",
            entry.role, entry.compressed_size, entry.uncompressed_size, entry.name
        );
        for (key, value) in &entry.attributes {
            println!("        {key} = {value}");
        }
    }
    if !summary.notes.is_empty() {
        println!("\nnotes:");
        for note in &summary.notes {
            println!("  - {note}");
        }
    }
    Ok(())
}

/// Decode a native CAD file and write canonical CADIR JSON.
pub fn decode(
    catalogs: &AppCatalogs,
    path: &Path,
    out: Option<&Path>,
    force: bool,
    report_path: Option<&Path>,
    forced: Option<ForcedInput>,
    args: &DecodeArgs,
) -> Result<()> {
    let outcome = match loader::load_artifact(&catalogs.inputs, path, args.options(), forced) {
        Ok(outcome) => outcome,
        Err(error) => {
            let Some(report_path) = report_path else {
                return Err(error);
            };
            let Some(refusal) = decode_failure_refusal(&error) else {
                return Err(error);
            };
            write_command_report(
                path,
                Some(report_path),
                force,
                "decode",
                CommandReportBody {
                    decode_report: None,
                    validation_report: None,
                    export: None,
                    refusal: Some(&refusal),
                },
            )?;
            return Err(refusal.into());
        }
    };
    print_load_notices(&outcome.notices);
    let loaded = &outcome.document;
    export_ir(
        &loaded.ir,
        loaded.decode_report(),
        loaded.fidelity(),
        Format::Cadir,
        out,
        path,
        force,
        EncoderRequest::Neutral,
    )?;
    if let Some(report) = loaded.decode_report() {
        print_decode_report(&mut io::stderr(), report)?;
    }
    // Decode does not validate. Convert/validate compose validate_neutral +
    // fidelity + native; salvage mode may emit IR with findings.
    eprintln!("validation: not run (successful decode is not a valid IR; run `cadmpeg validate`)");
    write_command_report(
        path,
        report_path,
        force,
        "decode",
        CommandReportBody {
            decode_report: loaded.decode_report(),
            validation_report: None,
            export: None,
            refusal: None,
        },
    )?;
    Ok(())
}

fn decode_failure_refusal(error: &anyhow::Error) -> Option<ConversionRefusal> {
    let codec_error = error.downcast_ref::<CodecError>()?;
    if matches!(codec_error, CodecError::Io(_)) {
        return None;
    }
    Some(ConversionRefusal::DecodeFailed {
        message: format!("decode failed: {error:#}"),
    })
}

/// Load and validate CADIR, printing a human-readable or JSON report.
pub fn validate_cmd(
    catalogs: &AppCatalogs,
    path: &Path,
    forced: Option<ForcedInput>,
    args: &DecodeArgs,
    json: bool,
    report_path: Option<&Path>,
    force: bool,
) -> Result<()> {
    let outcome = loader::load_artifact(&catalogs.inputs, path, args.options(), forced)?;
    print_load_notices(&outcome.notices);
    let loaded = &outcome.document;
    let mut stdout = io::stdout();
    if let Some(report) = loaded.decode_report() {
        print_decode_report(&mut io::stderr(), report)?;
    }
    let report = validate_ir(
        &catalogs.validators,
        &loaded.ir,
        loaded.fidelity(),
        losses(loaded.decode_report()),
    );
    let validate_refusal = (!report.is_ok()).then(|| ConversionRefusal::ValidationFailed {
        message: format!("validation found {} error(s)", report.error_count()),
        decode_report: loaded.decode_report().cloned(),
        validation: report.clone(),
    });
    write_json_report(
        path,
        report_path,
        force,
        "validate",
        &serde_json::json!({
            "decode_report": loaded.decode_report(),
            "validation_report": report,
        }),
        validate_refusal.as_ref(),
    )?;
    if json {
        let mut payload = serde_json::json!({
            "schema_version": CLI_SCHEMA_VERSION,
            "command": "validate",
            "decode_report": loaded.decode_report(),
            "validation_report": report,
        });
        match &validate_refusal {
            Some(refusal) => {
                let fields = refusal.report_fields();
                payload["status"] = fields["status"].clone();
                payload["refusal"] = fields["refusal"].clone();
            }
            None => {
                payload["status"] = serde_json::json!("ok");
                payload["refusal"] = serde_json::Value::Null;
            }
        }
        writeln!(stdout, "{}", serde_json::to_string_pretty(&payload)?)?;
    } else {
        print_validation_report(&mut stdout, &report)?;
    }
    if let Some(refusal) = validate_refusal {
        return Err(refusal.into());
    }
    Ok(())
}

/// Decode if needed and export without validating CADIR.
pub fn export(
    catalogs: &AppCatalogs,
    path: &Path,
    format: Option<Format>,
    out: Option<&Path>,
    plan: &ConversionPlan,
    args: &DecodeArgs,
) -> Result<()> {
    execute_conversion(catalogs, path, format, out, plan, args, "export")
}

/// Decode if needed, validate CADIR, and export.
pub fn convert(
    catalogs: &AppCatalogs,
    path: &Path,
    format: Option<Format>,
    out: Option<&Path>,
    plan: &ConversionPlan,
    args: &DecodeArgs,
) -> Result<()> {
    execute_conversion(catalogs, path, format, out, plan, args, "convert")
}

fn execute_conversion(
    catalogs: &AppCatalogs,
    path: &Path,
    format: Option<Format>,
    out: Option<&Path>,
    plan: &ConversionPlan,
    args: &DecodeArgs,
    command: &'static str,
) -> Result<()> {
    let format = resolve_format(format, out)?;
    let target = export_target(
        format,
        #[cfg(feature = "step")]
        plan.step_options.clone(),
        #[cfg(feature = "step")]
        plan.step_flag_present,
        #[cfg(feature = "iges")]
        plan.iges_options,
        #[cfg(feature = "rhino")]
        plan.rhino_version,
    )
    .map_err(anyhow::Error::from)?;

    let transcoder = Transcoder::new(&catalogs.inputs, &catalogs.validators);
    let source = SourceRequest {
        path,
        forced: plan.forced_input,
        options: args.options(),
    };
    let prepared = match transcoder.prepare(
        &source,
        target,
        ConversionPolicy {
            force: plan.force,
            binary_stdout: plan.binary_stdout,
            validation: plan.validation,
            allow_empty: plan.allow_empty,
            reject_lossy: plan.reject_lossy,
            destination: out.map(Path::to_path_buf),
        },
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            if let Some(refusal) = error.downcast_ref::<ConversionRefusal>() {
                let mut stderr = io::stderr();
                if let Some(report) = refusal.decode_report() {
                    print_decode_report(&mut stderr, report)?;
                    if matches!(plan.validation, ValidationMode::Skipped) {
                        eprintln!("note: export skips IR validation; use `convert` to validate");
                    } else {
                        writeln!(stderr)?;
                    }
                }
                if let Some(validation) = refusal.validation_report() {
                    print_validation_report(&mut stderr, validation)?;
                }
                if refusal.may_write_report() {
                    write_command_report(
                        path,
                        plan.report.as_deref(),
                        plan.force,
                        command,
                        CommandReportBody {
                            decode_report: refusal.decode_report(),
                            validation_report: refusal.validation_report(),
                            export: None,
                            refusal: Some(refusal),
                        },
                    )?;
                }
            }
            return Err(error);
        }
    };

    print_load_notices(&prepared.notices);
    let mut stderr = io::stderr();
    if let Some(report) = prepared.document.decode_report() {
        print_decode_report(&mut stderr, report)?;
        if matches!(plan.validation, ValidationMode::Skipped) {
            eprintln!("note: export skips IR validation; use `convert` to validate");
        } else {
            writeln!(stderr)?;
        }
    }
    if let Some(validation) = &prepared.validation {
        print_validation_report(&mut stderr, validation)?;
    }
    let decode_report = prepared.document.decode_report().cloned();
    let validation = prepared.validation.clone();
    let report = prepared.write()?;
    write_command_report(
        path,
        plan.report.as_deref(),
        plan.force,
        command,
        CommandReportBody {
            decode_report: decode_report.as_ref(),
            validation_report: validation.as_ref(),
            export: Some(&report),
            refusal: None,
        },
    )
}

/// Structurally compare two decoded models.
pub fn diff(
    catalogs: &AppCatalogs,
    a: DiffInput<'_>,
    b: DiffInput<'_>,
    args: &DecodeArgs,
    json: bool,
    report_path: Option<&Path>,
    force: bool,
) -> Result<ExitCode> {
    let left_outcome = loader::load_artifact(&catalogs.inputs, a.path, args.options(), a.forced)?;
    print_load_notices(&left_outcome.notices);
    let right_outcome = loader::load_artifact(&catalogs.inputs, b.path, args.options(), b.forced)?;
    print_load_notices(&right_outcome.notices);
    let left = &left_outcome.document;
    let right = &right_outcome.document;
    let result = cadmpeg_ir::diff(&left.ir, &right.ir);
    let fidelity = fidelity_diff(left.fidelity(), right.fidelity());
    let different = !result.is_empty() || fidelity_differs(&fidelity);
    write_json_report(
        a.path,
        report_path,
        force,
        "diff",
        &serde_json::json!({
            "different": different,
            "diff": result,
            "source_fidelity": fidelity_json(&fidelity),
        }),
        None,
    )?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": CLI_SCHEMA_VERSION,
                "command": "diff",
                "status": "ok",
                "refusal": null,
                "different": different,
                "diff": result,
                "source_fidelity": fidelity_json(&fidelity),
            }))?
        );
        return Ok(if different {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        });
    }
    println!("diff {} vs {}", a.path.display(), b.path.display());
    if let Some((before, after)) = &result.unit_change {
        println!("  units: {before:?} → {after:?}");
    }
    if let Some((before, after)) = &result.tolerance_change {
        println!("  tolerances: {before:?} → {after:?}");
    }
    print_source_diff(&result.source);
    for arena in &result.per_arena {
        if arena.added.is_empty() && arena.removed.is_empty() && arena.modified.is_empty() {
            continue;
        }
        println!(
            "  {}: +{} -{} ~{}",
            arena.kind,
            arena.added.len(),
            arena.removed.len(),
            arena.modified.len()
        );
        print_id_delta("removed", &arena.removed);
        print_id_delta("added", &arena.added);
        let modified: Vec<String> = arena
            .modified
            .iter()
            .map(|item| format!("{} ({})", item.id, item.fields.join(", ")))
            .collect();
        print_id_delta("modified", &modified);
    }
    print_fidelity_summary(&fidelity);
    if different {
        Ok(ExitCode::from(1))
    } else {
        println!("  identical");
        Ok(ExitCode::SUCCESS)
    }
}

/// Print source-metadata changes, with machine-local digests in their own
/// section.
///
/// The digest section is informational and does not reach the exit code: a
/// machine-local digest is a bitwise fingerprint of tolerantly compared geometry,
/// so the same file decoded on two platforms disagrees on it while describing one
/// model. `cadmpeg_ir::diff` states the convention that identifies them.
fn print_source_diff(source: &cadmpeg_ir::SourceDiff) {
    if let Some((before, after)) = &source.format_change {
        println!("  source format: {before} → {after}");
    }
    for change in &source.attributes {
        println!(
            "  source {}: {} → {}",
            change.key,
            render_attribute(change.left.as_deref()),
            render_attribute(change.right.as_deref())
        );
    }
    if source.local_digests.is_empty() {
        return;
    }
    println!("  machine-local digests (informational, not a difference):");
    for change in &source.local_digests {
        println!(
            "    {}: {} → {}",
            change.key,
            render_attribute(change.left.as_deref()),
            render_attribute(change.right.as_deref())
        );
    }
}

/// Render one side of an attribute change, naming an absent key rather than
/// printing an empty string that reads as an empty value.
fn render_attribute(value: Option<&str>) -> String {
    value.map_or_else(|| "<absent>".to_owned(), ToOwned::to_owned)
}

enum FidelitySummary {
    /// Neither decode reported a sidecar, for example when both inputs are CADIR JSON.
    None,
    /// Only the left input reported a sidecar.
    OnlyLeft,
    /// Only the right input reported a sidecar.
    OnlyRight,
    /// Both inputs reported a sidecar; the interpreted delta between them.
    Both(FidelityDiff),
}

struct FidelityDiff {
    version: Option<(String, String)>,
    annotations_changed: bool,
    retained_records_changed: bool,
}

impl FidelityDiff {
    fn between(left: &SourceFidelity, right: &SourceFidelity) -> Self {
        Self {
            version: (left.version() != right.version())
                .then(|| (left.version().to_owned(), right.version().to_owned())),
            annotations_changed: left.annotations != right.annotations,
            retained_records_changed: left.retained_records != right.retained_records,
        }
    }

    fn is_empty(&self) -> bool {
        self.version.is_none() && !self.annotations_changed && !self.retained_records_changed
    }
}

fn fidelity_diff(left: Option<&SourceFidelity>, right: Option<&SourceFidelity>) -> FidelitySummary {
    match (left, right) {
        (Some(left), Some(right)) => FidelitySummary::Both(FidelityDiff::between(left, right)),
        (Some(_), None) => FidelitySummary::OnlyLeft,
        (None, Some(_)) => FidelitySummary::OnlyRight,
        (None, None) => FidelitySummary::None,
    }
}

fn fidelity_differs(summary: &FidelitySummary) -> bool {
    match summary {
        FidelitySummary::None => false,
        FidelitySummary::OnlyLeft | FidelitySummary::OnlyRight => true,
        FidelitySummary::Both(diff) => !diff.is_empty(),
    }
}

fn fidelity_json(summary: &FidelitySummary) -> serde_json::Value {
    match summary {
        FidelitySummary::None => serde_json::Value::Null,
        FidelitySummary::OnlyLeft => serde_json::json!({ "present": "left_only" }),
        FidelitySummary::OnlyRight => serde_json::json!({ "present": "right_only" }),
        FidelitySummary::Both(diff) => serde_json::json!({
            "present": "both",
            "different": !diff.is_empty(),
            "diff": fidelity_delta_json(diff),
        }),
    }
}

fn fidelity_delta_json(diff: &FidelityDiff) -> serde_json::Value {
    let mut value = serde_json::json!({
        "annotations_changed": diff.annotations_changed,
        "retained_records_changed": diff.retained_records_changed,
    });
    if let Some(version) = &diff.version {
        value["version"] = serde_json::json!(version);
    }
    value
}

fn print_fidelity_summary(summary: &FidelitySummary) {
    let diff = match summary {
        FidelitySummary::None => return,
        FidelitySummary::OnlyLeft => {
            println!("  source fidelity: present on left only (not comparable)");
            return;
        }
        FidelitySummary::OnlyRight => {
            println!("  source fidelity: present on right only (not comparable)");
            return;
        }
        FidelitySummary::Both(diff) => diff,
    };
    if diff.is_empty() {
        println!("  source fidelity: identical");
        return;
    }
    println!("  source fidelity:");
    if let Some((before, after)) = &diff.version {
        println!("    version: {before} → {after}");
    }
    if diff.annotations_changed {
        println!("    annotations changed");
    }
    if diff.retained_records_changed {
        println!("    retained records changed");
    }
}

fn losses(report: Option<&DecodeReport>) -> Vec<cadmpeg_ir::LossNote> {
    report
        .map(|report| report.losses.clone())
        .unwrap_or_default()
}

fn resolve_format(explicit: Option<Format>, out: Option<&Path>) -> Result<Format> {
    if let Some(format) = explicit {
        if let Some(inferred) = Format::from_path(out) {
            if inferred != format {
                eprintln!(
                    "warning: explicit format {} disagrees with output extension format {}; using {}",
                    format.name(),
                    inferred.name(),
                    format.name()
                );
            }
        }
        return Ok(format);
    }
    Format::from_path(out).ok_or_else(|| anyhow!("cannot infer format; pass -f"))
}

/// Writes CADIR for the decode command (no conversion refusals).
#[allow(clippy::too_many_arguments)]
fn export_ir(
    ir: &CadIr,
    decode_report: Option<&DecodeReport>,
    source_fidelity: Option<&SourceFidelity>,
    format: Format,
    out: Option<&Path>,
    input: &Path,
    force: bool,
    encoder_request: EncoderRequest,
) -> Result<ExportReport> {
    if let Some(path) = out {
        ArtifactStore::check_output_path(input, path, force)?;
    }
    let encoder = build_encoder(format, encoder_request)?;
    let plan = encoder.plan(cadmpeg_ir::codec::EncodeInput {
        ir,
        fidelity: source_fidelity,
    })?;
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

#[derive(Clone, Copy)]
struct CommandReportBody<'a> {
    decode_report: Option<&'a DecodeReport>,
    validation_report: Option<&'a ValidationReport>,
    export: Option<&'a ExportReport>,
    refusal: Option<&'a ConversionRefusal>,
}

fn write_command_report(
    input: &Path,
    output: Option<&Path>,
    force: bool,
    command: &'static str,
    body: CommandReportBody<'_>,
) -> Result<()> {
    write_json_report(
        input,
        output,
        force,
        command,
        &serde_json::json!({
            "decode_report": body.decode_report,
            "validation_report": body.validation_report,
            "export": body.export,
        }),
        body.refusal,
    )
}

/// The version-and-revision string stamped into command reports.
pub(crate) fn generator() -> String {
    format!(
        "cadmpeg {}+g{}",
        env!("CARGO_PKG_VERSION"),
        env!("CADMPEG_BUILD_GIT")
    )
}

fn write_json_report(
    input: &Path,
    output: Option<&Path>,
    force: bool,
    command: &'static str,
    payload: &serde_json::Value,
    refusal: Option<&ConversionRefusal>,
) -> Result<()> {
    let Some(output) = output else {
        return Ok(());
    };
    let mut object = payload
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("command report payload must be a JSON object"))?;
    object.insert(
        "schema_version".to_string(),
        serde_json::json!(CLI_SCHEMA_VERSION),
    );
    object.insert("command".to_string(), serde_json::json!(command));
    // Names the writing binary so a stale report announces itself; see
    // `query summary`'s generator row.
    object.insert("generator".to_string(), serde_json::json!(generator()));
    match refusal {
        Some(refusal) => {
            let fields = refusal.report_fields();
            object.insert("status".to_string(), fields["status"].clone());
            object.insert("refusal".to_string(), fields["refusal"].clone());
        }
        None => {
            object.insert("status".to_string(), serde_json::json!("ok"));
            object.insert("refusal".to_string(), serde_json::Value::Null);
        }
    }
    let mut bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(object))?;
    bytes.push(b'\n');
    write_output(input, output, &bytes, force)?;
    eprintln!("wrote report {}", output.display());
    Ok(())
}

fn write_output(input: &Path, output: &Path, bytes: &[u8], force: bool) -> Result<()> {
    ArtifactStore::write_output(input, output, bytes, force)
}

fn print_id_delta(label: &str, ids: &[String]) {
    const MAX: usize = 8;
    if ids.is_empty() {
        return;
    }
    let more = ids.len().saturating_sub(MAX);
    let shown = ids
        .iter()
        .take(MAX)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if more > 0 {
        println!("      {label}: {shown} (+{more} more)");
    } else {
        println!("      {label}: {shown}");
    }
}

fn print_decode_report(writer: &mut impl Write, report: &DecodeReport) -> io::Result<()> {
    writeln!(
        writer,
        "decode report ({}): geometry_transferred={}, container_only={}",
        report.format, report.geometry_transferred, report.container_only
    )?;
    if !report.losses.is_empty() {
        writeln!(writer, "losses:")?;
        for loss in &report.losses {
            writeln!(
                writer,
                "  [{}/{}] {}",
                loss.severity,
                loss.code.category(),
                loss.message
            )?;
        }
    }
    for note in &report.notes {
        writeln!(writer, "  note: {note}")?;
    }
    Ok(())
}

fn print_validation_report(writer: &mut impl Write, report: &ValidationReport) -> io::Result<()> {
    writeln!(
        writer,
        "validation: {} ({} error(s), {} warning(s))",
        if report.is_ok() { "OK" } else { "FAILED" },
        report.error_count(),
        report.warning_count()
    )?;
    for (kind, count) in &report.entity_counts {
        if *count > 0 {
            writeln!(writer, "  {kind}: {count}")?;
        }
    }
    for finding in &report.findings {
        writeln!(
            writer,
            "  [{}/{}] {} ({})",
            finding.severity,
            finding.check,
            finding.message,
            finding.entity.as_deref().unwrap_or("-")
        )?;
    }
    Ok(())
}
