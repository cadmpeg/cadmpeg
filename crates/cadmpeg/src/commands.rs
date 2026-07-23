// SPDX-License-Identifier: Apache-2.0
//! Command execution, artifact writing, and human-readable reports.

use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use cadmpeg_ir::codec::{CadirEncoder, Encoder};
use cadmpeg_ir::decode::InspectOptions;
use cadmpeg_ir::report::{DecodeReport, ExportReport};
use cadmpeg_ir::validate::ValidationReport;
use cadmpeg_ir::{validate, validate_with_source_fidelity, CadIr, CodecEntry, SourceFidelity};

use crate::envelope::{envelope, print_json, write_output, ReportSink};
use crate::format::{ForcedInput, Format};
use crate::loader::{self, read_prefix, DETECTION_PREFIX_LEN};
use crate::registry::Registry;
use crate::DecodeArgs;

fn validate_ir(
    registry: &Registry,
    ir: &CadIr,
    source_fidelity: Option<&SourceFidelity>,
    losses: Vec<cadmpeg_ir::LossNote>,
) -> ValidationReport {
    let mut report = match source_fidelity {
        Some(source_fidelity) => validate_with_source_fidelity(ir, source_fidelity, losses),
        None => validate(ir, losses),
    };
    report.findings.extend(registry.native_findings(ir));
    report
}

#[derive(Debug)]
/// Error whose result is meaningful to the caller rather than operational.
///
/// The executable maps this error to exit status 1.
pub struct SemanticFailure(String);

