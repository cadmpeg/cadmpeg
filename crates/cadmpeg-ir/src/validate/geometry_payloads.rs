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

#[cfg(test)]
mod tests;
