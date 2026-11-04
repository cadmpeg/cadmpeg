// SPDX-License-Identifier: Apache-2.0
//! Types 402 Forms 3, 4 and 19, 404 and 410 rebuilt from typed records.
//!
//! A 404's name, size and units properties trail its annotations as
//! property pointers. The 406 Form 15 name re-emits with the attribute
//! writer; Forms 16 and 17 are never admitted as property records, so the
//! fixup below rebuilds them from the drawing record's values after the
//! attribute writer runs.

use super::{entity, integer, real, Entity, Fields, Value};

/// The declared count exactly as admitted, falling back to the current
/// typed member count when the declaration is missing.
fn declared_count(fields: &Fields<'_>, name: &str, actual: usize) -> Result<String, String> {
    match fields.value(name) {
        Some(value) if !value.is_null() => integer(Some(value)),
        _ => Ok(actual.to_string()),
    }
}

fn token_text(token: &Value) -> Result<String, String> {
    let kind = token
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "segment display value has no kind tag".to_owned())?;
    match kind {
        "omitted" => Ok(String::new()),
        "integer" => token
            .get("value")
            .and_then(Value::as_i64)
            .map(|value| value.to_string())
            .ok_or("segment display integer is not an integer".into()),
        "real" => token
            .get("value")
            .and_then(Value::as_f64)
            .and_then(super::FiniteReal::new)
            .map(super::number)
            .ok_or("segment display real is not a finite real".into()),
        "string" => {
            let bytes = crate::text::field(
                token
                    .get("value")
                    .ok_or_else(|| "segment display string has no value".to_owned())?,
            )?;
            let text = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
            Ok(format!("{}H{text}", bytes.len()))
        }
        _ => Err("segment display value has an unreadable typed literal".into()),
    }
}

pub(super) fn shape(arena: &str, record: &super::NativeRecord) -> Result<(u32, i64), String> {
    let form = || -> Result<i64, String> {
        record
            .field("form")
            .as_ref()
            .and_then(Value::as_i64)
            .ok_or("record has no typed form".into())
    };
    match arena {
        "views" => Ok((410, form()?)),
        "view_visibility" => Ok((402, form()?)),
        "segmented_visibility" => Ok((402, 19)),
        "drawings" => Ok((404, form()?)),
        _ => Err("record is not a drawing, view or visibility record".into()),
    }
}

pub(super) fn encode(
    arena: &str,
    fields: &mut Fields<'_>,
    type_code: u32,
    form: i64,
) -> Result<Entity, String> {
    match arena {
        "views" => {
            let mut values = vec![
                integer(fields.value("view_number"))?,
                real(fields.value("scale"))?,
            ];
            if form == 0 {
                let clips = fields
                    .value("clipping_planes")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for clip in clips.iter().take(6) {
                    if clip.is_null() {
                        values.push("0".into());
                    } else {
                        values.push(fields.reference(Some(clip))?);
                    }
                }
                let mut result = entity(type_code, form, values);
                if let Some(parent) = fields.value("model_to_view").cloned() {
                    if !parent.is_null() {
                        let id = parent
                            .as_str()
                            .ok_or("view transform is not a typed identity")?;
                        result.transform_link = Some(fields.target_index(id)?);
                    }
                }
                Ok(result)
            } else {
                for lane in [
                    "view_plane_normal",
                    "view_reference_point",
                    "center_of_projection",
                    "view_up",
                ] {
                    let vector = fields
                        .value(lane)
                        .and_then(Value::as_array)
                        .filter(|values| values.len() == 3)
                        .cloned()
                        .unwrap_or_default();
                    for component in &vector {
                        values.push(real(Some(component))?);
                    }
                }
                values.push(real(fields.value("view_plane_distance"))?);
                let window = fields
                    .value("clipping_window")
                    .and_then(Value::as_array)
                    .filter(|values| values.len() == 4)
                    .cloned()
                    .unwrap_or_default();
                for component in &window {
                    values.push(real(Some(component))?);
                }
                values.push(integer(fields.value("depth_clipping"))?);
                let range = fields
                    .value("depth_range")
                    .and_then(Value::as_array)
                    .filter(|values| values.len() == 2)
                    .cloned()
                    .unwrap_or_default();
                for component in &range {
                    values.push(real(Some(component))?);
                }
                Ok(entity(type_code, form, values))
            }
        }
        "view_visibility" => {
            let mut values = Vec::new();
            if form != 3 && form != 4 {
                return Err("view visibility has no typed form grammar".into());
            }
            let displays = fields.array("displays")?;
            let entities = fields.array("entities")?;
            values.push(declared_count(
                fields,
                "declared_view_count",
                displays.len(),
            )?);
            values.push(declared_count(
                fields,
                "declared_entity_count",
                entities.len(),
            )?);
            for display in &displays {
                let display = display
                    .as_object()
                    .ok_or("view display has no typed fields")?;
                let view = display.get("view").filter(|value| !value.is_null());
                match view {
                    None => values.push("0".into()),
                    Some(view) => values.push(fields.reference(Some(view))?),
                }
                if form == 4 {
                    values.push(integer(display.get("line_font"))?);
                    match display
                        .get("line_font_definition")
                        .filter(|value| !value.is_null())
                    {
                        None => values.push("0".into()),
                        Some(definition) => values.push(fields.reference(Some(definition))?),
                    }
                    values.push(integer(display.get("color"))?);
                    values.push(integer(display.get("line_weight"))?);
                }
            }
            for target in &entities {
                if target.is_null() {
                    values.push("0".into());
                } else {
                    values.push(fields.reference(Some(target))?);
                }
            }
            Ok(entity(type_code, form, values))
        }
        "segmented_visibility" => {
            let blocks = fields.array("blocks")?;
            let mut values = vec![declared_count(
                fields,
                "declared_block_count",
                blocks.len(),
            )?];
            for block in &blocks {
                let block = block
                    .as_object()
                    .ok_or("visibility block has no typed fields")?;
                let view = block.get("view").filter(|value| !value.is_null());
                match view {
                    None => values.push("0".into()),
                    Some(view) => values.push(fields.reference(Some(view))?),
                }
                values.push(real(block.get("breakpoint"))?);
                values.push(integer(block.get("display_flag"))?);
                for lane in ["color", "line_font", "line_weight"] {
                    let value = block
                        .get(lane)
                        .ok_or("visibility block has no typed display value")?;
                    values.push(token_text(value)?);
                }
            }
            Ok(entity(type_code, form, values))
        }
        "drawings" => {
            let views = fields.array("views")?;
            let mut values = vec![declared_count(fields, "declared_view_count", views.len())?];
            for slot in &views {
                let slot = slot.as_object().ok_or("drawing view has no typed fields")?;
                let view = slot.get("view").filter(|value| !value.is_null());
                match view {
                    None => values.push("0".into()),
                    Some(view) => values.push(fields.reference(Some(view))?),
                }
                let origin = slot
                    .get("origin")
                    .and_then(Value::as_array)
                    .filter(|values| values.len() == 2)
                    .cloned()
                    .unwrap_or_default();
                for component in &origin {
                    values.push(real(Some(component))?);
                }
                if form == 1 {
                    values.push(real(slot.get("rotation"))?);
                }
            }
            let annotations = fields.array("annotations")?;
            values.push(declared_count(
                fields,
                "declared_annotation_count",
                annotations.len(),
            )?);
            for annotation in &annotations {
                if annotation.is_null() {
                    values.push("0".into());
                } else {
                    values.push(fields.reference(Some(annotation))?);
                }
            }
            Ok(entity(type_code, form, values))
        }
        _ => Err("record is not a drawing, view or visibility record".into()),
    }
}

