// SPDX-License-Identifier: Apache-2.0
//! Command execution, artifact writing, and human-readable reports.

use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use cadmpeg_ir::codec::{CadirEncoder, CodecEntry, DecodeOptions, Encoder};
use cadmpeg_ir::decode::InspectOptions;
use cadmpeg_ir::report::{DecodeReport, ExportReport};
use cadmpeg_ir::source_fidelity::SourceFidelity;
use cadmpeg_ir::validate::ValidationReport;
use cadmpeg_ir::{validate, validate_with_source_fidelity, CadIr};

use crate::envelope::{envelope, print_json, write_output, ReportSink};
use crate::format::{ForcedInput, Format};
use crate::loader::{self, read_prefix, DETECTION_PREFIX_LEN};
use crate::registry::Registry;

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

impl fmt::Display for SemanticFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SemanticFailure {}

impl SemanticFailure {
    /// Whether this failure carries no message and should print nothing.
    pub fn is_silent(&self) -> bool {
        self.0.is_empty()
    }
}

fn semantic(message: impl Into<String>) -> anyhow::Error {
    SemanticFailure(message.into()).into()
}

/// A semantic failure carrying no message.
///
/// `diff` writes its comparison to stdout, then returns this to report a
/// non-empty diff as exit status 1 without printing an error line — the
/// quiet-stderr half of the diff exit contract.
pub(crate) fn semantic_silent() -> anyhow::Error {
    SemanticFailure(String::new()).into()
}

/// Inspect a native container and print its entries.
pub fn inspect(
    registry: &Registry,
    path: &Path,
    forced: Option<ForcedInput>,
    json: bool,
    report: Option<&Path>,
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
    if json || report.is_some() {
        let payload = serde_json::json!({
            "confidence": confidence,
            "summary": serde_json::to_value(&summary)?,
        });
        let sink = ReportSink {
            input: path,
            output: report,
            force: false,
            command: "inspect",
        };
        if json {
            sink.write_payload(payload.clone())?;
            print_json(&envelope("inspect", payload))?;
            return Ok(());
        }
        sink.write_payload(payload)?;
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
    options: DecodeOptions,
) -> Result<()> {
    let loaded = loader::load_ir(registry, path, options, forced)?;
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
    options: DecodeOptions,
    json: bool,
    report_path: Option<&Path>,
) -> Result<()> {
    let loaded = loader::load_ir(registry, path, options, forced)?;
    if let Some(decode_report) = &loaded.decode_report {
        print_decode_report(&mut io::stderr(), decode_report)?;
    }
    let report = validate_ir(
        registry,
        &loaded.ir,
        loaded.source_fidelity.as_ref(),
        losses(loaded.decode_report.as_ref()),
    );
    if json || report_path.is_some() {
        let payload = serde_json::json!({
            "decode_report": serde_json::to_value(&loaded.decode_report)?,
            "validation_report": serde_json::to_value(&report)?,
        });
        let sink = ReportSink {
            input: path,
            output: report_path,
            force: false,
            command: "validate",
        };
        if json {
            sink.write_payload(payload.clone())?;
            print_json(&envelope("validate", payload))?;
        } else {
            sink.write_payload(payload)?;
            print_validation_report(&mut io::stdout(), &report)?;
        }
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

/// Whether an export validates CADIR before writing.
pub enum ValidationGate {
    /// Export without validating, as `export` does.
    Skip,
    /// Validate first, refusing on errors unless `allow_invalid`, as `convert` does.
    Require {
        /// Export despite CADIR validation errors.
        allow_invalid: bool,
    },
}

/// One export's format, encoder, destinations, and safety gates.
///
/// This carries everything an export needs beyond the registry, input path, and
/// decode options, so `export` and `convert` differ only in their [`ValidationGate`].
pub struct ExportPipeline<'a> {
    /// Resolved output format.
    pub format: Format,
    /// Encoder for `format`, carried unresolved so its guard surfaces in order.
    pub encoder: Result<Box<dyn Encoder>>,
    /// Output artifact path, or `None` to write to standard output.
    pub out: Option<&'a Path>,
    /// Versioned JSON report path, or `None` for no report.
    pub report: Option<&'a Path>,
    /// Replace an existing output or report file.
    pub force: bool,
    /// Export a geometry format when decoding transferred no geometry.
    pub allow_empty: bool,
    /// Refuse to export when the decode reported any loss.
    pub reject_lossy: bool,
    /// Explicit input format selected by the user.
    pub forced_input: Option<ForcedInput>,
    /// Whether CADIR is validated before writing.
    pub gate: ValidationGate,
}

/// Decode if needed, optionally validate, then export.
pub fn run_export(
    registry: &Registry,
    input: &Path,
    pipeline: ExportPipeline<'_>,
    options: DecodeOptions,
) -> Result<()> {
    let ExportPipeline {
        format,
        encoder,
        out,
        report: report_path,
        force,
        allow_empty,
        reject_lossy,
        forced_input,
        gate,
    } = pipeline;
    let loaded = loader::load_ir(registry, input, options, forced_input)?;
    let sink = ReportSink {
        input,
        output: report_path,
        force,
        command: match gate {
            ValidationGate::Skip => "export",
            ValidationGate::Require { .. } => "convert",
        },
    };
    let mut stderr = io::stderr();
    if let Some(report) = &loaded.decode_report {
        print_decode_report(&mut stderr, report)?;
        match gate {
            ValidationGate::Skip => {
                eprintln!("note: export skips IR validation; use `convert` to validate");
            }
            ValidationGate::Require { .. } => writeln!(stderr)?,
        }
    }
    if let Some(refusal) = lossy_refusal(reject_lossy, loaded.decode_report.as_ref(), format) {
        sink.write(loaded.decode_report.as_ref(), None, None)?;
        return Err(refusal);
    }
    let validation = match gate {
        ValidationGate::Skip => None,
        ValidationGate::Require { allow_invalid } => {
            let validation = validate_ir(
                registry,
                &loaded.ir,
                loaded.source_fidelity.as_ref(),
                losses(loaded.decode_report.as_ref()),
            );
            print_validation_report(&mut stderr, &validation)?;
            if !validation.is_ok() && !allow_invalid {
                sink.write(loaded.decode_report.as_ref(), Some(&validation), None)?;
                return Err(semantic(format!(
                    "validation found {} error(s); refusing to export (use --allow-invalid to override)",
                    validation.error_count()
                )));
            }
            Some(validation)
        }
    };
    if format.is_geometry_export()
        && loaded
            .decode_report
            .as_ref()
            .is_some_and(|report| !report.geometry_transferred)
        && !allow_empty
    {
        sink.write(loaded.decode_report.as_ref(), validation.as_ref(), None)?;
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
        input,
        force,
    )?;
    sink.write(
        loaded.decode_report.as_ref(),
        validation.as_ref(),
        Some(&report),
    )
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
