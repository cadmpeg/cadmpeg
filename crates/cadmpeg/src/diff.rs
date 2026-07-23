// SPDX-License-Identifier: Apache-2.0
//! Structural comparison of two decoded models and their source-fidelity sidecars.

use std::path::Path;

use anyhow::Result;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::SourceFidelity;

use crate::commands::semantic_silent;
use crate::envelope::{envelope, print_json, ReportSink};
use crate::format::ForcedInput;
use crate::loader;
use crate::registry::Registry;

/// One positional input to compare: its path and an optional forced reader.
#[derive(Clone, Copy)]
pub struct DiffInput<'a> {
    /// Model file to load.
    pub path: &'a Path,
    /// Explicit input format for this input, bypassing content detection.
    pub forced: Option<ForcedInput>,
}

/// Structurally compare two decoded models.
///
/// Returns `Ok(())` when the models match and a silent `SemanticFailure` when
/// they differ, so a non-empty diff exits 1 with the comparison on stdout and no
/// error line, while operational errors still exit 2.
pub fn diff(
    registry: &Registry,
    a: DiffInput<'_>,
    b: DiffInput<'_>,
    options: DecodeOptions,
    json: bool,
    report: Option<&Path>,
) -> Result<()> {
    let left = loader::load_ir(registry, a.path, options, a.forced)?;
    let right = loader::load_ir(registry, b.path, options, b.forced)?;
    let result = cadmpeg_ir::diff(&left.ir, &right.ir);
    let fidelity = fidelity_diff(
        left.source_fidelity.as_ref(),
        right.source_fidelity.as_ref(),
    );
    let different = !result.is_empty() || fidelity_differs(&fidelity);
    if json || report.is_some() {
        let payload = serde_json::json!({
            "different": different,
            "diff": serde_json::to_value(&result)?,
            "source_fidelity": fidelity_json(&fidelity),
        });
        let sink = ReportSink {
            input: a.path,
            output: report,
            force: false,
            command: "diff",
        };
        if json {
            sink.write_payload(payload.clone())?;
            print_json(&envelope("diff", payload))?;
            return diff_status(different);
        }
        sink.write_payload(payload)?;
    }
    println!("diff {} vs {}", a.path.display(), b.path.display());
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
    if !different {
        println!("  identical");
    }
    diff_status(different)
}

/// A non-empty diff is a model-level result: exit 1 with no error line.
fn diff_status(different: bool) -> Result<()> {
    if different {
        Err(semantic_silent())
    } else {
        Ok(())
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