/// Safety and reporting options for `convert`.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent, orthogonal CLI safety toggle the user opts into by name; an enum would obscure that they compose freely"
)]
pub struct ConvertSettings {
    /// Replace an existing output or report file.
    pub force: bool,
    /// Optional path for the versioned JSON command report.
    pub report: Option<PathBuf>,
    /// Export despite CADIR validation errors.
    pub allow_invalid: bool,
    /// Export a geometry format when decoding transferred no geometry.
    pub allow_empty: bool,
    /// Refuse to export when the decode reported any loss.
    pub reject_lossy: bool,
    /// Explicit input format selected by the user.
    pub forced_input: Option<ForcedInput>,
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
    registry: &Registry,
    path: &Path,
    forced: Option<ForcedInput>,
    json: bool,
) -> Result<()> {
    let prefix = read_prefix(path, DETECTION_PREFIX_LEN)?;
    let (codec, confidence) = match forced {
        Some(ForcedInput::Codec(id)) => (
            registry
                .by_id(id)
                .ok_or_else(|| anyhow!("unsupported input format {id}"))?,
            None,
        ),
        Some(ForcedInput::Cadir) => bail!("inspect requires a container input, not cadir"),
        None => {
            let (codec, confidence) = registry.detect(&prefix).ok_or_else(|| {
                anyhow!("no codec recognized {}; inspect supports container inputs only, not .cadir.json IR documents; supported: FCStd, f3d, sldprt, CATPart, NX/Creo prt, Rhino 3DM, IGES, STEP; use --input-format to override detection", path.display())
            })?;
            (codec, Some(confidence))
        }
    };
    let mut file = File::open(path)?;
    let summary = codec
        .inspect(&mut file, &InspectOptions::default())
        .with_context(|| format!("inspecting {}", path.display()))?;
    if json {
        print_json(&envelope(
            "inspect",
            serde_json::json!({
                "confidence": confidence,
                "summary": summary,
            }),
        ))?;
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
    registry: &Registry,
    path: &Path,
    out: Option<&Path>,
    force: bool,
    report_path: Option<&Path>,
    forced: Option<ForcedInput>,
    args: &DecodeArgs,
) -> Result<()> {
    let loaded = loader::load_ir(registry, path, args.options(), forced)?;
    export_ir(
        &CadirEncoder,
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        out,
        path,
        force,
    )?;
    if let Some(report) = &loaded.decode_report {
        print_decode_report(&mut io::stderr(), report)?;
    }
    ReportSink {
        input: path,
        output: report_path,
        force,
        command: "decode",
    }
    .write(loaded.decode_report.as_ref(), None, None)?;
    Ok(())
}

/// Load and validate CADIR, printing a human-readable or JSON report.
pub fn validate_cmd(
    registry: &Registry,
    path: &Path,
    forced: Option<ForcedInput>,
    args: &DecodeArgs,
    json: bool,
) -> Result<()> {
    let loaded = loader::load_ir(registry, path, args.options(), forced)?;
    if let Some(report) = &loaded.decode_report {
        print_decode_report(&mut io::stderr(), report)?;
    }
    let report = validate_ir(
        registry,
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        losses(loaded.decode_report.as_ref()),
    );
    if json {
        print_json(&envelope(
            "validate",
            serde_json::json!({
                "decode_report": loaded.decode_report,
                "validation_report": report,
            }),
        ))?;
    } else {
        print_validation_report(&mut io::stdout(), &report)?;
    }
    if !report.is_ok() {
        return Err(semantic(format!(
            "validation found {} error(s)",
            report.error_count()
        )));
    }
    Ok(())
}

/// Safety and reporting options for `export`.
pub struct ExportSettings {
    /// Replace an existing output or report file.
    pub force: bool,
    /// Optional path for the versioned JSON command report.
    pub report: Option<PathBuf>,
    /// Export a geometry format when decoding transferred no geometry.
    pub allow_empty: bool,
    /// Refuse to export when the decode reported any loss.
    pub reject_lossy: bool,
    /// Explicit input format selected by the user.
    pub forced_input: Option<ForcedInput>,
}

/// Decode if needed and export without validating CADIR.
pub fn export(
    registry: &Registry,
    path: &Path,
    format: Format,
    out: Option<&Path>,
    encoder: Result<Box<dyn Encoder>>,
    settings: ExportSettings,
    args: &DecodeArgs,
) -> Result<()> {
    let ExportSettings {
        force,
        report: report_path,
        allow_empty,
        reject_lossy,
        forced_input,
    } = settings;
    let loaded = loader::load_ir(registry, path, args.options(), forced_input)?;
    let sink = ReportSink {
        input: path,
        output: report_path.as_deref(),
        force,
        command: "export",
    };
    if let Some(report) = &loaded.decode_report {
        print_decode_report(&mut io::stderr(), report)?;
        eprintln!("note: export skips IR validation; use `convert` to validate");
    }
    if let Some(refusal) = lossy_refusal(reject_lossy, loaded.decode_report.as_ref(), format) {
        sink.write(loaded.decode_report.as_ref(), None, None)?;
        return Err(refusal);
    }
    if format.is_geometry_export()
        && loaded
            .decode_report
            .as_ref()
            .is_some_and(|report| !report.geometry_transferred)
        && !allow_empty
    {
        sink.write(loaded.decode_report.as_ref(), None, None)?;
        return Err(semantic(format!(
            "decode transferred no geometry; refusing to write an empty {} (use --allow-empty to override)",
            format.name()
        )));
    }
    let report = export_ir(
        &*encoder?,
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        out,
        path,
        force,
    )?;
    sink.write(loaded.decode_report.as_ref(), None, Some(&report))
}

/// Decode if needed, validate CADIR, and export.
pub fn convert(
    registry: &Registry,
    path: &Path,
    format: Format,
    out: Option<&Path>,
    encoder: Result<Box<dyn Encoder>>,
    settings: &ConvertSettings,
    args: &DecodeArgs,
) -> Result<()> {
    let loaded = loader::load_ir(registry, path, args.options(), settings.forced_input)?;
    let sink = ReportSink {
        input: path,
        output: settings.report.as_deref(),
        force: settings.force,
        command: "convert",
    };
    let mut stderr = io::stderr();
    if let Some(report) = &loaded.decode_report {
        print_decode_report(&mut stderr, report)?;
        writeln!(stderr)?;
    }
    if let Some(refusal) =
        lossy_refusal(settings.reject_lossy, loaded.decode_report.as_ref(), format)
    {
        sink.write(loaded.decode_report.as_ref(), None, None)?;
        return Err(refusal);
    }
    let validation = validate_ir(
        registry,
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        losses(loaded.decode_report.as_ref()),
    );
    print_validation_report(&mut stderr, &validation)?;
    if !validation.is_ok() && !settings.allow_invalid {
        sink.write(loaded.decode_report.as_ref(), Some(&validation), None)?;
        return Err(semantic(format!(
            "validation found {} error(s); refusing to export (use --allow-invalid to override)",
            validation.error_count()
        )));
    }
    if format.is_geometry_export()
        && loaded
            .decode_report
            .as_ref()
            .is_some_and(|report| !report.geometry_transferred)
        && !settings.allow_empty
    {
        sink.write(loaded.decode_report.as_ref(), Some(&validation), None)?;
        return Err(semantic(format!(
            "decode transferred no geometry; refusing to write an empty {} (use --allow-empty to override)",
            format.name()
        )));
    }
    let report = export_ir(
        &*encoder?,
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        out,
        path,
        settings.force,
    )?;
    sink.write(
        loaded.decode_report.as_ref(),
        Some(&validation),
        Some(&report),
    )
}

/// Structurally compare two decoded models.
pub fn diff(
    registry: &Registry,
    a: &Path,
    b: &Path,
    args: &DecodeArgs,
    json: bool,
) -> Result<ExitCode> {
    let left = loader::load_ir(registry, a, args.options(), None)?;
    let right = loader::load_ir(registry, b, args.options(), None)?;
    let result = cadmpeg_ir::diff(&left.ir, &right.ir);
    let fidelity = fidelity_diff(
        left.source_fidelity.as_ref(),
        right.source_fidelity.as_ref(),
    );
    let different = !result.is_empty() || fidelity_differs(&fidelity);
    if json {
        print_json(&envelope(
            "diff",
            serde_json::json!({
                "different": different,
                "diff": result,
                "source_fidelity": fidelity_json(&fidelity),
            }),
        ))?;
        return Ok(if different {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        });
    }
    println!("diff {} vs {}", a.display(), b.display());
    if let Some((before, after)) = &result.unit_change {
        println!("  units: {before:?} → {after:?}");
    }
    if let Some((before, after)) = &result.tolerance_change {
        println!("  tolerances: {before:?} → {after:?}");
    }
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

fn export_ir(
    encoder: &dyn Encoder,
    ir: &CadIr,
    source_fidelity: Option<&SourceFidelity>,
    out: Option<&Path>,
    input: &Path,
    force: bool,
) -> Result<ExportReport> {
    let mut bytes = Vec::new();
    let report = encoder.encode_with_source_fidelity(ir, source_fidelity, &mut bytes)?;
    if let Some(path) = out {
        write_output(input, path, &bytes, force)?;
        eprintln!(
            "wrote {} ({} entities)",
            path.display(),
            report.total_entities
        );
    } else {
        io::stdout().write_all(&bytes)?;
    }
    if !report.losses.is_empty() {
        eprintln!("{} export losses:", report.format);
        for loss in &report.losses {
            eprintln!("  [{}/{}] {}", loss.severity, loss.category, loss.message);
        }
    }
    Ok(report)
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
                loss.severity, loss.category, loss.message
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
