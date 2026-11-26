//! The body names and the colour opacities that the entities which carry a
//! body or a face do not state, each charged once by the record that holds
//! it, however many written entities carry that record.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::writer::Synthesis;

/// One loss for each written body whose name a Type 406 Form 15 cannot
/// state, and one for each body and face whose colour a written Directory
/// entry states without its opacity.
pub(in crate::writer) fn losses(ir: &CadIr, synthesis: &Synthesis) -> Vec<LossNote> {
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
    let mut named = BTreeSet::new();
    let mut body_colors = BTreeSet::new();
    let mut face_colors = BTreeSet::new();
    for presented in synthesis.presented.values() {
        let body = presented
            .body
            .as_ref()
            .and_then(|body| bodies.get(synthesis.source_body(body).as_str()).copied());
        let face = presented
            .face
            .as_ref()
            .and_then(|face| faces.get(face.as_str()).copied());
        named.extend(body.map(|body| body.id.as_str()));
        if let Some(face) = face.filter(|face| face.color.is_some()) {
            face_colors.insert(face.id.as_str());
        } else if let Some(body) = body {
            body_colors.insert(body.id.as_str());
        }
    }
    let mut losses = Vec::new();
    for body in named.iter().filter_map(|id| bodies.get(id)) {
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
    }
    for body in body_colors.iter().filter_map(|id| bodies.get(id)) {
        if let Some(alpha) = body.color.map(Color::a).filter(|alpha| *alpha != 1.0) {
            losses.push(IgesLossCode::WriterBodyOpacityNotRepresented.note(format!(
                "IGES body {} opacity {alpha} has no Directory representation",
                body.id
            )));
        }
    }
    for face in face_colors.iter().filter_map(|id| faces.get(id)) {
        if let Some(alpha) = face.color.map(Color::a).filter(|alpha| *alpha != 1.0) {
            losses.push(IgesLossCode::WriterFaceOpacityNotRepresented.note(format!(
                "IGES face {} opacity {alpha} has no Directory representation",
                face.id
            )));
        }
    }
    losses
}
