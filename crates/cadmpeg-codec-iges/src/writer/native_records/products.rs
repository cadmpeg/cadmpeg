// SPDX-License-Identifier: Apache-2.0
//! Types 124, 132, 184, 308, 320, 408, 412, 414, 416, 420 and 430 rebuilt
//! from typed native records.

use super::{coordinates, entity, integer, real, text, Entity, Fields, Value};

/// Resolve an instance when exactly one current occurrence uses it.
pub(super) fn current_occurrence<'a>(
    ir: &'a super::CadIr,
    arena: &str,
    record: &super::NativeRecord,
) -> Option<&'a cadmpeg_ir::products::Occurrence> {
    if !matches!(arena, "network_instances" | "subfigure_instances") {
        return None;
    }
    let source = record.field("source_entity")?.as_str()?.to_owned();
    let native = ir
        .native
        .namespace("iges")?
        .arenas()
        .get("product_occurrences")?;
    let candidates = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            native.iter().any(|native| {
                occurrence.native_ref.as_deref() == Some(native.id())
                    && native
                        .field("source_instance")
                        .as_ref()
                        .and_then(Value::as_str)
                        == Some(source.as_str())
            })
        })
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [occurrence] => Some(*occurrence),
        _ => None,
    }
}

/// A pointer that is zero when the typed record holds no reference.
fn optional_pointer(fields: &mut Fields<'_>, name: &str) -> Result<String, String> {
    match fields.value(name) {
        Some(Value::String(_)) => fields.pointer(name),
        _ => Ok("0".into()),
    }
}

fn definition_pointer(fields: &mut Fields<'_>) -> Result<String, String> {
    match fields.value("definition") {
        Some(Value::String(_)) => fields.pointer("definition"),
        _ => Err("record has no typed definition reference".into()),
    }
}

fn member_pointers(fields: &mut Fields<'_>, name: &str) -> Result<Vec<String>, String> {
    fields
        .array(name)?
        .iter()
        .map(|member| {
            if member.is_null() {
                return Ok("0".into());
            }
            fields.reference(Some(member))
        })
        .collect()
}

/// The declared count exactly as admitted; overdeclared counts stay
/// overdeclared, so the re-decode admits the same record. A missing
/// declaration falls back to the current typed member count.
fn declared_count(fields: &Fields<'_>, name: &str, actual: usize) -> Result<String, String> {
    match fields.value(name) {
        Some(value) if !value.is_null() => integer(Some(value)),
        _ => Ok(actual.to_string()),
    }
}

fn text_or_empty(fields: &Fields<'_>, name: &str) -> Result<String, String> {
    match fields.value(name) {
        Some(value) if !value.is_null() => text(Some(value)),
        _ => Ok(String::new()),
    }
}

fn display_template(fields: &mut Fields<'_>) -> Result<String, String> {
    match fields.value("display_template") {
        Some(Value::String(_)) => fields.pointer("display_template"),
        _ => Ok("0".into()),
    }
}

fn subfigure_definition(
    fields: &mut Fields<'_>,
    type_code: u32,
    form: i64,
) -> Result<Entity, String> {
    let mut values = vec![
        integer(fields.value("depth"))?,
        text_or_empty(fields, "name")?,
    ];
    let members = member_pointers(fields, "members")?;
    values.push(declared_count(
        fields,
        "declared_member_count",
        members.len(),
    )?);
    values.extend(members);
    Ok(entity(type_code, form, values))
}

fn network_parts(fields: &mut Fields<'_>) -> Result<Vec<String>, String> {
    let mut values = vec![
        integer(fields.value("depth"))?,
        text_or_empty(fields, "name")?,
    ];
    let members = member_pointers(fields, "members")?;
    values.push(declared_count(
        fields,
        "declared_member_count",
        members.len(),
    )?);
    values.extend(members);
    values.push(integer(fields.value("type_flag"))?);
    values.push(text_or_empty(fields, "primary_reference_designator")?);
    values.push(display_template(fields)?);
    let points = member_pointers(fields, "connect_points")?;
    values.push(declared_count(
        fields,
        "declared_connect_point_count",
        points.len(),
    )?);
    values.extend(points);
    Ok(values)
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
        "subfigure_definitions" => Ok((308, 0)),
        "subfigure_instances" => Ok((408, 0)),
        "network_definitions" => Ok((320, 0)),
        "network_instances" => Ok((420, 0)),
        "connect_points" => Ok((132, 0)),
        "solid_assemblies" => Ok((184, form()?)),
        "solid_instances" => Ok((430, form()?)),
        "external_references" => Ok((416, form()?)),
        "transformations" => Ok((124, form()?)),
        _ => Err("record is not a product, network or transformation record".into()),
    }
}

