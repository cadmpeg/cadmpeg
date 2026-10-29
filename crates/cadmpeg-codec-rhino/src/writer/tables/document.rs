// SPDX-License-Identifier: Apache-2.0
//! Properties regenerated through their current neutral attribute links.

use super::{
    color, crc_chunk, integer, records, short_chunk, utf16, wire_index, CadIr, CodecError,
    LossNote, Map, RhinoLossCode, Value,
};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

struct MissingPositions<'a>(&'a [Option<AttributeValue>]);

impl std::fmt::Display for MissingPositions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[")?;
        let mut separator = "";
        for (position, value) in self.0.iter().enumerate() {
            if value.is_none() {
                write!(f, "{separator}{position}")?;
                separator = ", ";
            }
        }
        f.write_str("]")
    }
}

fn unreadable_attribute_loss(attribute: &SourceAttribute) -> Option<LossNote> {
    attribute.values.iter().any(Option::is_none).then(|| {
        RhinoLossCode::WriterAttributesOmitted.note(format!(
            "attribute {} is omitted because its scalar target cannot hold None positions {}",
            attribute.id,
            MissingPositions(&attribute.values)
        ))
    })
}

pub(in crate::writer) fn uncarried_attribute_losses(ir: &CadIr, losses: &mut Vec<LossNote>) {
    for attribute in &ir.model.attributes {
        let document_carrier = attribute.native_ref.as_deref().is_some_and(|id| {
            ["revisions", "document_notes", "applications"]
                .into_iter()
                .any(|arena| records(ir, arena).iter().any(|record| record.id() == id))
        });
        if document_carrier {
            continue;
        }
        if let Some(loss) = unreadable_attribute_loss(attribute) {
            losses.push(loss);
        } else if attribute.values.iter().flatten().any(|value| {
            matches!(
                value,
                AttributeValue::Map(_)
                    | AttributeValue::Expression(_)
                    | AttributeValue::SubReference { .. }
                    | AttributeValue::ExternalReference { .. }
                    | AttributeValue::EmptyReference
                    | AttributeValue::Json(_)
            )
        }) {
            losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                "attribute {} has no exact Rhino attribute carrier for its structured value",
                attribute.id
            )));
        }
    }
}

fn current_fields(
    ir: &CadIr,
    arena: &str,
    record: &cadmpeg_ir::NativeRecord,
) -> Result<Map<String, Value>, CodecError> {
    let mut fields = record.fields()?;
    let prefix = format!("rhino.{arena}.");
    let mut linked = std::collections::BTreeSet::new();
    for attribute in ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.native_ref.as_deref() == Some(record.id()))
    {
        let role = attribute
            .name
            .strip_prefix(prefix.as_str())
            .filter(|role| match arena {
                "document_notes" => matches!(
                    *role,
                    "html" | "text" | "visible" | "window_rectangle" | "locked"
                ),
                "revisions" => matches!(
                    *role,
                    "created_by"
                        | "created_utc_fields"
                        | "last_edited_by"
                        | "last_edited_utc_fields"
                        | "revision_count"
                ),
                "applications" => matches!(*role, "name" | "url" | "details"),
                _ => false,
            })
            .ok_or_else(|| {
                CodecError::UnsupportedContent(format!(
                    "attribute {} has no current Rhino {arena} field role",
                    attribute.id
                ))
            })?;
        if attribute.target != AttributeTarget::Document || !linked.insert(role) {
            return Err(CodecError::UnsupportedContent(format!(
                "attribute {} has an invalid or duplicate current Rhino {arena}.{role} binding",
                attribute.id
            )));
        }
        let value = match attribute.values.as_slice() {
            [Some(AttributeValue::String(value))] => Value::String(value.clone()),
            [Some(AttributeValue::Boolean(value))] => Value::Bool(*value),
            [Some(AttributeValue::Integer(value))] => (*value).into(),
            [Some(AttributeValue::Float(value))] => {
                serde_json::to_value(value).map_err(CodecError::malformed)?
            }
            [Some(AttributeValue::Vector(values))] => {
                serde_json::to_value(values).map_err(CodecError::malformed)?
            }
            _ => {
                return Err(CodecError::UnsupportedContent(format!(
                    "attribute {} has no single current Rhino {arena}.{role} field value",
                    attribute.id
                )))
            }
        };
        fields.insert(role.to_owned(), value);
    }
    Ok(fields)
}

