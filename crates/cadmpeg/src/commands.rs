// SPDX-License-Identifier: Apache-2.0
//! Command execution, artifact writing, and human-readable reports.

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::{validate, validate_with_source_fidelity, CadIr, CodecEntry, SourceFidelity};

use crate::application::{
    build_encoder, ArtifactStore, EncoderRequest, ForcedInput, InputCatalog,
    NativeValidatorCatalog, ResolveSourceError, ResolvedSource, SidecarPersistOutcome,
};
use crate::loader::{self, read_prefix, LoadNotice, DETECTION_PREFIX_LEN};
use crate::{DecodeArgs, Format};

pub(crate) const CLI_SCHEMA_VERSION: u32 = 5;

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
        Some(source_fidelity) => validate_with_source_fidelity(ir, source_fidelity, losses),
        None => validate(ir, losses),
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

#[derive(Debug)]
/// Error whose result is meaningful to the caller rather than operational.
///
/// The executable maps this error to exit status 1.
pub struct SemanticFailure(String);

/// Whether a conversion validates the neutral model before export.
#[derive(Debug, Clone, Copy)]
pub enum ValidationMode {
    /// Validate and optionally permit invalid output.
    Required {
        /// Continue despite validation errors.
        allow_invalid: bool,
    },
    /// Skip neutral validation.
    Skipped,
}

/// Complete policy and target configuration for one conversion pipeline.
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
    /// Explicit Rhino output archive version.
    #[cfg(feature = "rhino")]
    pub rhino_version: Option<cadmpeg_codec_rhino::RhinoArchiveVersion>,
    /// STEP writer options selected by the caller.
    #[cfg(feature = "step")]
    pub step_options: cadmpeg_codec_step::StepWriteOptions,
    /// IGES writer options selected by the caller.
    #[cfg(feature = "iges")]
    pub iges_options: cadmpeg_codec_iges::IgesWriteOptions,
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

impl fmt::Display for SemanticFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SemanticFailure {}

