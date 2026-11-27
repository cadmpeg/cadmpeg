//! What the semantic writer does with each appearance and appearance binding
//! (cadir-v11.md §3.4 and the Appearances paragraph of §8): a record is
//! written when the written file states it, and every other record is charged
//! by id with its reason.
//!
//! A written Directory entry states the red, green, and blue of the colour of
//! the body or the face that its entity carries, and no opacity. A binding of
//! the whole of a body or a face is written when every entity that carries it
//! states the red, green, and blue of its appearance; its source identity,
//! object type, and channels are the source's names for the target that the
//! written entity restates. An appearance is written when a written entity
//! that carries a body or a face that one of its bindings names states its
//! red, green, and blue, it states only an opaque colour and a name, and a
//! written Type 314 states its name.
//!
//! An appearance that loses several facts is charged once with every reason.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::Presented;
use crate::loss::IgesLossCode;
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
pub(super) const BINDING_COLOR: &str =
    "whose body or face a written Directory entry states with a colour \
                             other than that of the appearance";
pub(super) const APPEARANCE_UNUSED: &str =
    "whose colour no written body or face that one of their bindings names states";
const APPEARANCE_CONTENT: &str = "that state an identity, a category, a schema, properties, or \
                                  textures that IGES does not state";
const APPEARANCE_OPACITY: &str =
    "whose colour states an opacity, which a Directory colour does not state";
const APPEARANCE_NAME: &str = "whose name no written colour definition states";

/// The schema of the appearances the IGES decoder states.
const IGES_SCHEMA: &str = "IGES color";

/// The appearances and the appearance bindings the written file does not
/// state, each with its reasons, by id.
#[derive(Default)]
pub(in crate::writer) struct AppearanceDrops {
    pub(in crate::writer) appearances: BTreeMap<String, String>,
    pub(in crate::writer) bindings: BTreeMap<String, String>,
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
            if let Some(reason) = binding_drop(binding, appearance, synthesis) {
                drops
                    .bindings
                    .insert(binding.id.as_str().to_owned(), reason.to_owned());
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
                    .and_then(|appearance| appearance.base_color)
                    .and_then(super::opaque);
                color.is_some()
                    && carriers(&binding.target.entity, synthesis)
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
            let reasons = if stated.contains(id) {
                [
                    (states_more_than_a_colour(appearance), APPEARANCE_CONTENT),
                    (
                        appearance.base_color.is_some_and(|color| color.a() != 1.0),
                        APPEARANCE_OPACITY,
                    ),
                    (
                        appearance.name.is_some() && !named.contains(id),
                        APPEARANCE_NAME,
                    ),
                ]
                .into_iter()
                .filter_map(|(lost, reason)| lost.then_some(reason))
                .collect::<Vec<_>>()
            } else {
                vec![APPEARANCE_UNUSED]
            };
            if !reasons.is_empty() {
                drops
                    .appearances
                    .insert(id.to_owned(), reasons.join(", and "));
            }
        }
        drops
    }

    /// One loss for each arena and reason, naming every record it charges.
    pub(in crate::writer) fn losses(&self) -> Vec<LossNote> {
        let mut losses = Vec::new();
        for (arena, drops) in [
            ("appearances", &self.appearances),
            ("appearance_bindings", &self.bindings),
        ] {
            let mut by_reason = BTreeMap::<&str, Vec<&str>>::new();
            for (id, reason) in drops {
                by_reason.entry(reason.as_str()).or_default().push(id);
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
    synthesis: &Synthesis,
) -> Option<&'static str> {
    if binding.target.path.is_some() {
        return Some(BINDING_PATH);
    }
    if binding.target.entity == AppearanceTarget::Unresolved {
        return Some(BINDING_UNRESOLVED);
    }
    let Some(carrying) = carriers(&binding.target.entity, synthesis) else {
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
        .any(|presented| presented.color != super::opaque(color))
    {
        return Some(BINDING_COLOR);
    }
    None
}

/// The written entities that carry `target` when it is a body or a face.
fn carriers<'s>(target: &AppearanceTarget, synthesis: &'s Synthesis) -> Option<Vec<&'s Presented>> {
    let carries = |presented: &Presented| match target {
        AppearanceTarget::Body(id) => presented
            .body
            .as_ref()
            .is_some_and(|body| synthesis.source_body(body) == id),
        AppearanceTarget::Face(id) => presented.face.as_ref() == Some(id),
        _ => false,
    };
    matches!(
        target,
        AppearanceTarget::Body(_) | AppearanceTarget::Face(_)
    )
    .then(|| {
        synthesis
            .presented
            .values()
            .filter(|p| carries(p))
            .collect()
    })
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
