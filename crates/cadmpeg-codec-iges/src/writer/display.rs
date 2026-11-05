// SPDX-License-Identifier: Apache-2.0
//! Attribution for Directory display records reconstructed without loss.

use super::{attributes, levels, BodyColor, BodyPresentation, CadIr, DirectoryLineFont, Entity};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn regenerated(
    ir: &CadIr,
    entities: &[Entity],
    presentations: &BTreeMap<usize, BodyPresentation>,
    version: crate::IgesVersion,
) -> BTreeSet<String> {
    let Some(records) = ir
        .native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("display_attributes"))
    else {
        return BTreeSet::new();
    };
    let targets = attributes::targets(ir, entities);
    let memberships = levels::layer_memberships(ir);
    records
        .iter()
        .filter_map(|record| {
            let fields = record.fields().ok()?;
            for field in ["line_weight_number", "view"] {
                if fields.get(field).and_then(serde_json::Value::as_i64) != Some(0) {
                    return None;
                }
            }
            for field in ["color_definition", "level_definition", "line_weight_mm"] {
                if fields.get(field).is_some_and(|value| !value.is_null()) {
                    return None;
                }
            }
            let source = fields.get("source_entity")?.as_str()?;
            let [index] = targets.get(source)?.as_slice() else {
                return None;
            };
            let entity = entities.get(*index)?;
            let font = match entity.line_font {
                Some(DirectoryLineFont::Number(number)) => {
                    if fields
                        .get("line_font_definition")
                        .is_some_and(|value| !value.is_null())
                    {
                        return None;
                    }
                    number
                }
                None => {
                    if fields
                        .get("line_font_definition")
                        .is_some_and(|value| !value.is_null())
                    {
                        return None;
                    }
                    super::generated_line_font(version, entity.type_code, entity.form)
                }
                Some(DirectoryLineFont::Definition(font)) => {
                    let definition = fields.get("line_font_definition")?.as_str()?;
                    if targets.get(definition).map(Vec::as_slice) != Some(&[font][..]) {
                        return None;
                    }
                    -directory_sequence(entities, font)?
                }
            };
            if fields
                .get("line_font_number")
                .and_then(serde_json::Value::as_i64)
                != Some(font)
            {
                return None;
            }
            let visible = presentations
                .get(index)
                .and_then(|presentation| presentation.visible)
                != Some(false);
            if fields.get("visible").and_then(serde_json::Value::as_bool) != Some(visible) {
                return None;
            }
            let assigned = levels::entity_layer_numbers(entity, &memberships);
            let level = match assigned.len() {
                0 => 0,
                1 => *assigned.first()?,
                _ => return None,
            };
            if fields
                .get("level_number")
                .and_then(serde_json::Value::as_i64)
                != Some(level)
            {
                return None;
            }
            let color = match presentations
                .get(index)
                .map(|presentation| presentation.color)
            {
                None => 0,
                Some(BodyColor::Standard(number)) => number,
                _ => return None,
            };
            if fields
                .get("color_number")
                .and_then(serde_json::Value::as_i64)
                != Some(color)
            {
                return None;
            }
            Some(record.id().to_owned())
        })
        .collect()
}

/// Directory pointers count inserted placement entities as well as carriers.
fn directory_sequence(entities: &[Entity], index: usize) -> Option<i64> {
    let placement_count = entities
        .get(..=index)?
        .iter()
        .filter(|entity| entity.transform.is_some())
        .count();
    let expanded = index.checked_add(placement_count)?;
    i64::try_from(expanded.checked_mul(2)?.checked_add(1)?).ok()
}
