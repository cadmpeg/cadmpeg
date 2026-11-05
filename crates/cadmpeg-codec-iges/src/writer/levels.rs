// SPDX-License-Identifier: Apache-2.0
//! IGES Directory levels derived from neutral presentation layers.

use super::{
    ArenaDisposition, BodyPresentation, CadIr, Entity, EntityStatus, IgesLossCode, LossNote,
};
use std::collections::{BTreeMap, BTreeSet};

fn layer_item_id(item: &cadmpeg_ir::presentation::PresentationItem) -> Option<String> {
    use cadmpeg_ir::presentation::PresentationItem;
    Some(match item {
        PresentationItem::Body { body } => body.to_string(),
        PresentationItem::Face { face } => face.to_string(),
        PresentationItem::Edge { edge } => edge.to_string(),
        PresentationItem::Vertex { vertex } => vertex.to_string(),
        PresentationItem::Point { point } => point.to_string(),
        PresentationItem::Curve { curve } => curve.to_string(),
        PresentationItem::Surface { surface } => surface.to_string(),
        PresentationItem::Product { product } => product.to_string(),
        PresentationItem::Occurrence { occurrence } => occurrence.to_string(),
        PresentationItem::Pmi { annotation } => annotation.to_string(),
        _ => return None,
    })
}

fn layer_memberships(ir: &CadIr) -> BTreeMap<String, BTreeSet<i64>> {
    let mut layer_of: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    for layer in &ir.model.presentation_layers {
        let Ok(number) = layer.name.parse::<i64>() else {
            continue;
        };
        if !(0..=99_999_999).contains(&number) || number.to_string() != layer.name {
            continue;
        }
        for item in &layer.items {
            if let Some(id) = layer_item_id(item) {
                layer_of.entry(id).or_default().insert(number);
            }
        }
    }
    layer_of
}

pub(super) fn presentation_layer_coverage(
    ir: &CadIr,
    entities: &[Entity],
) -> (ArenaDisposition, Vec<LossNote>) {
    let memberships = layer_memberships(ir);
    let definitions = entities
        .iter()
        .filter_map(definition_level_set)
        .collect::<BTreeSet<_>>();
    let mut missing = Vec::new();
    for layer in &ir.model.presentation_layers {
        let valid_number = layer.name.parse::<i64>().is_ok_and(|number| {
            (0..=99_999_999).contains(&number) && number.to_string() == layer.name
        });
        let represented = valid_number
            && layer.parent.is_none()
            && layer.description.is_none()
            && layer.visible.is_none()
            && !layer.items.is_empty()
            && layer.items.iter().all(|item| {
                layer_item_id(item).is_some_and(|id| {
                    entities.iter().any(|entity| {
                        if !entity.source_targets.contains(&id) {
                            return false;
                        }
                        let levels = entity_layer_numbers(entity, &memberships);
                        levels.len() <= 1 || definitions.contains(&levels)
                    })
                })
            });
        if !represented {
            missing.push(layer.id.to_string());
        }
    }
    let coverage = if missing.is_empty() {
        ArenaDisposition::Written
    } else if missing.len() == ir.model.presentation_layers.len() {
        ArenaDisposition::Dropped
    } else {
        ArenaDisposition::Reported
    };
    let losses = if missing.is_empty() {
        Vec::new()
    } else {
        vec![IgesLossCode::WriterArenaOmitted.note(format!(
            "IGES cannot fully encode presentation layers: {}",
            missing.join(", ")
        ))]
    };
    (coverage, losses)
}

fn entity_layer_numbers(
    entity: &Entity,
    memberships: &BTreeMap<String, BTreeSet<i64>>,
) -> BTreeSet<i64> {
    entity
        .source_targets
        .iter()
        .filter_map(|target| memberships.get(target))
        .flatten()
        .copied()
        .collect()
}

fn definition_level_set(entity: &Entity) -> Option<BTreeSet<i64>> {
    if entity.type_code != 406 || entity.form != 1 {
        return None;
    }
    let text = std::str::from_utf8(&entity.parameter_body)
        .ok()?
        .strip_suffix(';')?;
    let mut tokens = text.split(',');
    let count = tokens.next()?.parse::<usize>().ok()?;
    let values = tokens
        .map(str::parse::<i64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (values.len() == count).then(|| values.into_iter().collect())
}

pub(super) fn append_level_definitions(ir: &CadIr, entities: &mut Vec<Entity>) {
    let memberships = layer_memberships(ir);
    let mut sets = entities
        .iter()
        .filter_map(definition_level_set)
        .collect::<BTreeSet<_>>();
    let wanted = entities
        .iter()
        .map(|entity| entity_layer_numbers(entity, &memberships))
        .filter(|levels| levels.len() > 1)
        .collect::<BTreeSet<_>>();
    for levels in wanted {
        if !sets.insert(levels.clone()) {
            continue;
        }
        let body = format!(
            "{},{};",
            levels.len(),
            levels
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        entities.push(Entity {
            structure: None,
            property_links: Vec::new(),
            association_links: Vec::new(),
            line_font: None,
            source_targets: Vec::new(),
            type_code: 406,
            form: 1,
            label: String::new(),
            status: EntityStatus::PhysicallyDependent,
            subscript: 0,
            parameter_body: body.into_bytes(),
            transform: None,
            transform_link: None,
        });
    }
}

/// Resolve neutral layer memberships to Directory levels and definition sets.
pub(super) fn directory_levels(
    ir: &CadIr,
    entities: &[Entity],
    expanded: &[(Entity, u32, Option<&BodyPresentation>)],
    expanded_index_by_entity: &[usize],
) -> Vec<String> {
    let layer_of = layer_memberships(ir);
    let defined_sets = entities
        .iter()
        .enumerate()
        .filter_map(|(index, entity)| definition_level_set(entity).map(|levels| (index, levels)))
        .collect::<Vec<_>>();
    // One slot per expanded directory entry: expanded indices address
    // this table, so it shares the expanded graph's length.
    let mut levels = vec!["0".to_owned(); expanded.len()];
    for (index, entity) in entities.iter().enumerate() {
        let Some(expanded) = expanded_index_by_entity.get(index).copied() else {
            continue;
        };
        let Some(slot) = levels.get_mut(expanded) else {
            continue;
        };
        let assigned = entity_layer_numbers(entity, &layer_of);
        let mut ordered: Vec<i64> = assigned.into_iter().collect();
        if ordered.len() == 1 {
            *slot = ordered[0].to_string();
            continue;
        }
        if ordered.len() > 1 {
            let wanted: BTreeSet<i64> = ordered.drain(..).collect();
            if let Some((definition, _)) = defined_sets.iter().find(|(_, levels)| *levels == wanted)
            {
                if let Some(definition_expanded) =
                    expanded_index_by_entity.get(*definition).copied()
                {
                    let sequence = definition_expanded
                        .checked_mul(2)
                        .and_then(|value| value.checked_add(1))
                        .and_then(|value| i64::try_from(value).ok());
                    if let Some(sequence) = sequence {
                        *slot = format!("-{sequence}");
                    }
                }
            }
        }
    }
    levels
}
