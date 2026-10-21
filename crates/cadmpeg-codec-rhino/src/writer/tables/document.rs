// SPDX-License-Identifier: Apache-2.0
//! Properties regenerated through their current neutral attribute links.

use super::*;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};

fn current_fields(
    ir: &CadIr,
    record: &cadmpeg_ir::NativeRecord,
) -> Result<Option<Map<String, Value>>, CodecError> {
    let mut fields = record.fields()?;
    let links = fields
        .get("neutral_attributes")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (name, id) in links {
        let Some(attribute) = id.as_str().and_then(|id| {
            ir.model.attributes.iter().find(|attribute| {
                attribute.id.as_str() == id && attribute.target == AttributeTarget::Document
            })
        }) else {
            return Ok(None);
        };
        let value = match attribute.values.as_slice() {
            [AttributeValue::String(value)] => Value::String(value.clone()),
            [AttributeValue::Boolean(value)] => Value::Bool(*value),
            [AttributeValue::Integer(value)] => (*value).into(),
            [AttributeValue::Float(value)] => {
                serde_json::to_value(value).map_err(CodecError::malformed)?
            }
            [AttributeValue::Vector(values)] => {
                serde_json::to_value(values).map_err(CodecError::malformed)?
            }
            _ => return Ok(None),
        };
        fields.insert(name, value);
    }
    Ok(Some(fields))
}

fn text(fields: &Map<String, Value>, name: &str, output: &mut Vec<u8>) -> Result<(), CodecError> {
    output.extend(utf16(
        fields.get(name).and_then(Value::as_str).unwrap_or(""),
    )?);
    Ok(())
}

fn integer_vector(
    fields: &Map<String, Value>,
    name: &str,
    count: usize,
    output: &mut Vec<u8>,
) -> Result<(), CodecError> {
    let values = fields
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| values.len() == count)
        .ok_or_else(|| {
            CodecError::UnsupportedContent(format!(
                "Rhino document {name} needs {count} integer fields"
            ))
        })?;
    for value in values {
        let value = value
            .as_i64()
            .or_else(|| {
                value
                    .as_f64()
                    .filter(|value| value.fract() == 0.0)
                    .and_then(cadmpeg_core::convert::truncate_f64_to_i64)
            })
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| {
                CodecError::UnsupportedContent(format!(
                    "Rhino document {name} field is outside its integer lane"
                ))
            })?;
        output.extend(value.to_le_bytes());
    }
    Ok(())
}

pub(in crate::writer) struct PropertyExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    pub(in crate::writer) regenerated: std::collections::BTreeSet<String>,
    pub(in crate::writer) losses: Vec<LossNote>,
}

fn property(arena: &str, fields: &Map<String, Value>) -> Result<Vec<u8>, CodecError> {
    let mut payload = vec![if arena == "document_notes" {
        0x11
    } else {
        0x10
    }];
    let typecode = match arena {
        "revisions" => {
            text(&fields, "created_by", &mut payload)?;
            integer_vector(&fields, "created_utc_fields", 8, &mut payload)?;
            text(&fields, "last_edited_by", &mut payload)?;
            integer_vector(&fields, "last_edited_utc_fields", 8, &mut payload)?;
            payload.extend(integer(Some(&fields), "revision_count", 0).to_le_bytes());
            0x2000_8021
        }
        "document_notes" => {
            payload.extend(
                i32::from(fields.get("html").and_then(Value::as_bool).unwrap_or(false))
                    .to_le_bytes(),
            );
            text(&fields, "text", &mut payload)?;
            payload.extend(
                i32::from(
                    fields
                        .get("visible")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                )
                .to_le_bytes(),
            );
            integer_vector(&fields, "window_rectangle", 4, &mut payload)?;
            payload.push(u8::from(
                fields
                    .get("locked")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ));
            0x2000_8022
        }
        _ => {
            for name in ["name", "url", "details"] {
                text(&fields, name, &mut payload)?;
            }
            0x2000_8024
        }
    };
    crc_chunk(typecode, &payload)
}

