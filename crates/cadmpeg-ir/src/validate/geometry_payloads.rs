// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for geometry payloads.

use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

pub(super) fn check_tessellations(
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    let bodies = crate::index::identities::BorrowedIdentities::build(|add| {
        for body in &ir.model.bodies {
            add(body.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let faces = crate::index::identities::BorrowedIdentities::build(|add| {
        for face in &ir.model.faces {
            add(face.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let assets = crate::index::identities::BorrowedIdentities::build(|add| {
        for asset in &ir.model.assets {
            add(asset.id.as_str(), ())?;
        }
        Ok(())
    })?;
    for mesh in &ir.model.tessellations {
        if let Some(body) = &mesh.body {
            if !bodies.contains(body.as_str()) {
                super::record_finding(
                    findings,
                    Check::Tessellation,
                    Severity::Error,
                    Some(mesh.id.as_str()),
                    format_args!("references a missing tessellation body"),
                );
            }
        }
        for face in &mesh.faces {
            if !faces.contains(face.as_str()) {
                super::record_finding(
                    findings,
                    Check::Tessellation,
                    Severity::Error,
                    Some(mesh.id.as_str()),
                    format_args!("references a missing tessellation face"),
                );
                break;
            }
        }
        for assignment in mesh.texture_assignments() {
            if !assets.contains(assignment.texture.as_str()) {
                super::record_finding(
                    findings,
                    Check::Tessellation,
                    Severity::Error,
                    Some(mesh.id.as_str()),
                    format_args!("references a missing tessellation texture asset"),
                );
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