/// Link each re-emitted 404 to its name property and rebuild its size and
/// units properties from the drawing record's values. The Form 15 name
/// re-emits with the attribute writer when admitted; Forms 16 and 17 are
/// never admitted as property records, so they are synthesized here.
pub(in crate::writer) fn link_drawing_properties(
    ir: &super::CadIr,
    entities: &mut Vec<super::Entity>,
) {
    use std::collections::BTreeMap;
    let Some(namespace) = ir.native.namespace("iges") else {
        return;
    };
    let Some(drawings) = namespace.arenas().get("drawings") else {
        return;
    };
    let mut by_record: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, entity) in entities.iter().enumerate() {
        for target in &entity.source_targets {
            by_record.entry(target.as_str()).or_insert(index);
        }
    }
    let targets = super::super::attributes::targets(ir, entities);
    let mut links: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut synthesized: Vec<super::Entity> = Vec::new();
    for record in drawings {
        let Ok(record_fields) = record.fields() else {
            continue;
        };
        let Some(entity_index) = by_record.get(record.id()).copied() else {
            continue;
        };
        let mut property_links: Vec<usize> = Vec::new();
        if let Some(property) = record_fields
            .get("name_property")
            .and_then(serde_json::Value::as_str)
        {
            match targets.get(property).map(Vec::as_slice) {
                Some([index]) => property_links.push(*index),
                _ => {
                    if let Some(body) = name_body(record_fields.get("name")) {
                        property_links.push(entities.len() + synthesized.len());
                        synthesized.push(unit_property(406, 15, body));
                    }
                }
            }
        }
        if let Some(body) = size_body(record_fields.get("size")) {
            property_links.push(entities.len() + synthesized.len());
            synthesized.push(unit_property(406, 16, body));
        }
        if let Some(body) = units_body(
            record_fields.get("units_flag"),
            record_fields.get("units_name"),
        ) {
            property_links.push(entities.len() + synthesized.len());
            synthesized.push(unit_property(406, 17, body));
        }
        links.push((entity_index, property_links));
    }
    for (entity_index, property_links) in links {
        entities[entity_index].property_links.extend(property_links);
    }
    entities.extend(synthesized);
}

fn unit_property(type_code: u32, form: i64, parameter_body: Vec<u8>) -> super::Entity {
    super::Entity {
        structure: None,
        property_links: Vec::new(),
        association_links: Vec::new(),
        line_font: None,
        source_targets: Vec::new(),
        type_code,
        form,
        label: String::new(),
        status: super::EntityStatus::Independent,
        parameter_body,
        transform: None,
        transform_link: None,
        subscript: 0,
    }
}

fn name_body(name: Option<&serde_json::Value>) -> Option<Vec<u8>> {
    let bytes = crate::text::field(name?);
    let bytes = bytes.ok()?;
    let label = std::str::from_utf8(&bytes).ok()?;
    Some(format!("1,{}H{};", bytes.len(), label).into_bytes())
}

fn size_body(size: Option<&serde_json::Value>) -> Option<Vec<u8>> {
    let size = size?.as_array()?;
    if size.len() != 2 {
        return None;
    }
    let mut numbers = Vec::new();
    for component in size {
        numbers.push(super::number(super::FiniteReal::new(component.as_f64()?)?));
    }
    Some(format!("2,{};", numbers.join(",")).into_bytes())
}

fn units_body(
    flag: Option<&serde_json::Value>,
    name: Option<&serde_json::Value>,
) -> Option<Vec<u8>> {
    let flag = flag?.as_i64()?;
    let bytes = crate::text::field(name?);
    let bytes = bytes.ok()?;
    let label = std::str::from_utf8(&bytes).ok()?;
    Some(format!("2,{flag},{}H{};", bytes.len(), label).into_bytes())
}
