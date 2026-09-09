// SPDX-License-Identifier: Apache-2.0
//! Human-readable and JSON command-report rendering.

use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;
use cadmpeg_ir::report::{DecodeReport, ExportReport, ValidationReport};
use cadmpeg_ir::SourceFidelity;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::application::artifact_store::{FileDestination, SidecarPersistOutcome};
use crate::application::refusal::ConversionRefusal;
use crate::application::transcoder::{EmittedArtifact, ExportEmission};

pub(super) fn print_source_diff(source: &cadmpeg_ir::SourceDiff) {
    if let Some(change) = &source.format_change {
        let before = change.before().unwrap_or("");
        let after = change.after().unwrap_or("");
        println!("  source format: {before} → {after}");
    }
    if let Some(change) = &source.dialects_change {
        println!(
            "  source dialect layers: {} → {}",
            render_dialect_layers(change.before()),
            render_dialect_layers(change.after())
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
}

impl FidelityDiff {
    fn between(left: &SourceFidelity, right: &SourceFidelity) -> Self {
        Self {
            annotations_changed: left.annotations != right.annotations,
            retained_records_changed: left.retained_records != right.retained_records,
        }
    }

    fn is_empty(&self) -> bool {
        !self.annotations_changed && !self.retained_records_changed
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
    if diff.annotations_changed {
        println!("    annotations changed");
    }
    if diff.retained_records_changed {
        println!("    retained records changed");
    }
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

/// Reports completed by a command.
#[derive(Serialize)]
pub(super) struct Reports<'a> {
    decode_report: Option<&'a DecodeReport>,
    check_report: Option<&'a ValidationReport>,
    export: Option<&'a ExportReport>,
}

impl CommandReportBody<'_> {
    fn command_report(&self, command: &'static str) -> CommandReport<'_, Reports<'_>> {
        CommandReport::new(command, self.payload())
    }

    /// Returns the reports with their command status.
    pub(super) fn payload(&self) -> Payload<'_, Reports<'_>> {
        match self {
            Self::Ok {
                decode_report,
                check_report,
                export,
            } => Payload::Ok(Reports {
                decode_report: *decode_report,
                check_report: *check_report,
                export: *export,
            }),
            Self::Refused(refusal) => {
                let reports = refusal.evidence().reports;
                Payload::Refused(
                    Reports {
                        decode_report: reports.decode,
                        check_report: reports.check,
                        export: reports.export,
                    },
                    refusal,
                )
            }
        }
    }
}

pub(super) fn write_command_report(
    input: &Path,
    output: Option<&FileDestination>,
    command: &'static str,
    body: CommandReportBody<'_>,
) -> Result<()> {
    write_serialized_report(input, output, &body.command_report(command))
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

/// Status-bearing serialized command payload.
pub(super) enum Payload<'a, P> {
    Ok(P),
    Refused(P, &'a ConversionRefusal),
}

impl<P: Serialize> Serialize for Payload<'_, P> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Fields<'a, P> {
            status: CommandStatus,
            refusal: Option<crate::application::refusal::RefusalReport<'a>>,
            #[serde(flatten)]
            payload: P,
        }
        let fields = match self {
            Self::Ok(payload) => Fields {
                status: CommandStatus::Ok,
                refusal: None,
                payload,
            },
            Self::Refused(payload, refusal) => Fields {
                status: CommandStatus::Refused,
                refusal: Some(refusal.report()),
                payload,
            },
        };
        fields.serialize(serializer)
    }
}

#[derive(Serialize)]
struct CommandReport<'a, P> {
    command: &'static str,
    generator: String,
    #[serde(flatten)]
    payload: Payload<'a, P>,
}

impl<'a, P> CommandReport<'a, P> {
    fn new(command: &'static str, payload: Payload<'a, P>) -> Self {
        Self {
            command,
            generator: generator(),
            payload,
        }
    }
}

pub(crate) fn command_report_json<P: Serialize>(
    command: &'static str,
    payload: P,
) -> Result<String> {
    Ok(serde_json::to_string_pretty(&CommandReport::new(
        command,
        Payload::Ok(payload),
    ))?)
}

pub(crate) fn refused_command_report_json<P: Serialize>(
    command: &'static str,
    payload: P,
    refusal: &ConversionRefusal,
) -> Result<String> {
    Ok(serde_json::to_string_pretty(&CommandReport::new(
        command,
        Payload::Refused(payload, refusal),
    ))?)
}

pub(super) fn write_json_report<P: Serialize>(
    input: &Path,
    output: Option<&FileDestination>,
    command: &'static str,
    payload: &P,
) -> Result<()> {
    write_serialized_report(
        input,
        output,
        &CommandReport::new(command, Payload::Ok(payload)),
    )
}

/// Writes a status-bearing command payload.
pub(super) fn write_payload_report<P: Serialize>(
    input: &Path,
    output: Option<&FileDestination>,
    command: &'static str,
    payload: Payload<'_, P>,
) -> Result<()> {
    write_serialized_report(input, output, &CommandReport::new(command, payload))
}

fn write_serialized_report(
    input: &Path,
    output: Option<&FileDestination>,
    report: &impl Serialize,
) -> Result<()> {
    let Some(output) = output else {
        return Ok(());
    };
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    output.write(input, &bytes)?;
    eprintln!("wrote report {}", output.path.display());
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
    fn fidelity_summary_serializes_the_diff_shape_without_value_glue() {
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
            }))
            .unwrap(),
            serde_json::json!({
                "present": "both",
                "different": true,
                "diff": {
                    "annotations_changed": true,
                    "retained_records_changed": false
                }
            })
        );
    }
}