fn semantic(message: impl Into<String>) -> anyhow::Error {
    SemanticFailure(message.into()).into()
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
    )?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": CLI_SCHEMA_VERSION,
                "command": "inspect",
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
    let outcome = loader::load_artifact(&catalogs.inputs, path, args.options(), forced)?;
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
        false,
    )?;
    if let Some(report) = loaded.decode_report() {
        print_decode_report(&mut io::stderr(), report)?;
    }
    write_command_report(
        path,
        report_path,
        force,
        "decode",
        loaded.decode_report(),
        None,
        None,
    )?;
    Ok(())
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
    write_json_report(
        path,
        report_path,
        force,
        "validate",
        &serde_json::json!({
            "decode_report": loaded.decode_report(),
            "validation_report": report,
        }),
    )?;
    if json {
        writeln!(
            stdout,
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": CLI_SCHEMA_VERSION,
                "command": "validate",
                "decode_report": loaded.decode_report(),
                "validation_report": report,
            }))?
        )?;
    } else {
        print_validation_report(&mut stdout, &report)?;
    }
    if !report.is_ok() {
        return Err(semantic(format!(
            "validation found {} error(s)",
            report.error_count()
        )));
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
    if format.is_binary_container() && out.is_none() && !plan.binary_stdout {
        // Streaming a ZIP or 3DM to stdout is nearly always the --format /
        // --input-format mix-up, and the bytes get mistaken for JSON.
        bail!(
            "refusing to write binary {name} to standard output; pass -o FILE.{name}, or \
             --input-format {name} (alias --from) if you meant to force how the INPUT is \
             read; pass --binary-stdout to stream the bytes anyway",
            name = format.name()
        );
    }
    let outcome = loader::load_artifact(&catalogs.inputs, path, args.options(), plan.forced_input)?;
    print_load_notices(&outcome.notices);
    let loaded = &outcome.document;
    let mut stderr = io::stderr();
    if let Some(report) = loaded.decode_report() {
        print_decode_report(&mut stderr, report)?;
        if matches!(plan.validation, ValidationMode::Skipped) {
            eprintln!("note: export skips IR validation; use `convert` to validate");
        } else {
            writeln!(stderr)?;
        }
    }
    if let Some(refusal) = lossy_refusal(plan.reject_lossy, loaded.decode_report(), format) {
        write_command_report(
            path,
            plan.report.as_deref(),
            plan.force,
            command,
            loaded.decode_report(),
            None,
            None,
        )?;
        return Err(refusal);
    }
    let validation = match plan.validation {
        ValidationMode::Required { allow_invalid } => {
            let validation = validate_ir(
                &catalogs.validators,
                &loaded.ir,
                loaded.fidelity(),
                losses(loaded.decode_report()),
            );
            print_validation_report(&mut stderr, &validation)?;
            if !validation.is_ok() && !allow_invalid {
                write_command_report(
                    path,
                    plan.report.as_deref(),
                    plan.force,
                    command,
                    loaded.decode_report(),
                    Some(&validation),
                    None,
                )?;
                return Err(semantic(format!(
                    "validation found {} error(s); refusing to export (use --allow-invalid to override)",
                    validation.error_count()
                )));
            }
            Some(validation)
        }
        ValidationMode::Skipped => None,
    };
    if format.is_geometry_export()
        && loaded
            .decode_report()
            .as_ref()
            .is_some_and(|report| !report.geometry_transferred)
        && !plan.allow_empty
    {
        write_command_report(
            path,
            plan.report.as_deref(),
            plan.force,
            command,
            loaded.decode_report(),
            validation.as_ref(),
            None,
        )?;
        return Err(semantic(format!(
            "decode transferred no geometry; refusing to write an empty {} (use --allow-empty to override)",
            format.name()
        )));
    }
    #[cfg(feature = "rhino")]
    if plan.rhino_version.is_some() && format != Format::Rhino {
        bail!("--rhino-version requires Rhino output");
    }
    let encoder_request = encoder_request_for(plan, format);
    let report = export_ir(
        &loaded.ir,
        loaded.decode_report(),
        loaded.fidelity(),
        format,
        out,
        path,
        plan.force,
        encoder_request,
        plan.reject_lossy,
    )?;
    write_command_report(
        path,
        plan.report.as_deref(),
        plan.force,
        command,
        loaded.decode_report(),
        validation.as_ref(),
        Some(&report),
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
    )?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": CLI_SCHEMA_VERSION,
                "command": "diff",
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
            version: (left.version != right.version)
                .then(|| (left.version.clone(), right.version.clone())),
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

/// When `--reject-lossy` is set and the decode reported any loss, the export is
/// refused as a model refusal — [`SemanticFailure`], exit 1 — distinct from a
/// decode error, which is an operational failure at exit 2. This is the
/// `refused-lossy` category of the exit-code contract.
fn lossy_refusal(
    reject_lossy: bool,
    report: Option<&DecodeReport>,
    format: Format,
) -> Option<anyhow::Error> {
    if !reject_lossy {
        return None;
    }
    let count = report.map_or(0, |report| report.losses.len());
    (count > 0).then(|| {
        semantic(format!(
            "decode reported {count} loss(es); refusing to write a lossy {} (omit --reject-lossy to allow)",
            format.name()
        ))
    })
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

fn encoder_request_for(plan: &ConversionPlan, format: Format) -> EncoderRequest {
    match format {
        #[cfg(feature = "step")]
        Format::Step => EncoderRequest::Step(plan.step_options.clone()),
        #[cfg(feature = "rhino")]
        Format::Rhino => EncoderRequest::Rhino(
            plan.rhino_version
                .unwrap_or(cadmpeg_codec_rhino::RhinoArchiveVersion::V8),
        ),
        #[cfg(feature = "iges")]
        Format::Iges => EncoderRequest::Iges(plan.iges_options),
        _ => {
            let _ = plan;
            EncoderRequest::Neutral
        }
    }
}

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
    reject_lossy: bool,
) -> Result<ExportReport> {
    if let Some(path) = out {
        ArtifactStore::check_output_path(input, path, force)?;
    }
    let encoder = build_encoder(format, encoder_request)?;
    let plan = encoder.plan(cadmpeg_ir::codec::EncodeInput {
        ir,
        fidelity: source_fidelity,
    })?;
    if reject_lossy && !plan.report().losses.is_empty() {
        return Err(semantic(format!(
            "export planning reported {} loss(es); refusing to write a lossy {} (omit --reject-lossy to allow)",
            plan.report().losses.len(),
            format.name()
        )));
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

fn write_command_report(
    input: &Path,
    output: Option<&Path>,
    force: bool,
    command: &'static str,
    decode_report: Option<&DecodeReport>,
    validation_report: Option<&ValidationReport>,
    export: Option<&ExportReport>,
) -> Result<()> {
    write_json_report(
        input,
        output,
        force,
        command,
        &serde_json::json!({
            "decode_report": decode_report,
            "validation_report": validation_report,
            "export": export,
        }),
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
