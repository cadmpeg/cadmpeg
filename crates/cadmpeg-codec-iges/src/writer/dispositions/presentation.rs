//! The names and the colours of the bodies and the faces that the written
//! entities which carry them do not state, each charged once by the record
//! that holds it, however many written entities carry that record.
//!
//! A Type 143 or 144 has one Directory colour field for its sheet body and
//! its face. A body whose faces it writes with another colour is charged
//! once, with its opacity, unless a binding of the whole body to an
//! appearance of the body colour is charged for the colour its entities
//! state. Every other written body and every face whose colour a written
//! entity states is charged once for an alpha other than 1.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;

use super::appearances::{AppearanceDrops, BINDING_COLOR};
use crate::loss::IgesLossCode;
use crate::writer::Synthesis;

/// One loss for each written body whose name a Type 406 Form 15 cannot
/// state, for each body whose colour the entities of its faces do not state,
/// and for each other body and each face whose colour a written Directory
/// entry states without its opacity.
pub(in crate::writer) fn losses(
    ir: &CadIr,
    synthesis: &Synthesis,
    appearances: &AppearanceDrops,
) -> Vec<LossNote> {
    let bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    let faces = ir
        .geometry
        .faces
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    let mut written = BTreeSet::new();
    let mut face_colors = BTreeSet::new();
    let mut recolored = BTreeMap::<&str, BTreeSet<&str>>::new();
    for presented in synthesis.presented.values() {
        let body = presented
            .body
            .as_ref()
            .and_then(|body| bodies.get(synthesis.source_body(body).as_str()).copied());
        let face = presented
            .face
            .as_ref()
            .and_then(|face| faces.get(face.as_str()).copied());
        written.extend(body.map(|body| body.id.as_str()));
        let Some(face) = face.filter(|face| face.color.is_some()) else {
            continue;
        };
        face_colors.insert(face.id.as_str());
        if let Some(body) = body.filter(|body| body.color.is_some_and(|c| face.color != Some(c))) {
            recolored
                .entry(body.id.as_str())
                .or_default()
                .insert(face.id.as_str());
        }
    }
    for binding in &ir.model.appearance_bindings {
        let AppearanceTarget::Body(body) = &binding.target.entity else {
            continue;
        };
        let names_body_color = binding.target.path.is_none()
            && appearances
                .bindings
                .get(binding.id.as_str())
                .map(String::as_str)
                == Some(BINDING_COLOR)
            && bodies.get(body.as_str()).is_some_and(|body| {
                body.color.is_some()
                    && ir
                        .model
                        .appearances
                        .iter()
                        .find(|appearance| appearance.id == binding.appearance)
                        .is_some_and(|appearance| appearance.base_color == body.color)
            });
        if names_body_color {
            recolored.remove(body.as_str());
        }
    }
    let opacity = |color: Option<Color>| color.map(Color::a).filter(|alpha| *alpha != 1.0);
    let mut losses = Vec::new();
    for body in written.iter().filter_map(|id| bodies.get(id)) {
        if let Some(name) = body
            .name
            .as_deref()
            .filter(|name| !crate::writer::writable_name(name))
        {
            losses.push(IgesLossCode::WriterBodyNameNotRepresented.note(format!(
                "IGES body {} name {name:?} cannot be encoded as a Type 406 Form 15 name",
                body.id
            )));
        }
        if let Some(faces) = recolored.get(body.id.as_str()) {
            let alpha = opacity(body.color).map_or_else(String::new, |alpha| {
                format!(", and its opacity {alpha} has no Directory representation")
            });
            losses.push(IgesLossCode::WriterBodyColorNotRepresented.note(format!(
                "IGES body {} colour is not written on the entities of faces {}, whose one \
                 Directory colour field states the face colour{alpha}",
                body.id,
                faces.iter().copied().collect::<Vec<_>>().join(", ")
            )));
        } else if let Some(alpha) = opacity(body.color) {
            losses.push(IgesLossCode::WriterBodyOpacityNotRepresented.note(format!(
                "IGES body {} opacity {alpha} has no Directory representation",
                body.id
            )));
        }
    }
    for face in face_colors.iter().filter_map(|id| faces.get(id)) {
        if let Some(alpha) = opacity(face.color) {
            losses.push(IgesLossCode::WriterFaceOpacityNotRepresented.note(format!(
                "IGES face {} opacity {alpha} has no Directory representation",
                face.id
            )));
        }
    }
    losses
}
