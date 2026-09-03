// SPDX-License-Identifier: Apache-2.0
//! Human-readable and JSON command-report rendering.

use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::SourceFidelity;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::application::transcoder::{EmittedArtifact, ExportEmission};
use crate::application::{ArtifactStore, ConversionRefusal, SidecarPersistOutcome};

use super::CLI_SCHEMA_VERSION;

pub(super) fn print_source_diff(source: &cadmpeg_ir::SourceDiff) {
    if let Some(change) = &source.format_change {
        let before = change.before().unwrap_or("");
        let after = change.after().unwrap_or("");
        println!("  source format: {before} → {after}");
    }
    if let Some((before, after)) = &source.dialects_change {
        println!(
            "  source dialect layers: {} → {}",
            render_dialect_layers(before.as_ref()),
            render_dialect_layers(after.as_ref())
        );
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

fn render_dialect_layers(layers: Option<&cadmpeg_core::dialect::DialectLayers>) -> String {
    layers.map_or_else(
        || "<absent>".to_owned(),
        |layers| serde_json::to_string(layers).expect("dialect layers always serialize"),
    )
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

#[derive(Serialize)]
pub(super) struct FidelityDiff {
    annotations_changed: bool,
    retained_records_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<(String, String)>,
}

impl FidelityDiff {
    fn between(left: &SourceFidelity, right: &SourceFidelity) -> Self {
        Self {
            annotations_changed: left.annotations != right.annotations,
            retained_records_changed: left.retained_records != right.retained_records,
            version: (left.version() != right.version())
                .then(|| (left.version().to_owned(), right.version().to_owned())),
        }
    }

    fn is_empty(&self) -> bool {
        self.version.is_none() && !self.annotations_changed && !self.retained_records_changed
    }
}

impl Serialize for FidelitySummary {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::None => serializer.serialize_none(),
            Self::OnlyLeft | Self::OnlyRight => {
                let mut state = serializer.serialize_struct("FidelityPresence", 1)?;
                state.serialize_field(
                    "present",
                    if matches!(self, Self::OnlyLeft) {
                        "left_only"
                    } else {
                        "right_only"
                    },
                )?;
                state.end()
            }
            Self::Both(diff) => {
                let mut state = serializer.serialize_struct("FidelityComparison", 3)?;
                state.serialize_field("present", "both")?;
                state.serialize_field("different", &!diff.is_empty())?;
                state.serialize_field("diff", diff)?;
                state.end()
            }
        }
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
pub(super) enum CommandReportBody<'a> {
    Ok {
        decode_report: Option<&'a DecodeReport>,
        check_report: Option<&'a ValidationReport>,
        export: Option<&'a ExportReport>,
    },
    Refused(&'a ConversionRefusal),
}

impl<'a> CommandReportBody<'a> {
    fn command_report(self, command: &'static str) -> CommandReport<'a, Self> {
        match self {
            Self::Ok { .. } => CommandReport::ok(command, self),
            Self::Refused(refusal) => CommandReport::refused(command, self, refusal),
        }
    }
}

impl Serialize for CommandReportBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let reports = match self {
            Self::Ok {
                decode_report,
                check_report,
                export,
            } => (*decode_report, *check_report, *export),
            Self::Refused(refusal) => {
                let reports = refusal.evidence().reports;
                (reports.decode, reports.check, reports.export)
            }
        };
        let mut state = serializer.serialize_struct("CommandReportBody", 3)?;
        state.serialize_field("decode_report", &reports.0)?;
        state.serialize_field("check_report", &reports.1)?;
        state.serialize_field("export", &reports.2)?;
        state.end()
    }
}

pub(super) fn write_command_report(
    input: &Path,
    output: Option<&Path>,
    force: bool,
    command: &'static str,
    body: CommandReportBody<'_>,
) -> Result<()> {
    write_serialized_report(input, output, force, &body.command_report(command))
}

pub(super) fn command_body_json(
    command: &'static str,
    body: CommandReportBody<'_>,
) -> Result<String> {
    Ok(serde_json::to_string_pretty(&body.command_report(command))?)
}

fn generator() -> String {
    format!(
        "cadmpeg {}+g{}",
        env!("CARGO_PKG_VERSION"),
        env!("CADMPEG_BUILD_GIT")
    )
}

pub(super) fn print_export_emission(
    writer: &mut impl Write,
    emission: &ExportEmission,
) -> io::Result<()> {
    match &emission.artifact {
        EmittedArtifact::File { path, sidecar } => {
            match sidecar {
                SidecarPersistOutcome::Wrote(sidecar) => {
                    writeln!(writer, "wrote decode sidecar {}", sidecar.display())?;
                }
                SidecarPersistOutcome::RemovedStale(sidecar) => {
                    writeln!(writer, "removed stale decode sidecar {}", sidecar.display())?;
                }
                SidecarPersistOutcome::Absent => {}
            }
            writeln!(
                writer,
                "wrote {} ({} entities)",
                path.display(),
                emission.report.census.total()
            )?;
        }
        EmittedArtifact::StdoutWithoutSidecar => {
            writeln!(
                writer,
                "note: CADIR written to stdout cannot carry its decode-fidelity sidecar"
            )?;
        }
        EmittedArtifact::Stdout => {}
    }
    if !emission.report.losses.is_empty() {
        writeln!(writer, "{} export losses:", emission.report.format())?;
        for loss in &emission.report.losses {
            writeln!(
                writer,
                "  [{}/{}] {}",
                loss.severity,
                loss.code.category(),
                loss.message
            )?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum CommandStatus {
    Ok,
    Refused,
}

enum Outcome<'a> {
    Ok,
    Refused(crate::application::refusal::RefusalReport<'a>),
}

impl Serialize for Outcome<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("CommandOutcome", 2)?;
        match self {
            Self::Ok => {
                state.serialize_field("status", &CommandStatus::Ok)?;
                state.serialize_field(
                    "refusal",
                    &Option::<&crate::application::refusal::RefusalReport<'_>>::None,
                )?;
            }
            Self::Refused(refusal) => {
                state.serialize_field("status", &CommandStatus::Refused)?;
                state.serialize_field("refusal", refusal)?;
            }
        }
        state.end()
    }
}

#[derive(Serialize)]
struct CommandReport<'a, P> {
    schema_version: u32,
    command: &'static str,
    generator: String,
    #[serde(flatten)]
    outcome: Outcome<'a>,
    #[serde(flatten)]
    payload: P,
}

impl<'a, P> CommandReport<'a, P> {
    fn ok(command: &'static str, payload: P) -> Self {
        Self {
            schema_version: CLI_SCHEMA_VERSION,
            command,
            generator: generator(),
            outcome: Outcome::Ok,
            payload,
        }
    }

    fn refused(command: &'static str, payload: P, refusal: &'a ConversionRefusal) -> Self {
        Self {
            schema_version: CLI_SCHEMA_VERSION,
            command,
            generator: generator(),
            outcome: Outcome::Refused(refusal.report()),
            payload,
        }
    }
}

pub(crate) fn command_report_json<P: Serialize>(
    command: &'static str,
    payload: P,
) -> Result<String> {
    Ok(serde_json::to_string_pretty(&CommandReport::ok(
        command, payload,
    ))?)
}

pub(crate) fn refused_command_report_json<P: Serialize>(
    command: &'static str,
    payload: P,
    refusal: &ConversionRefusal,
) -> Result<String> {
    Ok(serde_json::to_string_pretty(&CommandReport::refused(
        command, payload, refusal,
    ))?)
}

pub(super) fn write_json_report<P: Serialize>(
    input: &Path,
    output: Option<&Path>,
    force: bool,
    command: &'static str,
    payload: &P,
) -> Result<()> {
    write_serialized_report(input, output, force, &CommandReport::ok(command, payload))
}

pub(super) fn write_refused_json_report<P: Serialize>(
    input: &Path,
    output: Option<&Path>,
    force: bool,
    command: &'static str,
    payload: &P,
    refusal: &ConversionRefusal,
) -> Result<()> {
    write_serialized_report(
        input,
        output,
        force,
        &CommandReport::refused(command, payload, refusal),
    )
}

fn write_serialized_report(
    input: &Path,
    output: Option<&Path>,
    force: bool,
    report: &impl Serialize,
) -> Result<()> {
    let Some(output) = output else {
        return Ok(());
    };
    let mut bytes = serde_json::to_vec_pretty(report)?;
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
        report.geometry_transferred(),
        report.container_only()
    )?;
    for line in crate::registry_view::dialect_lines(report.dialects()) {
        writeln!(writer, "{line}")?;
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

#[cfg(test)]
mod tests {
    use super::{FidelityDiff, FidelitySummary};

    #[test]
    fn fidelity_summary_serializes_the_v7_diff_shape_without_value_glue() {
        assert_eq!(
            serde_json::to_value(FidelitySummary::None).unwrap(),
            serde_json::Value::Null
        );
        assert_eq!(
            serde_json::to_value(FidelitySummary::OnlyLeft).unwrap(),
            serde_json::json!({ "present": "left_only" })
        );
        assert_eq!(
            serde_json::to_value(FidelitySummary::OnlyRight).unwrap(),
            serde_json::json!({ "present": "right_only" })
        );
        assert_eq!(
            serde_json::to_value(FidelitySummary::Both(FidelityDiff {
                annotations_changed: false,
                retained_records_changed: false,
                version: None,
            }))
            .unwrap(),
            serde_json::json!({
                "present": "both",
                "different": false,
                "diff": {
                    "annotations_changed": false,
                    "retained_records_changed": false
                }
            })
        );
        assert_eq!(
            serde_json::to_value(FidelitySummary::Both(FidelityDiff {
                annotations_changed: true,
                retained_records_changed: false,
                version: Some(("1".to_owned(), "2".to_owned())),
            }))
            .unwrap(),
            serde_json::json!({
                "present": "both",
                "different": true,
                "diff": {
                    "annotations_changed": true,
                    "retained_records_changed": false,
                    "version": ["1", "2"]
                }
            })
        );
    }
}
