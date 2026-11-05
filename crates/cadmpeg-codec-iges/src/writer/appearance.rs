// SPDX-License-Identifier: Apache-2.0
//! Appearance facts preserved by emitted IGES surface colors.

use super::{
    display_color, number, BodyColor, BodyPresentation, CadIr, Entity, FiniteReal, IgesLossCode,
    LossNote,
};
use std::collections::{BTreeMap, BTreeSet};

/// Report appearance facts for which no equivalent surface color was emitted.
pub(super) fn losses(
    ir: &CadIr,
    entities: &[Entity],
    presentations: &BTreeMap<usize, BodyPresentation>,
) -> Vec<LossNote> {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut written_bindings = BTreeSet::new();
    let mut written_appearances = BTreeSet::new();
    for binding in &ir.model.appearance_bindings {
        let AppearanceTarget::Surface(surface) = &binding.target else {
            continue;
        };
        if binding.source_entity_id.is_some()
            || binding.object_type.is_some()
            || binding.visible.is_some()
            || !binding.channels.is_empty()
            || ir
                .model
                .appearance_bindings
                .iter()
                .filter(|other| other.target == binding.target)
                .count()
                != 1
        {
            continue;
        }
        let Some(appearance) = ir
            .model
            .appearances
            .iter()
            .find(|appearance| appearance.id == binding.appearance)
        else {
            continue;
        };
        let Some(color) = appearance.base_color.filter(|color| color.a() == 1.0) else {
            continue;
        };
        if appearance.name.is_some()
            || appearance.asset_guid.is_some()
            || appearance.library_id.is_some()
            || appearance.visual_guid.is_some()
            || appearance.physical_token.is_some()
            || appearance.schema.is_some()
            || appearance.category.is_some()
            || !appearance.properties.is_empty()
            || !appearance.textures.is_empty()
        {
            continue;
        }
        let expected = display_color(Some(color), "surface", &mut Vec::new());
        let written = entities.iter().enumerate().any(|(index, entity)| {
            if !entity
                .source_targets
                .iter()
                .any(|id| id == surface.as_str())
            {
                return false;
            }
            let Some(presentation) = presentations.get(&index) else {
                return false;
            };
            match (expected, presentation.color) {
                (BodyColor::Standard(left), BodyColor::Standard(right)) => left == right,
                (BodyColor::Custom(color), BodyColor::Definition(index)) => {
                    let values = [color.r(), color.g(), color.b()].map(|v| {
                        number(FiniteReal::new(f64::from(v) * 100.0).expect("finite color"))
                    });
                    entities.get(index).is_some_and(|definition| {
                        definition.type_code == 314
                            && definition.parameter_body
                                == format!("{},{},{},;", values[0], values[1], values[2]).as_bytes()
                    })
                }
                _ => false,
            }
        });
        if written {
            written_bindings.insert(binding.id.as_str());
            written_appearances.insert(appearance.id.as_str());
        }
    }
    ir.model
        .appearance_bindings
        .iter()
        .filter(|binding| !written_bindings.contains(binding.id.as_str()))
        .map(|binding| {
            IgesLossCode::WriterArenaOmitted.note(format!(
                "IGES omits unsupported appearance_bindings record {}",
                binding.id
            ))
        })
        .chain(
            ir.model
                .appearances
                .iter()
                .filter(|appearance| !written_appearances.contains(appearance.id.as_str()))
                .map(|appearance| {
                    IgesLossCode::WriterArenaOmitted.note(format!(
                        "IGES omits unsupported appearances record {}",
                        appearance.id
                    ))
                }),
        )
        .collect()
}
