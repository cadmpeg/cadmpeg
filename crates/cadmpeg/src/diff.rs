// SPDX-License-Identifier: Apache-2.0
//! Structural comparison of two decoded models and their source-fidelity sidecars.

use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::SourceFidelity;

use crate::envelope::{envelope, print_json};
use crate::loader;
use crate::registry::Registry;

/// Structurally compare two decoded models.
pub fn diff(
    registry: &Registry,
    a: &Path,
    b: &Path,
    options: DecodeOptions,
    json: bool,
) -> Result<ExitCode> {
    let left = loader::load_ir(registry, a, options, None)?;
    let right = loader::load_ir(registry, b, options, None)?;
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
