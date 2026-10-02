// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for geometry payloads.

use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

pub(super) fn check_tessellations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    let bodies = crate::index::identities::BorrowedIdentities::build(ctx, |add| {
        for body in &ir.model.bodies { add(body.id.as_str(), ())?; }
        Ok(())
    })?;
    let faces = crate::index::identities::BorrowedIdentities::build(ctx, |add| {
        for face in &ir.model.faces { add(face.id.as_str(), ())?; }
        Ok(())
    })?;
    let assets = crate::index::identities::BorrowedIdentities::build(ctx, |add| {
        for asset in &ir.model.assets { add(asset.id.as_str(), ())?; }
        Ok(())
    })?;
    for mesh in &ir.model.tessellations {
        ctx.charge_work(1, "tessellation reference row")?;
        if let Some(body) = &mesh.body {
            if !bodies.contains(ctx, body.as_str())? {
                super::record_finding(ctx, findings, Check::Tessellation, Severity::Error,
                    Some(mesh.id.as_str()), format_args!("references a missing tessellation body"))?;
            }
        }
        for face in &mesh.faces {
            ctx.charge_work(1, "tessellation face reference")?;
            if !faces.contains(ctx, face.as_str())? {
                super::record_finding(ctx, findings, Check::Tessellation, Severity::Error,
                    Some(mesh.id.as_str()), format_args!("references a missing tessellation face"))?;
                break;
            }
        }
        for assignment in mesh.texture_assignments() {
            ctx.charge_work(1, "tessellation texture reference")?;
            if !assets.contains(ctx, assignment.texture.as_str())? {
                super::record_finding(ctx, findings, Check::Tessellation, Severity::Error,
                    Some(mesh.id.as_str()), format_args!("references a missing tessellation texture asset"))?;
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
