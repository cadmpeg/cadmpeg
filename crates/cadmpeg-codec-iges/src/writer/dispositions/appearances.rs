//! What the semantic writer does with each appearance and appearance binding
//! (cadir-v11.md §3.4 and the Appearances paragraph of §8): a record is
//! written when the written file states it, and every other record is charged
//! by id with its reason.
//!
//! A written Directory entry states the colour of the body or the face that
//! its entity carries. A binding of the whole of a body or a face is written
//! when every entity that carries it states the colour of its appearance; its
//! source identity, object type, and channels are the source's names for the
//! target that the written entity restates. An appearance is written when a
//! written entity that carries a body or a face that one of its bindings
//! names states its colour, it states only a colour and a name, and a written
//! Type 314 states its name.
//!
//! A Type 143 or 144 has one Directory colour field for its sheet body and
//! its face. A body whose faces it writes with another colour is charged
//! once, unless the charge of a binding of the whole body to an appearance of
//! the body colour states that loss.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::Presented;
use crate::loss::IgesLossCode;
use crate::writer::unjoined::source_body;
use crate::writer::Synthesis;

const BINDING_PATH: &str = "that bind through an occurrence path, which IGES does not state";
const BINDING_UNRESOLVED: &str =
    "whose target is no record of the document, which the semantic writer does not apply";
pub(super) const BINDING_TARGET: &str =
    "that bind a record other than a body or a face, whose colour the semantic writer does \
     not write";
const BINDING_UNWRITTEN: &str = "whose body or face no written entity carries";
const BINDING_FIELDS: &str = "that state a precedence or a visibility that IGES does not state";
const BINDING_NO_COLOR: &str = "whose appearance states no colour";
const BINDING_COLOR: &str = "whose body or face a written Directory entry states with a colour \
                             other than that of the appearance";
pub(super) const APPEARANCE_UNUSED: &str =
    "whose colour no written body or face that one of their bindings names states";
const APPEARANCE_CONTENT: &str = "that state an identity, a category, a schema, properties, or \
                                  textures that IGES does not state";
const APPEARANCE_NAME: &str = "whose name no written colour definition states";

/// The schema of the appearances the IGES decoder states.
const IGES_SCHEMA: &str = "IGES color";

/// The appearances and the appearance bindings the written file does not
/// state, each with its reason, by id.
#[derive(Default)]
pub(in crate::writer) struct AppearanceDrops {
    pub(in crate::writer) appearances: BTreeMap<String, &'static str>,
    pub(in crate::writer) bindings: BTreeMap<String, &'static str>,
    /// Each body whose colour no written entity of its faces states, with
    /// those faces.
    body_colors: BTreeMap<String, BTreeSet<String>>,
}

impl AppearanceDrops {
    pub(in crate::writer) fn of(ir: &CadIr, synthesis: &Synthesis) -> Self {
        let appearances = ir
            .model
            .appearances
            .iter()
            .map(|appearance| (appearance.id.as_str(), appearance))
            .collect::<BTreeMap<_, _>>();
        let mut drops = Self::default();
        for binding in &ir.model.appearance_bindings {
            let appearance = appearances.get(binding.appearance.as_str()).copied();
            if let Some(reason) = binding_drop(binding, appearance, &synthesis.presented) {
                drops
                    .bindings
                    .insert(binding.id.as_str().to_owned(), reason);
            }
        }
        let stated = ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| binding.target.path.is_none())
            .filter(|binding| {
                let color = appearances
                    .get(binding.appearance.as_str())
                    .and_then(|appearance| appearance.base_color);
                color.is_some()
                    && carriers(&binding.target.entity, &synthesis.presented)
                        .is_some_and(|carrying| carrying.iter().any(|p| p.color == color))
            })
            .map(|binding| binding.appearance.as_str())
            .collect::<BTreeSet<_>>();
        let named = synthesis
            .entities
            .iter()
            .filter(|entity| entity.type_code == 314)
            .flat_map(|entity| entity.restates.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        for appearance in &ir.model.appearances {
            let id = appearance.id.as_str();
            let reason = if !stated.contains(id) {
                APPEARANCE_UNUSED
            } else if states_more_than_a_colour(appearance) {
                APPEARANCE_CONTENT
            } else if appearance.name.is_some() && !named.contains(id) {
                APPEARANCE_NAME
            } else {
                continue;
            };
            drops.appearances.insert(id.to_owned(), reason);
        }
        drops.body_colors = body_colors(ir, synthesis, &drops.bindings);
        drops
    }

