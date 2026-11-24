// SPDX-License-Identifier: Apache-2.0
//! Caches of a toolpath that in-IR arithmetic decides (part 10, Caches),
//! where the toolpath's status is `Current`: its frame against its
//! operation's resolved frame (D55), and each cycle's depth against its stock
//! level minus the z of its point (§7.8). Each disagreement is a Warning.

use super::records::ProcessRecords;
use super::warning;
use crate::design::evaluation::EvaluationStatus;
use crate::process::cycle::{Cycle, CycleKind};
use crate::process::toolpath::{Motion, Toolpath, ToolpathContent};
use crate::report::check::Finding;
use crate::transform::Transform;

pub(super) fn check_caches(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let tolerances = records.ir.tolerances;
    let (linear, angular) = (tolerances.linear.get(), tolerances.angular.get());
    for toolpath in &records.ir.process.toolpaths {
        if toolpath.status != EvaluationStatus::Current {
            continue;
        }
        let ToolpathContent::Motions { frame, motions } = &toolpath.content else {
            continue;
        };
        if let Some(resolved) = resolved_frame(records, toolpath) {
            if !frame.agrees_with(resolved, linear, angular) {
                warning(
                    findings,
                    toolpath.id.as_str(),
                    format_args!(
                        "the toolpath's frame is not its operation's resolved frame {:?}",
                        resolved.affine_rows()
                    ),
                );
            }
        }
        for motion in motions {
            if let Motion::Cycle(cycle) = motion {
                check_cycle_depth(toolpath, cycle, linear, findings);
            }
        }
    }
}

/// The operation's resolved frame: its `tool_frame`, else its setup's WCS,
/// where that frame states its placement.
fn resolved_frame(records: &ProcessRecords<'_>, toolpath: &Toolpath) -> Option<Transform> {
    let operation = records.operations.get(toolpath.operation.as_str())?;
    let wcs = match &operation.tool_frame {
        Some(frame) => frame,
        None => {
            let setup = records.setup_of(operation.id.as_str())?;
            &records.setups.get(setup)?.wcs
        }
    };
    wcs.placement
}

/// A cycle's `depth` is its `stock` level minus the z of its point. A probing
/// cycle's point is its start point, which no depth measures from.
fn check_cycle_depth(toolpath: &Toolpath, cycle: &Cycle, linear: f64, findings: &mut Vec<Finding>) {
    if matches!(cycle.kind, CycleKind::Probe(_)) {
        return;
    }
    let (Some(stock), Some(depth)) = (cycle.stock, cycle.depth) else {
        return;
    };
    let expected = stock.get() - cycle.at.get().z;
    if (depth.get() - expected).abs() > linear {
        warning(
            findings,
            toolpath.id.as_str(),
            format_args!(
                "a cycle's depth {} is not its stock level {} minus the z of its point, {expected}",
                depth.get(),
                stock.get()
            ),
        );
    }
}

#[cfg(test)]
mod tests;