fn boolean(fields: &Map<String, Value>, name: &str) -> Result<bool, CodecError> {
    fields.get(name).and_then(Value::as_bool).ok_or_else(|| {
        CodecError::UnsupportedContent(format!("Rhino document {name} needs a boolean value"))
    })
}

fn text(fields: &Map<String, Value>, name: &str, output: &mut Vec<u8>) -> Result<(), CodecError> {
    let value = fields.get(name).and_then(Value::as_str).ok_or_else(|| {
        CodecError::UnsupportedContent(format!("Rhino document {name} needs a string value"))
    })?;
    output.extend(utf16(value)?);
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
    pub(in crate::writer) written: std::collections::BTreeSet<String>,
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
            text(fields, "created_by", &mut payload)?;
            integer_vector(fields, "created_utc_fields", 8, &mut payload)?;
            text(fields, "last_edited_by", &mut payload)?;
            integer_vector(fields, "last_edited_utc_fields", 8, &mut payload)?;
            let count = fields
                .get("revision_count")
                .and_then(Value::as_i64)
                .and_then(|count| i32::try_from(count).ok())
                .ok_or_else(|| {
                    CodecError::UnsupportedContent(
                        "Rhino document revision_count needs an i32 value".into(),
                    )
                })?;
            payload.extend(count.to_le_bytes());
            0x2000_8021
        }
        "document_notes" => {
            payload.extend(i32::from(boolean(fields, "html")?).to_le_bytes());
            text(fields, "text", &mut payload)?;
            payload.extend(i32::from(boolean(fields, "visible")?).to_le_bytes());
            integer_vector(fields, "window_rectangle", 4, &mut payload)?;
            payload.push(u8::from(boolean(fields, "locked")?));
            0x2000_8022
        }
        _ => {
            for name in ["name", "url", "details"] {
                text(fields, name, &mut payload)?;
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
        written: std::collections::BTreeSet::new(),
        losses: Vec::new(),
    };
    for arena in ["revisions", "document_notes", "applications"] {
        for record in records(ir, arena) {
            let mut unreadable = false;
            for attribute in ir
                .model
                .attributes
                .iter()
                .filter(|attribute| attribute.native_ref.as_deref() == Some(record.id()))
            {
                if let Some(loss) = unreadable_attribute_loss(attribute) {
                    output.losses.push(loss);
                    unreadable = true;
                }
            }
            if unreadable {
                continue;
            }
            let encoded =
                current_fields(ir, arena, record).and_then(|fields| property(arena, &fields));
            match encoded {
                Ok(encoded) => {
                    let unchanged = match property(arena, &record.fields()?) {
                        Ok(native) => native == encoded,
                        Err(CodecError::UnsupportedContent(_)) => false,
                        Err(error) => return Err(error),
                    };
                    output.records.push(encoded);
                    if unchanged {
                        output.written.insert(record.id().to_owned());
                    } else {
                        output.regenerated.insert(record.id().to_owned());
                    }
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

pub(in crate::writer) fn settings(
    ir: &CadIr,
    materials: &super::materials::MaterialPlan,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut output = Vec::new();
    for setting in records(ir, "document_settings") {
        let fields = setting.fields()?;
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
            if let Some(record) = records(ir, "layers").iter().find(|record| {
                record
                    .field("archive_index")
                    .and_then(|value| value.as_i64())
                    == Some(index)
            }) {
                if let Some(index) = ir
                    .model
                    .presentation_layers
                    .iter()
                    .position(|layer| layer.native_ref.as_deref() == Some(record.id()))
                {
                    output.push(short_chunk(0xa000_0038, i64::from(wire_index(index)?)));
                }
            }
        }
        if let Some(index) = fields.get("current_material_index").and_then(Value::as_i64) {
            if let Some(record) = records(ir, "materials").iter().find(|record| {
                record
                    .field("archive_index")
                    .and_then(|value| value.as_i64())
                    == Some(index)
            }) {
                match materials.native_targets(record.id()) {
                    [material_index] => {
                        let mut body = material_index.to_le_bytes().to_vec();
                        body.extend(integer(Some(&fields),"current_material_source",0).to_le_bytes());
                        output.push(crc_chunk(0x2000_8039,&body)?);
                    }
                    [] => {},
                    _ => losses.push(RhinoLossCode::WriterAttributesOmitted.note(format!("document setting {} /current_material_index {} -> absent; native material {} has multiple authored appearances {:?}, so no unique target exists",setting.id(),index,record.id(),super::materials::MaterialPlan::target_names(ir,record.id())))),
                }
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