    /// One loss for each arena and reason, naming every record it charges.
    pub(in crate::writer) fn losses(&self) -> Vec<LossNote> {
        let mut losses = Vec::new();
        for (body, faces) in &self.body_colors {
            losses.push(IgesLossCode::WriterBodyColorNotRepresented.note(format!(
                "IGES body {body} colour is not written on the entities of faces {}, whose one \
                 Directory colour field states the face colour",
                faces.iter().map(String::as_str).collect::<Vec<_>>().join(", ")
            )));
        }
        for (arena, drops) in [
            ("appearances", &self.appearances),
            ("appearance_bindings", &self.bindings),
        ] {
            let mut by_reason = BTreeMap::<&str, Vec<&str>>::new();
            for (id, reason) in drops {
                by_reason.entry(reason).or_default().push(id);
            }
            for (reason, ids) in by_reason {
                losses.push(IgesLossCode::WriterAppearanceNotRepresented.note(format!(
                    "the IGES write does not state {} {arena} record(s) {reason}: {}",
                    ids.len(),
                    ids.join(", ")
                )));
            }
        }
        losses
    }
}

/// Why the written file does not state `binding`, or `None` when it does.
fn binding_drop(
    binding: &AppearanceBinding,
    appearance: Option<&Appearance>,
    presented: &BTreeMap<usize, Presented>,
) -> Option<&'static str> {
    if binding.target.path.is_some() {
        return Some(BINDING_PATH);
    }
    if binding.target.entity == AppearanceTarget::Unresolved {
        return Some(BINDING_UNRESOLVED);
    }
    let Some(carrying) = carriers(&binding.target.entity, presented) else {
        return Some(BINDING_TARGET);
    };
    if carrying.is_empty() {
        return Some(BINDING_UNWRITTEN);
    }
    if binding.precedence.is_some() || binding.visible.is_some() {
        return Some(BINDING_FIELDS);
    }
    let Some(color) = appearance.and_then(|appearance| appearance.base_color) else {
        return Some(BINDING_NO_COLOR);
    };
    if carrying
        .iter()
        .any(|presented| presented.color != Some(color))
    {
        return Some(BINDING_COLOR);
    }
    None
}

/// The written entities that carry `target` when it is a body or a face.
fn carriers<'p>(
    target: &AppearanceTarget,
    presented: &'p BTreeMap<usize, Presented>,
) -> Option<Vec<&'p Presented>> {
    let carries = |presented: &Presented| match target {
        AppearanceTarget::Body(id) => presented
            .body
            .as_ref()
            .is_some_and(|body| source_body(body.as_str()) == id.as_str()),
        AppearanceTarget::Face(id) => presented.face.as_ref() == Some(id),
        _ => false,
    };
    matches!(
        target,
        AppearanceTarget::Body(_) | AppearanceTarget::Face(_)
    )
    .then(|| presented.values().filter(|p| carries(p)).collect())
}

/// Each body with a colour whose faces a Type 143 or 144 writes with another
/// colour, with those faces, less each body that a charged binding of the
/// whole body to an appearance of the body colour names.
fn body_colors(
    ir: &CadIr,
    synthesis: &Synthesis,
    charged: &BTreeMap<String, &'static str>,
) -> BTreeMap<String, BTreeSet<String>> {
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
    let mut unwritten = BTreeMap::<String, BTreeSet<String>>::new();
    for presented in synthesis.presented.values() {
        let (Some(body), Some(face)) = (&presented.body, &presented.face) else {
            continue;
        };
        let (Some(body), Some(face)) = (
            bodies.get(source_body(body.as_str())),
            faces.get(face.as_str()),
        ) else {
            continue;
        };
        if face.color.is_some() && body.color.is_some_and(|color| face.color != Some(color)) {
            unwritten
                .entry(body.id.as_str().to_owned())
                .or_default()
                .insert(face.id.as_str().to_owned());
        }
    }
    let appearances = ir
        .model
        .appearances
        .iter()
        .map(|appearance| (appearance.id.as_str(), appearance))
        .collect::<BTreeMap<_, _>>();
    for binding in &ir.model.appearance_bindings {
        let AppearanceTarget::Body(body) = &binding.target.entity else {
            continue;
        };
        let names_body_color = binding.target.path.is_none()
            && charged.contains_key(binding.id.as_str())
            && bodies.get(body.as_str()).is_some_and(|body| {
                body.color.is_some()
                    && appearances
                        .get(binding.appearance.as_str())
                        .is_some_and(|appearance| appearance.base_color == body.color)
            });
        if names_body_color {
            unwritten.remove(body.as_str());
        }
    }
    unwritten
}

/// Whether `appearance` states more than a colour and a name.
fn states_more_than_a_colour(appearance: &Appearance) -> bool {
    appearance.asset_guid.is_some()
        || appearance.library_id.is_some()
        || appearance.visual_guid.is_some()
        || appearance.physical_token.is_some()
        || appearance.category.is_some()
        || !appearance.properties.is_empty()
        || !appearance.textures.is_empty()
        || appearance
            .schema
            .as_deref()
            .is_some_and(|schema| schema != IGES_SCHEMA)
}
