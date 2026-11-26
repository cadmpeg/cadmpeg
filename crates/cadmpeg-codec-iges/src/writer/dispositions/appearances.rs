//! What the semantic writer does with each appearance and appearance binding
//! (cadir-v11.md §3.4 and the Appearances paragraph of §8): a record is
//! written when the written file states it, and every other record is charged
//! by id with its reason.
//!
//! A written Directory entry states the colour of the body or the face that
//! its entity carries. A binding of the whole of a body or a face is written
//! when every entity that carries it states the colour of its appearance. An
//! appearance is written when a written binding names it, it states only a
//! colour and a name, and a written Type 314 states its name.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::Presented;
use crate::loss::IgesLossCode;
use crate::writer::Synthesis;

const BINDING_PATH: &str = "that bind through an occurrence path, which IGES does not state";
pub(super) const BINDING_TARGET: &str =
    "that bind a record other than a body or a face, whose Directory \
                              colour the semantic writer writes as 0";
const BINDING_UNWRITTEN: &str = "whose body or face no written entity carries";
const BINDING_FIELDS: &str = "that state a precedence, channels, a source identity, a \
                              visibility, or an object type that IGES does not state";
const BINDING_COLOR: &str =
    "whose body or face a written Directory entry states with another colour";
pub(super) const APPEARANCE_UNUSED: &str = "that no written body or face states with its colour";
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
        let mut bound = BTreeSet::new();
        for binding in &ir.model.appearance_bindings {
            let appearance = appearances.get(binding.appearance.as_str()).copied();
            match binding_drop(binding, appearance, &synthesis.presented) {
                Some(reason) => {
                    drops
                        .bindings
                        .insert(binding.id.as_str().to_owned(), reason);
                }
                None => {
                    bound.insert(binding.appearance.as_str());
                }
            }
        }
        let named = synthesis
            .entities
            .iter()
            .filter(|entity| entity.type_code == 314)
            .flat_map(|entity| entity.restates.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        for appearance in &ir.model.appearances {
            let id = appearance.id.as_str();
            let reason = if !bound.contains(id) {
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
    let (object_type, carrying) = match &binding.target.entity {
        AppearanceTarget::Body(id) => (
            "Body",
            presented
                .values()
                .filter(|presented| presented.body.as_ref() == Some(id))
                .collect::<Vec<_>>(),
        ),
        AppearanceTarget::Face(id) => (
            "Face",
            presented
                .values()
                .filter(|presented| presented.face.as_ref() == Some(id))
                .collect(),
        ),
        _ => return Some(BINDING_TARGET),
    };
    if carrying.is_empty() {
        return Some(BINDING_UNWRITTEN);
    }
    if binding.precedence.is_some()
        || !binding.channels.is_empty()
        || binding.source_entity_id.is_some()
        || binding.visible.is_some()
        || binding
            .object_type
            .as_deref()
            .is_some_and(|stated| stated != object_type)
    {
        return Some(BINDING_FIELDS);
    }
    let color = appearance.and_then(|appearance| appearance.base_color);
    if color.is_none() || carrying.iter().any(|presented| presented.color != color) {
        return Some(BINDING_COLOR);
    }
    None
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
