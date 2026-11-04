// SPDX-License-Identifier: Apache-2.0
//! Types 150 through 168, 180, 182, 412 and 414 rebuilt from typed records.

use super::{entity, integer, real, Entity, Fields, Value};

fn operand(fields: &mut Fields<'_>, term: &Value) -> Result<String, String> {
    let item = term
        .as_object()
        .ok_or("boolean operand has no typed fields")?;
    match item.get("entity") {
        // Boolean operands are negated Directory pointers.
        Some(value) if !value.is_null() => Ok(format!("-{}", fields.reference(Some(value))?)),
        _ => item
            .get("raw")
            .and_then(Value::as_i64)
            .map(|raw| raw.to_string())
            .ok_or("boolean operand has no raw pointer".into()),
    }
}

fn dimensions(dimensions: &Value, names: &[&str]) -> Result<Vec<String>, String> {
    let values = dimensions
        .as_object()
        .ok_or("primitive solid has no typed dimensions")?;
    names
        .iter()
        .map(|name| {
            real(Some(
                values
                    .get(*name)
                    .ok_or("primitive solid omits a dimension")?,
            ))
        })
        .collect()
}

fn axis(values: &[Value]) -> Result<Vec<String>, String> {
    values.iter().map(|value| real(Some(value))).collect()
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
        "primitive_solids" => {
            let kind_field = record.field("kind");
            let kind = kind_field
                .as_ref()
                .and_then(Value::as_str)
                .ok_or_else(|| "primitive solid has no typed kind".to_owned())?;
            let type_code = match kind {
                "block" => 150,
                "right_angular_wedge" => 152,
                "right_circular_cylinder" => 154,
                "right_circular_cone_frustum" => 156,
                "sphere" => 158,
                "torus" => 160,
                "ellipsoid" => 168,
                _ => return Err("primitive solid has no exact typed kind".into()),
            };
            Ok((type_code, 0))
        }
        "procedural_solids" => {
            let kind_field = record.field("kind");
            let kind = kind_field
                .as_ref()
                .and_then(Value::as_str)
                .ok_or_else(|| "procedural solid has no typed kind".to_owned())?;
            match kind {
                "revolution" => Ok((162, form()?)),
                "linear_extrusion" => Ok((164, form()?)),
                _ => Err("procedural solid has no exact typed kind".into()),
            }
        }
        "boolean_trees" => Ok((180, form()?)),
        "selected_components" => Ok((182, 0)),
        "rectangular_arrays" => Ok((412, 0)),
        "circular_arrays" => Ok((414, 0)),
        _ => Err("record is not a constructive solid or array".into()),
    }
}

pub(super) fn encode(
    arena: &str,
    fields: &mut Fields<'_>,
    type_code: u32,
    form: i64,
) -> Result<Entity, String> {
    match arena {
        "primitive_solids" => {
            let table = fields
                .value("dimensions")
                .cloned()
                .ok_or("primitive solid has no typed dimensions")?;
            let mut values = match type_code {
                150 => dimensions(&table, &["x_length", "y_length", "z_length"])?,
                152 => dimensions(
                    &table,
                    &["x_length", "y_length", "z_length", "top_x_length"],
                )?,
                154 => dimensions(&table, &["height", "radius"])?,
                156 => dimensions(&table, &["height", "large_radius", "small_radius"])?,
                158 => dimensions(&table, &["radius"])?,
                160 => dimensions(&table, &["major_radius", "minor_radius"])?,
                168 => dimensions(&table, &["x_radius", "y_radius", "z_radius"])?,
                _ => return Err("primitive solid has no exact typed kind".into()),
            };
            let origin = fields
                .value("origin")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            values.extend(axis(&origin)?);
            for lane in ["x_axis", "z_axis"] {
                let present = match type_code {
                    150 | 152 | 168 => true,
                    154 | 156 | 160 => lane == "z_axis",
                    _ => false,
                };
                if !present {
                    continue;
                }
                match fields.value(lane).and_then(Value::as_array) {
                    Some(lane) if lane.len() == 3 => values.extend(axis(lane)?),
                    _ => values.extend([String::new(), String::new(), String::new()]),
                }
            }
            Ok(entity(type_code, form, values))
        }
        "procedural_solids" => {
            let profile = fields.value("profile").cloned();
            let mut values = vec![match profile {
                Some(value) if !value.is_null() => fields.reference(Some(&value))?,
                _ => "0".into(),
            }];
            values.push(real(fields.value("amount"))?);
            if type_code == 162 {
                let origin = fields
                    .value("origin")
                    .and_then(Value::as_array)
                    .filter(|values| values.len() == 3)
                    .cloned()
                    .unwrap_or_default();
                values.extend(axis(&origin)?);
            }
            let direction = fields
                .value("direction")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            values.extend(axis(&direction)?);
            Ok(entity(type_code, form, values))
        }
        "boolean_trees" => {
            let mut values = vec![integer(fields.value("declared_length"))?];
            for term in fields.array("terms")? {
                let table = term.as_object().ok_or("boolean term has no typed fields")?;
                match table.get("kind").and_then(Value::as_str) {
                    Some("operand") => values.push(operand(fields, &term)?),
                    Some("operation") => values.push(integer(table.get("operation"))?),
                    _ => return Err("boolean term has no typed kind".into()),
                }
            }
            Ok(entity(type_code, form, values))
        }
        "selected_components" => {
            let tree = fields.value("boolean_tree").cloned();
            let mut values = vec![match tree {
                Some(value) if !value.is_null() => fields.reference(Some(&value))?,
                _ => "0".into(),
            }];
            let point = fields
                .value("selection_point")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            values.extend(axis(&point)?);
            Ok(entity(type_code, form, values))
        }
        "rectangular_arrays" => {
            let base = fields.value("base").cloned();
            let mut values = vec![match base {
                Some(value) if !value.is_null() => fields.reference(Some(&value))?,
                _ => return Err("rectangular array has no typed base entity".into()),
            }];
            values.push(real(fields.value("scale"))?);
            let origin = fields
                .value("origin")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            values.extend(axis(&origin)?);
            values.push(integer(fields.value("columns"))?);
            values.push(integer(fields.value("rows"))?);
            values.push(real(fields.value("column_spacing"))?);
            values.push(real(fields.value("row_spacing"))?);
            values.push(real(fields.value("rotation"))?);
            let positions = fields.array("positions")?;
            values.push(positions.len().to_string());
            values.push(integer(fields.value("do_dont_flag"))?);
            for position in &positions {
                values.push(integer(Some(position))?);
            }
            Ok(entity(type_code, form, values))
        }
        "circular_arrays" => {
            let base = fields.value("base").cloned();
            let mut values = vec![match base {
                Some(value) if !value.is_null() => fields.reference(Some(&value))?,
                _ => return Err("circular array has no typed base entity".into()),
            }];
            values.push(integer(fields.value("location_count"))?);
            let center = fields
                .value("center")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            values.extend(axis(&center)?);
            values.push(real(fields.value("radius"))?);
            values.push(real(fields.value("start_angle"))?);
            values.push(real(fields.value("delta_angle"))?);
            let positions = fields.array("positions")?;
            values.push(positions.len().to_string());
            values.push(integer(fields.value("do_dont_flag"))?);
            for position in &positions {
                values.push(integer(Some(position))?);
            }
            Ok(entity(type_code, form, values))
        }
        _ => Err("record is not a constructive solid or array".into()),
    }
}
