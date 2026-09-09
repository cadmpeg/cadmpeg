// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for geometry payloads.
#![allow(clippy::wildcard_imports)]

use super::*;
pub(super) fn check_tessellations(ir: &CadIr, findings: &mut Vec<Finding>) {
    for mesh in &ir.model.tessellations {
        if mesh.body.as_ref().is_some_and(|body| {
            !ir.model
                .bodies
                .iter()
                .any(|candidate| candidate.id == *body)
        }) {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "references a missing tessellation body".into(),
                entity: Some(mesh.id.to_string()),
            });
        }
        if mesh
            .faces
            .iter()
            .any(|face| !ir.model.faces.iter().any(|candidate| candidate.id == *face))
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "references a missing tessellation face".into(),
                entity: Some(mesh.id.to_string()),
            });
        }
        if mesh.texture_assignments().iter().any(|assignment| {
            !ir.model
                .assets
                .iter()
                .any(|asset| asset.id == assignment.texture)
        }) {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "references a missing tessellation texture asset".into(),
                entity: Some(mesh.id.to_string()),
            });
        }
    }
}

pub(super) fn check_bounds(ir: &CadIr, findings: &mut Vec<Finding>) {
    for (id, tolerance) in ir
        .model
        .vertices
        .iter()
        .map(|entity| (entity.id.as_str(), entity.tolerance))
        .chain(
            ir.model
                .edges
                .iter()
                .map(|entity| (entity.id.as_str(), entity.tolerance)),
        )
        .chain(
            ir.model
                .faces
                .iter()
                .map(|entity| (entity.id.as_str(), entity.tolerance)),
        )
    {
        if tolerance.is_some_and(|value| value.get() > 1.0e6) {
            findings.push(Finding {
                check: Check::Tolerances,
                severity: Severity::Warning,
                message: "topology tolerance is outside a sane canonical range".into(),
                entity: Some(id.to_owned()),
            });
        }
    }
}

#[cfg(test)]
mod tests;
