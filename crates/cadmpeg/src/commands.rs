// SPDX-License-Identifier: Apache-2.0
//! Command execution, artifact writing, and human-readable reports.

use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::{validate, validate_with_source_fidelity, CadIr, SourceFidelity};

use crate::loader::{self, read_prefix};
use crate::registry::Registry;
use crate::{DecodeArgs, ForcedInput, Format};

const CLI_SCHEMA_VERSION: u32 = 3;

fn validate_ir(
    ir: &CadIr,
    source_fidelity: Option<&SourceFidelity>,
    losses: Vec<cadmpeg_ir::LossNote>,
) -> ValidationReport {
    let mut report = match source_fidelity {
        Some(source_fidelity) => validate_with_source_fidelity(ir, source_fidelity, losses),
        None => validate(ir, losses),
    };
    if ir.native.namespace("f3d").is_some() {
        report
            .findings
            .extend(cadmpeg_codec_f3d::validate::validate_native(ir));
    }
    if ir.native.namespace("fcstd").is_some() {
        report
            .findings
            .extend(cadmpeg_codec_freecad::validate_native(ir));
    }
    if ir.native.namespace("sldprt").is_some() {
        report
            .findings
            .extend(cadmpeg_codec_sldprt::validate_native(ir));
    }
    report
}

#[derive(Debug)]
/// Error whose result is meaningful to the caller rather than operational.
///
/// The executable maps this error to exit status 1.
pub struct SemanticFailure(String);

