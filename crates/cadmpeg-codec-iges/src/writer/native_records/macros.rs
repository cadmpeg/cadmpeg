// SPDX-License-Identifier: Apache-2.0
//! Type 306 macro definitions and macro instances rebuilt from typed records.
//!
//! The decoder keeps only the defined entity type of a 306 definition, so a
//! re-emitted definition carries a minimal valid macro body naming one formal
//! parameter. Instances echo their entity type and re-emit every stored token.

use super::{
    entity, number, Entity, EntityStatus, Fields, FiniteReal, NativeRecord, Value,
};

pub(super) fn shape(arena: &str, record: &NativeRecord) -> Result<(u32, i64), String> {
    let form = || -> Result<i64, String> {
        record
            .field("form")
            .as_ref()
            .and_then(Value::as_i64)
            .ok_or("record has no typed form".into())
    };
    match arena {
        "macro_definitions" => Ok((306, form().unwrap_or(0))),
        "macro_instances" => {
            let entity_type = record
                .field("entity_type")
                .as_ref()
                .and_then(Value::as_i64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or("macro instance has no typed entity type")?;
            Ok((entity_type, form()?))
        }
        _ => Err("record is not a macro definition or instance".into()),
    }
}

fn token_text(token: &Value) -> Result<String, String> {
    let Some(wire) = token.get("value") else {
        return Err("macro parameter has no typed value".into());
    };
    let kind = wire
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("macro parameter has no kind tag")?;
    match kind {
        "omitted" => Ok(String::new()),
        "integer" => wire
            .get("value")
            .and_then(Value::as_i64)
            .map(|value| value.to_string())
            .ok_or("macro integer parameter is not an integer".into()),
        "real" => wire
            .get("value")
            .and_then(Value::as_f64)
            .and_then(FiniteReal::new)
            .map(number)
            .ok_or("macro real parameter is not a finite real".into()),
        "string" => {
            let bytes = crate::text::field(
                wire.get("value").ok_or("macro string parameter has no value")?,
            )?;
            let text = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
            Ok(format!("{}H{text}", bytes.len()))
        }
        _ => Err("macro parameter has an unreadable typed literal".into()),
    }
}

pub(super) fn encode(
    arena: &str,
    fields: &mut Fields<'_>,
    type_code: u32,
    form: i64,
) -> Result<Entity, String> {
    if arena == "macro_definitions" {
        let defined = fields.integer("defined_entity_type")?;
        let body = format!("MACRO,{defined},X;ENDM;");
        return Ok(Entity {
            structure: None,
            property_links: Vec::new(),
            association_links: Vec::new(),
            line_font: None,
            source_targets: Vec::new(),
            type_code,
            form,
            label: String::new(),
            status: EntityStatus::Definition,
            parameter_body: body.into_bytes(),
            transform: None,
            subscript: 0,
        });
    }
    let mut parameters = Vec::new();
    for token in fields.array("parameters")? {
        if token.is_null() {
            return Err("macro instance holds an unreadable parameter".into());
        }
        parameters.push(token_text(&token)?);
    }
    let mut result = entity(type_code, form, parameters);
    if let Some(definition) = fields.value("macro_definition").cloned() {
        let id = definition
            .as_str()
            .ok_or("macro instance definition is not a typed identity")?;
        result.structure = Some(fields.target_index(id)?);
    }
    Ok(result)
}