pub(super) fn encode(
    arena: &str,
    fields: &mut Fields<'_>,
    type_code: u32,
    form: i64,
) -> Result<Entity, String> {
    match arena {
        "subfigure_definitions" => subfigure_definition(fields, type_code, form),
        "subfigure_instances" => {
            let mut values = vec![definition_pointer(fields)?];
            values.extend(coordinates(fields.value("translation"), 3)?);
            values.push(real(fields.value("scale"))?);
            Ok(entity(type_code, form, values))
        }
        "network_definitions" => Ok(entity(type_code, form, network_parts(fields)?)),
        "network_instances" => {
            let mut values = vec![definition_pointer(fields)?];
            values.extend(coordinates(fields.value("translation"), 3)?);
            values.extend(coordinates(fields.value("scale"), 3)?);
            values.push(integer(fields.value("type_flag"))?);
            values.push(text_or_empty(fields, "primary_reference_designator")?);
            values.push(display_template(fields)?);
            let points = member_pointers(fields, "connect_points")?;
            values.push(declared_count(
                fields,
                "declared_connect_point_count",
                points.len(),
            )?);
            values.extend(points);
            Ok(entity(type_code, form, values))
        }
        "connect_points" => {
            let mut values = coordinates(fields.value("position"), 3)?;
            values.push(optional_pointer(fields, "display_geometry")?);
            values.push(integer(fields.value("type_flag"))?);
            values.push(integer(fields.value("function_flag"))?);
            values.push(text_or_empty(fields, "function_identifier")?);
            values.push(optional_pointer(fields, "identifier_display_template")?);
            values.push(text_or_empty(fields, "function_name")?);
            values.push(optional_pointer(fields, "name_display_template")?);
            values.push(integer(fields.value("identifier"))?);
            values.push(integer(fields.value("function_code"))?);
            values.push(integer(fields.value("swap_flag"))?);
            values.push(optional_pointer(fields, "owner")?);
            Ok(entity(type_code, form, values))
        }
        "solid_assemblies" => {
            let items = fields.array("items")?;
            let mut values = vec![declared_count(fields, "declared_count", items.len())?];
            let mut transforms = Vec::new();
            for item in &items {
                let item = item
                    .as_object()
                    .ok_or("solid assembly item has no typed fields")?;
                let member = item.get("item").filter(|value| !value.is_null());
                match member {
                    None => values.push("0".into()),
                    Some(member) => values.push(fields.reference(Some(member))?),
                }
                let transform = item.get("transformation").filter(|value| !value.is_null());
                match transform {
                    None => transforms.push("0".into()),
                    Some(transform) => transforms.push(fields.reference(Some(transform))?),
                }
            }
            values.extend(transforms);
            Ok(entity(type_code, form, values))
        }
        "solid_instances" => Ok(entity(
            type_code,
            form,
            vec![optional_pointer(fields, "solid")?],
        )),
        "external_references" => {
            let mut values = Vec::new();
            match form {
                0 | 2 => {
                    values.push(text_or_empty(fields, "file_identifier")?);
                    values.push(text_or_empty(fields, "symbolic_name")?);
                }
                1 => {
                    values.push(text_or_empty(fields, "file_identifier")?);
                }
                3 => {
                    values.push(text_or_empty(fields, "symbolic_name")?);
                }
                4 => {
                    values.push(text_or_empty(fields, "library_name")?);
                    values.push(text_or_empty(fields, "symbolic_name")?);
                }
                _ => return Err("external reference has no typed form grammar".into()),
            }
            Ok(entity(type_code, form, values))
        }
        "transformations" => {
            let coefficients = fields
                .value("coefficients")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 12)
                .ok_or_else(|| "transformation has no twelve-coefficient lane".to_owned())?;
            let mut values = Vec::new();
            for coefficient in coefficients {
                values.push(real(Some(coefficient))?);
            }
            let mut result = entity(type_code, form, values);
            if let Some(parent) = fields.value("parent").cloned() {
                if !parent.is_null() {
                    let id = parent
                        .as_str()
                        .ok_or("transformation parent is not a typed identity")?;
                    result.transform_link = Some(fields.target_index(id)?);
                }
            }
            Ok(result)
        }
        _ => Err("record is not a product, network or transformation record".into()),
    }
}