/// Safety and reporting options for `convert`.
pub struct ConvertSettings {
    /// Replace an existing output or report file.
    pub force: bool,
    /// Optional path for the versioned JSON command report.
    pub report: Option<PathBuf>,
    /// Export despite CADIR validation errors.
    pub allow_invalid: bool,
    /// Export a geometry format when decoding transferred no geometry.
    pub allow_empty: bool,
    /// Explicit Rhino output archive version.
    pub rhino_version: Option<cadmpeg_codec_rhino::RhinoArchiveVersion>,
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
    let prefix = read_prefix(path, 512)?;
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
        .inspect(&mut file)
        .with_context(|| format!("inspecting {}", path.display()))?;
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
        registry,
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        Format::Cadir,
        out,
        path,
        force,
        None,
    )?;
    if let Some(report) = &loaded.decode_report {
        print_decode_report(&mut io::stderr(), report)?;
    }
    write_command_report(
        path,
        report_path,
        force,
        "decode",
        loaded.decode_report.as_ref(),
        None,
        None,
    )?;
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
    let mut stdout = io::stdout();
    if let Some(report) = &loaded.decode_report {
        print_decode_report(&mut io::stderr(), report)?;
    }
    let report = validate_ir(
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        losses(loaded.decode_report.as_ref()),
    );
    if json {
        writeln!(
            stdout,
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": CLI_SCHEMA_VERSION,
                "command": "validate",
                "decode_report": loaded.decode_report,
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

/// Safety and reporting options for `export`.
pub struct ExportSettings {
    /// Replace an existing output or report file.
    pub force: bool,
    /// Optional path for the versioned JSON command report.
    pub report: Option<PathBuf>,
    /// Export a geometry format when decoding transferred no geometry.
    pub allow_empty: bool,
    /// Explicit Rhino output archive version.
    pub rhino_version: Option<cadmpeg_codec_rhino::RhinoArchiveVersion>,
    /// Explicit input format selected by the user.
    pub forced_input: Option<ForcedInput>,
}

/// Decode if needed and export without validating CADIR.
pub fn export(
    registry: &Registry,
    path: &Path,
    format: Option<Format>,
    out: Option<&Path>,
    settings: ExportSettings,
    args: &DecodeArgs,
) -> Result<()> {
    let ExportSettings {
        force,
        report: report_path,
        allow_empty,
        rhino_version,
        forced_input,
    } = settings;
    let format = resolve_format(format, out)?;
    let loaded = loader::load_ir(registry, path, args.options(), forced_input)?;
    if let Some(report) = &loaded.decode_report {
        print_decode_report(&mut io::stderr(), report)?;
        eprintln!("note: export skips IR validation; use `convert` to validate");
    }
    if format.is_geometry_export()
        && loaded
            .decode_report
            .as_ref()
            .is_some_and(|report| !report.geometry_transferred)
        && !allow_empty
    {
        write_command_report(
            path,
            report_path.as_deref(),
            force,
            "export",
            loaded.decode_report.as_ref(),
            None,
            None,
        )?;
        return Err(semantic(format!(
            "decode transferred no geometry; refusing to write an empty {} (use --allow-empty to override)",
            format.name()
        )));
    }
    let report = export_ir(
        registry,
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        format,
        out,
        path,
        force,
        rhino_version,
    )?;
    write_command_report(
        path,
        report_path.as_deref(),
        force,
        "export",
        loaded.decode_report.as_ref(),
        None,
        Some(&report),
    )
}

/// Decode if needed, validate CADIR, and export.
pub fn convert(
    registry: &Registry,
    path: &Path,
    format: Option<Format>,
    out: Option<&Path>,
    settings: &ConvertSettings,
    args: &DecodeArgs,
) -> Result<()> {
    let format = resolve_format(format, out)?;
    let loaded = loader::load_ir(registry, path, args.options(), settings.forced_input)?;
    let mut stderr = io::stderr();
    if let Some(report) = &loaded.decode_report {
        print_decode_report(&mut stderr, report)?;
        writeln!(stderr)?;
    }
    let validation = validate_ir(
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        losses(loaded.decode_report.as_ref()),
    );
    print_validation_report(&mut stderr, &validation)?;
    if !validation.is_ok() && !settings.allow_invalid {
        write_command_report(
            path,
            settings.report.as_deref(),
            settings.force,
            "convert",
            loaded.decode_report.as_ref(),
            Some(&validation),
            None,
        )?;
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
        write_command_report(
            path,
            settings.report.as_deref(),
            settings.force,
            "convert",
            loaded.decode_report.as_ref(),
            Some(&validation),
            None,
        )?;
        return Err(semantic(format!(
            "decode transferred no geometry; refusing to write an empty {} (use --allow-empty to override)",
            format.name()
        )));
    }
    let report = export_ir(
        registry,
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        format,
        out,
        path,
        settings.force,
        settings.rhino_version,
    )?;
    write_command_report(
        path,
        settings.report.as_deref(),
        settings.force,
        "convert",
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
    let left = loader::load_ir(registry, a, args.options(), None)?.ir;
    let right = loader::load_ir(registry, b, args.options(), None)?.ir;
    let result = cadmpeg_ir::diff(&left, &right);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": CLI_SCHEMA_VERSION,
                "command": "diff",
                "different": !result.is_empty(),
                "diff": result,
            }))?
        );
        return Ok(if result.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
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
    if result.is_empty() {
        println!("  identical");
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
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

#[allow(clippy::too_many_arguments)]
fn export_ir(
    registry: &Registry,
    ir: &CadIr,
    source_fidelity: Option<&SourceFidelity>,
    format: Format,
    out: Option<&Path>,
    input: &Path,
    force: bool,
    rhino_version: Option<cadmpeg_codec_rhino::RhinoArchiveVersion>,
) -> Result<ExportReport> {
    let mut bytes = Vec::new();
    if rhino_version.is_some() && format != Format::Rhino {
        bail!("--rhino-version requires Rhino output");
    }
    let report = registry
        .encode_by_id(
            format.name(),
            rhino_version,
            ir,
            source_fidelity,
            &mut bytes,
        )
        .ok_or_else(|| anyhow!("no encoder registered for {}", format.name()))??;
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

fn write_command_report(
    input: &Path,
    output: Option<&Path>,
    force: bool,
    command: &'static str,
    decode_report: Option<&DecodeReport>,
    validation_report: Option<&ValidationReport>,
    export: Option<&ExportReport>,
) -> Result<()> {
    let Some(output) = output else {
        return Ok(());
    };
    let mut bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": CLI_SCHEMA_VERSION,
        "command": command,
        "decode_report": decode_report,
        "validation_report": validation_report,
        "export": export,
    }))?;
    bytes.push(b'\n');
    write_output(input, output, &bytes, force)?;
    eprintln!("wrote report {}", output.display());
    Ok(())
}

fn write_output(input: &Path, output: &Path, bytes: &[u8], force: bool) -> Result<()> {
    let input = std::fs::canonicalize(input)
        .with_context(|| format!("canonicalizing {}", input.display()))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let output_absolute = if output.exists() {
        std::fs::canonicalize(output)?
    } else {
        std::fs::canonicalize(parent)?.join(
            output
                .file_name()
                .ok_or_else(|| anyhow!("output path has no filename"))?,
        )
    };
    if input == output_absolute {
        bail!("refusing to overwrite input {}", input.display());
    }
    if output.exists() && !force {
        bail!("{} exists; pass --force to overwrite", output.display());
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temporary output in {}", parent.display()))?;
    temporary
        .write_all(bytes)
        .with_context(|| format!("writing temporary output for {}", output.display()))?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("persisting temporary output to {}", output.display()))?;
    Ok(())
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
