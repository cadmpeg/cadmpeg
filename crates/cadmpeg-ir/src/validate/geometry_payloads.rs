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
            .stated_body_regions(&body.id)
            .flat_map(crate::topology::Region::stated_shells)
            .flat_map(|shell| ir.geometry.stated_shell_faces(shell))
        {
            face_bodies.insert(face.id.as_str(), body.id.as_str());
        }
    }
    let assets = crate::index::identities::BorrowedIdentities::build(|add| {
        for asset in &ir.model.assets {
            add(asset.id.as_str(), ());
        }
    });
    let definitions: std::collections::BTreeMap<&str, &crate::products::ProductDefinition> = ir
        .model
        .product_definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect();
    let pmi: std::collections::BTreeMap<&str, &crate::pmi::PmiAnnotation> = ir
        .model
        .pmi
        .iter()
        .map(|annotation| (annotation.id.as_str(), annotation))
        .collect();
    for mesh in &ir.model.tessellations {
        if let Some(message) = meshed_error(mesh, &bodies, &faces, &face_bodies, &definitions, &pmi)
        {
            super::record_finding(
                findings,
                Check::Tessellation,
                Severity::Error,
                Some(mesh.id.as_str()),
                crate::rule!("geometry_payloads.tessellation-targets", "{message}"),
            );
        }
        if let Some(message) = triangle_group_face_error(mesh, &face_bodies) {
            super::record_finding(
                findings,
                Check::Tessellation,
                Severity::Error,
                Some(mesh.id.as_str()),
                crate::rule!(
                    "geometry_payloads.tessellation-triangle-group-faces",
                    "{message}"
                ),
            );
        }
        for assignment in mesh.texture_assignments() {
            if !assets.contains(assignment.texture.as_str()) {
                super::record_finding(
                    findings,
                    Check::Tessellation,
                    Severity::Error,
                    Some(mesh.id.as_str()),
                    crate::rule!(
                        "geometry_payloads.references-missing-tessellation-texture-asset",
                        "references a missing tessellation texture asset"
                    ),
                );
                break;
            }
        }
    }
}

/// The first rule of part 10 that what a tessellation is breaks: a `Body`
/// mesh names a body and faces of it; a `Display` or `Definition` mesh names a
/// definition whose storage is not `External`; a `Pmi` mesh names a
/// presentation annotation; an `Unresolved` mesh states no `state`.
fn meshed_error(
    mesh: &crate::tessellation::Tessellation,
    bodies: &crate::index::identities::BorrowedIdentities<'_, ()>,
    faces: &crate::index::identities::BorrowedIdentities<'_, ()>,
    face_bodies: &std::collections::BTreeMap<&str, &str>,
    definitions: &std::collections::BTreeMap<&str, &crate::products::ProductDefinition>,
    pmi: &std::collections::BTreeMap<&str, &crate::pmi::PmiAnnotation>,
) -> Option<&'static str> {
    use crate::tessellation::Meshed;
    match &mesh.of {
        Meshed::Body {
            body,
            faces: covered,
        } => {
            if !bodies.contains(body.as_str()) {
                return Some("references a missing tessellation body");
            }
            if covered.iter().any(|face| !faces.contains(face.as_str())) {
                return Some("references a missing tessellation face");
            }
            covered
                .iter()
                .any(|face| face_bodies.get(face.as_str()) != Some(&body.as_str()))
                .then_some("covers a face outside the tessellation body")
        }
        Meshed::Display { definition } | Meshed::Definition { definition, .. } => {
            match definitions.get(definition.as_str()) {
                None => Some("references a missing tessellation product definition"),
                Some(found) if matches!(found.storage, crate::products::Storage::External(_)) => {
                    Some("references an external tessellation product definition")
                }
                Some(_) => None,
            }
        }
        Meshed::Pmi { pmi: annotation } => match pmi.get(annotation.as_str()) {
            None => Some("references a missing tessellation PMI annotation"),
            Some(found)
                if !matches!(
                    found.definition,
                    crate::pmi::PmiDefinition::Presentation { .. }
                ) =>
            {
                Some("references a tessellation PMI annotation that is not a presentation")
            }
            Some(_) => None,
        },
        Meshed::Unresolved => mesh
            .state
            .is_some()
            .then_some("states a state on an unresolved tessellation"),
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
        let Some(body) = mesh.of.body() else {
            return Some("has a triangle group face and no tessellation body");
        };
        if face_bodies.get(face.as_str()) != Some(&body.as_str()) {
            return Some("has a triangle group face outside the tessellation body");
        }
        if !mesh.of.faces().is_empty() && !mesh.of.faces().contains(face) {
            return Some("has a triangle group face outside the tessellation faces");
        }
    }
    None
}

#[cfg(test)]
mod tests;