pub(in crate::writer) fn properties(ir: &CadIr) -> Result<PropertyExport, CodecError> {
    let mut output = PropertyExport {
        records: Vec::new(),
        regenerated: std::collections::BTreeSet::new(),
        losses: Vec::new(),
    };
    for arena in ["revisions", "document_notes", "applications"] {
        for record in records(ir, arena) {
            let Some(fields) = current_fields(ir, record)? else {
                continue;
            };
            match property(arena, &fields) {
                Ok(encoded) => {
                    output.records.push(encoded);
                    output.regenerated.insert(record.id().to_owned());
                }
                Err(CodecError::UnsupportedContent(reason)) => output.losses.push(
                    RhinoLossCode::WriterAttributesOmitted
                        .note(format!("{}: {reason}", record.id())),
                ),
                Err(error) => return Err(error),
            }
        }
    }
    for record in records(ir, "document_settings") {
        if let Some(value) = record
            .field("archive_file_name")
            .and_then(|value| value.as_str().map(str::to_owned))
        {
            output
                .records
                .push(crc_chunk(0x2000_8027, &utf16(&value)?)?);
        }
    }
    Ok(output)
}

/// Whether synthesis emits every held setting, including an empty settings state.
pub(in crate::writer) fn settings_complete(
    ir: &CadIr,
    record: &cadmpeg_ir::NativeRecord,
) -> Result<bool, CodecError> {
    let fields = record.fields()?;
    let present = |key: &str| fields.get(key).is_some_and(|value| !value.is_null());
    // Font and dimension-style selections need their corresponding table
    // writer support; do not claim those settings were emitted yet.
    if present("current_font_index") || present("current_dimension_style_index") {
        return Ok(false);
    }
    for (field, arena, neutral) in [
        ("current_layer_index", "layers", "presentation_layers"),
        ("current_material_index", "materials", "appearances"),
    ] {
        let Some(index) = fields.get(field).filter(|value| !value.is_null()) else {
            continue;
        };
        let Some(id) = records(ir, arena)
            .iter()
            .find(|record| record.field("archive_index").as_ref() == Some(index))
            .and_then(|record| record.field("neutral_ref"))
            .and_then(|value| value.as_str().map(str::to_owned))
        else {
            return Ok(false);
        };
        let resolves = if neutral == "presentation_layers" {
            ir.model
                .presentation_layers
                .iter()
                .any(|record| record.id.as_str() == id)
        } else {
            ir.model
                .appearances
                .iter()
                .any(|record| record.id.as_str() == id)
        };
        if !resolves {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(in crate::writer) fn settings(ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut output = Vec::new();
    for record in records(ir, "document_settings") {
        let fields = record.fields()?;
        if let Some(url) = fields.get("model_url").and_then(Value::as_str) {
            output.push(crc_chunk(0x2000_8131, &utf16(url)?)?);
        }
        if let Some(value) = fields.get("current_wire_density").and_then(Value::as_i64) {
            output.push(short_chunk(0xa000_003c, value));
        }
        if fields
            .get("current_color")
            .is_some_and(|value| !value.is_null())
        {
            let mut body = color(Some(&fields), "current_color")?.to_vec();
            body.extend(integer(Some(&fields), "current_color_source", 0).to_le_bytes());
            output.push(crc_chunk(0x2000_803a, &body)?);
        }
        if let Some(index) = fields.get("current_layer_index").and_then(Value::as_i64) {
            if let Some(id) = records(ir, "layers")
                .iter()
                .find(|record| {
                    record
                        .field("archive_index")
                        .and_then(|value| value.as_i64())
                        == Some(index)
                })
                .and_then(|record| record.field("neutral_ref"))
                .and_then(|value| value.as_str().map(str::to_owned))
            {
                if let Some(index) = ir
                    .model
                    .presentation_layers
                    .iter()
                    .position(|layer| layer.id.as_str() == id)
                {
                    output.push(short_chunk(0xa000_0038, i64::from(wire_index(index)?)));
                }
            }
        }
        if let Some(index) = fields.get("current_material_index").and_then(Value::as_i64) {
            if let Some(id) = records(ir, "materials")
                .iter()
                .find(|record| {
                    record
                        .field("archive_index")
                        .and_then(|value| value.as_i64())
                        == Some(index)
                })
                .and_then(|record| record.field("neutral_ref"))
                .and_then(|value| value.as_str().map(str::to_owned))
            {
                if let Some(index) = ir
                    .model
                    .appearances
                    .iter()
                    .position(|appearance| appearance.id.as_str() == id)
                {
                    let mut body = wire_index(index)?.to_le_bytes().to_vec();
                    body.extend(integer(Some(&fields), "current_material_source", 0).to_le_bytes());
                    output.push(crc_chunk(0x2000_8039, &body)?);
                }
            }
        }
    }
    Ok(output)
}
