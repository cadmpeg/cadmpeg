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
                entity: Some(mesh.id.clone()),
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
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .chordal_deflection
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "has an invalid tessellation deflection".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .vertices()
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains a non-finite tessellation vertex".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .normals()
            .iter()
            .chain(mesh.corner_normals())
            .any(|normal| !normal.x.is_finite() || !normal.y.is_finite() || !normal.z.is_finite())
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains a non-finite tessellation normal".into(),
                entity: Some(mesh.id.clone()),
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
                entity: Some(mesh.id.clone()),
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
        if tolerance.is_some_and(nonpositive) {
            findings.push(Finding {
                check: Check::Tolerances,
                severity: Severity::Error,
                message: "topology tolerance is not positive and finite".into(),
                entity: Some(id.to_owned()),
            });
        } else if tolerance.is_some_and(|value| value > 1.0e6) {
            findings.push(Finding {
                check: Check::Tolerances,
                severity: Severity::Warning,
                message: "topology tolerance is outside a sane canonical range".into(),
                entity: Some(id.to_owned()),
            });
        }
    }
    for procedural in &ir.model.procedural_surfaces {
        if let ProceduralSurfaceDefinition::TSpline { construction } = procedural.definition() {
            if construction.subtransform.inline().is_none() {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "T-spline surface subtransform is unresolved",
                );
            }
        }
    }
}

pub(super) fn bounds_err(findings: &mut Vec<Finding>, id: &str, msg: &str) {
    findings.push(Finding {
        check: Check::Bounds,
        severity: Severity::Error,
        message: msg.to_string(),
        entity: Some(id.to_string()),
    });
}

#[cfg(test)]
mod tests;
