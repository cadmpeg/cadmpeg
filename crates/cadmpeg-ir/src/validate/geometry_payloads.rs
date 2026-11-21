// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for geometry payloads.

use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

pub(super) fn check_tessellations(ir: &CadIr, findings: &mut Vec<Finding>) {
    let bodies = crate::index::identities::BorrowedIdentities::build(|add| {
        for body in &ir.geometry.bodies {
            add(body.id.as_str(), ());
        }
    });
    let faces = crate::index::identities::BorrowedIdentities::build(|add| {
        for face in &ir.geometry.faces {
            add(face.id.as_str(), ());
        }
    });
    let mut face_bodies = std::collections::BTreeMap::new();
    for body in &ir.geometry.bodies {
        for face in ir
            .geometry
            .body_regions(&body.id)
            .flat_map(|region| region.shells.iter())
            .flat_map(|shell| ir.geometry.shell_faces(shell))
        {
            face_bodies.insert(face.id.as_str(), body.id.as_str());
        }
    }
    let assets = crate::index::identities::BorrowedIdentities::build(|add| {
        for asset in &ir.model.assets {
            add(asset.id.as_str(), ());
        }
    });
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
        if let Some(message) = triangle_group_face_error(mesh, &face_bodies) {
            super::record_finding(
                findings,
                Check::Tessellation,
                Severity::Error,
                Some(mesh.id.as_str()),
                format_args!("{message}"),
            );
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
}

/// The first rule that a triangle group's face breaks: a group names a face
/// of the tessellation's body, and one of its `faces` when it lists any.
fn triangle_group_face_error(
    mesh: &crate::tessellation::Tessellation,
    face_bodies: &std::collections::BTreeMap<&str, &str>,
) -> Option<&'static str> {
    for face in mesh
        .triangle_groups()
        .iter()
        .filter_map(|group| group.face.as_ref())
    {
        let Some(body) = &mesh.body else {
            return Some("has a triangle group face and no tessellation body");
        };
        if face_bodies.get(face.as_str()) != Some(&body.as_str()) {
            return Some("has a triangle group face outside the tessellation body");
        }
        if !mesh.faces.is_empty() && !mesh.faces.contains(face) {
            return Some("has a triangle group face outside the tessellation faces");
        }
    }
    None
}

#[cfg(test)]
mod tests;
