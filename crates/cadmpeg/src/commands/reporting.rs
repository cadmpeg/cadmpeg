// SPDX-License-Identifier: Apache-2.0
//! Human-readable and JSON command-report rendering.

use std::io::{self, Write};
use std::path::Path;

use anyhow::{anyhow, Result};
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::SourceFidelity;

use crate::application::{ArtifactStore, ConversionRefusal};

use super::CLI_SCHEMA_VERSION;

pub(super) fn print_source_diff(source: &cadmpeg_ir::SourceDiff) {
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

fn render_attribute(value: Option<&str>) -> String {
    value.map_or_else(|| "<absent>".to_owned(), ToOwned::to_owned)
}

pub(super) enum FidelitySummary {
    None,
    OnlyLeft,
    OnlyRight,
    Both(FidelityDiff),
}

pub(super) struct FidelityDiff {
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

pub(super) fn fidelity_diff(
    left: Option<&SourceFidelity>,
    right: Option<&SourceFidelity>,
) -> FidelitySummary {
    match (left, right) {
        (Some(left), Some(right)) => FidelitySummary::Both(FidelityDiff::between(left, right)),
        (Some(_), None) => FidelitySummary::OnlyLeft,
        (None, Some(_)) => FidelitySummary::OnlyRight,
        (None, None) => FidelitySummary::None,
    }
}

pub(super) fn fidelity_differs(summary: &FidelitySummary) -> bool {
    match summary {
        FidelitySummary::None => false,
        FidelitySummary::OnlyLeft | FidelitySummary::OnlyRight => true,
        FidelitySummary::Both(diff) => !diff.is_empty(),
    }
}

pub(super) fn fidelity_json(summary: &FidelitySummary) -> serde_json::Value {
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

pub(super) fn print_fidelity_summary(summary: &FidelitySummary) {
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

pub(super) fn losses(report: Option<&DecodeReport>) -> Vec<cadmpeg_ir::LossNote> {
    report
        .map(|report| report.losses.clone())
        .unwrap_or_default()
}

#[derive(Clone, Copy)]
pub(super) struct CommandReportBody<'a> {
    pub(super) decode_report: Option<&'a DecodeReport>,
    pub(super) check_report: Option<&'a ValidationReport>,
    pub(super) export: Option<&'a ExportReport>,
    pub(super) refusal: Option<&'a ConversionRefusal>,
}

pub(super) fn write_command_report(
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
            "check_report": body.check_report,
            "export": body.export,
        }),
        body.refusal,
    )
}

fn generator() -> String {
    format!(
        "cadmpeg {}+g{}",
        env!("CARGO_PKG_VERSION"),
        env!("CADMPEG_BUILD_GIT")
    )
}

pub(super) fn write_json_report(
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
    ArtifactStore::write_output(input, output, &bytes, force)?;
    eprintln!("wrote report {}", output.display());
    Ok(())
}

pub(super) fn print_id_delta(label: &str, ids: &[String]) {
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

pub(super) fn print_decode_report(
    writer: &mut impl Write,
    report: &DecodeReport,
) -> io::Result<()> {
    writeln!(
        writer,
        "decode report ({}): geometry_transferred={}, container_only={}",
        report.format(),
        report.geometry_transferred,
        report.container_only
    )?;
    if let Some(dialects) = report.dialects() {
        writeln!(writer, "dialects:")?;
        for dialect in dialects.iter() {
            writeln!(writer, "  {}: {}", dialect.format, dialect.dialect)?;
        }
    }
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

pub(super) fn print_check_report(
    writer: &mut impl Write,
    report: &ValidationReport,
) -> io::Result<()> {
    writeln!(
        writer,
        "check: {} ({} error(s), {} warning(s))",
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
