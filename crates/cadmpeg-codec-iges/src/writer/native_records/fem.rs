// SPDX-License-Identifier: Apache-2.0
//! Types 134, 136, 138, 146, 148 and 418 are rebuilt from typed finite-element records.

use super::{count, entity, integer, real, Entity, Fields, NativeRecord, Value};

pub(super) fn shape(record: &NativeRecord) -> Result<(u32, i64), String> {
    let kind_field = record.field("kind");
    let kind = kind_field
        .as_ref()
        .and_then(Value::as_str)
        .ok_or("finite-element record has no typed kind")?;
    let form_field = record.field("form");
    let form = form_field
        .as_ref()
        .and_then(Value::as_i64)
        .ok_or("finite-element record has no typed form")?;
    match kind {
        "node" => Ok((134, form)),
        "finite_element" => Ok((136, form)),
        "nodal_displacement_rotation" => Ok((138, form)),
        "nodal_results" => Ok((146, form)),
        "element_results" => Ok((148, form)),
        "nodal_load_constraint" => Ok((418, form)),
        _ => Err("finite-element record has no typed kind".into()),
    }
}

fn subscript(fields: &Fields<'_>, name: &str) -> Result<u32, String> {
    match fields.value(name) {
        None => Ok(0),
        Some(value) => value
            .as_i64()
            .and_then(|number| u32::try_from(number).ok())
            .ok_or("entity subscript is not a Directory sequence".into()),
    }
}

fn pointer(fields: &mut Fields<'_>, value: Option<&Value>) -> Result<String, String> {
    match value {
        None => Ok("0".into()),
        Some(value) => {
            if value.is_null() {
                return Ok("0".into());
            }
            fields.reference(Some(value))
        }
    }
}

fn node_pointers(fields: &mut Fields<'_>, name: &str) -> Result<Vec<String>, String> {
    fields
        .array(name)?
        .iter()
        .map(|value| {
            if value.is_null() {
                return Ok("0".into());
            }
            if value.as_str().is_none() {
                return Err("node pointer has no typed identity".into());
            }
            fields.reference(Some(value))
        })
        .collect()
}

fn identifier(value: Option<&Value>) -> Result<String, String> {
    integer(value)
}

fn coordinates3(value: Option<&Value>) -> Result<Vec<String>, String> {
    value
        .and_then(Value::as_array)
        .filter(|values| values.len() == 3)
        .ok_or("node coordinates have no three-component lane")?
        .iter()
        .map(|value| real(Some(value)))
        .collect()
}

fn samples(fields: &mut Fields<'_>, name: &str) -> Result<Vec<Value>, String> {
    fields
        .array(name)
        .map_err(|_| format!("{name} has no typed sample list"))
}

fn sample_reals(
    sample: &serde_json::Map<String, Value>,
    name: &str,
) -> Result<Vec<String>, String> {
    sample
        .get(name)
        .and_then(Value::as_array)
        .ok_or("sample values have no typed list")?
        .iter()
        .map(|value| real(Some(value)))
        .collect()
}

pub(super) fn encode(fields: &mut Fields<'_>, type_code: u32, form: i64) -> Result<Entity, String> {
    let mut values = Vec::new();
    let mut subscript = 0u32;
    match (type_code, form) {
        (134, 0) => {
            if fields.value("definition_transformation").is_some() {
                return Err("a nodal definition transformation has no re-emitted Type 124".into());
            }
            values.extend(coordinates3(fields.value("coordinates"))?);
            values.push(fields.pointer("displacement_transformation")?);
            subscript = self::subscript(fields, "node_number")?;
        }
        (136, 0) => {
            values.push(fields.integer("topology_type")?);
            let nodes = fields.array("nodes")?;
            values.push(count(&fields.values, "declared_node_count", nodes.len())?);
            values.extend(node_pointers(fields, "nodes")?);
            values.push(fields.text("element_type")?);
            subscript = self::subscript(fields, "element_number")?;
        }
        (138, 0) => {
            let cases = fields.array("case_descriptions")?;
            values.push(count(&fields.values, "declared_case_count", cases.len())?);
            for case in &cases {
                values.push(pointer(fields, Some(case))?);
            }
            let nodes = samples(fields, "nodes")?;
            values.push(count(&fields.values, "declared_node_count", nodes.len())?);
            for node in &nodes {
                let node = node
                    .as_object()
                    .ok_or("displacement sample has no typed fields")?;
                values.push(identifier(node.get("identifier"))?);
                values.push(pointer(fields, node.get("node"))?);
                for lane in ["translations", "rotations"] {
                    let lane = node
                        .get(lane)
                        .and_then(Value::as_array)
                        .ok_or("displacement sample has no typed case lane")?;
                    if lane.len() != cases.len() {
                        return Err("displacement sample does not hold one entry per case".into());
                    }
                    for entry in lane {
                        let entry = entry
                            .as_array()
                            .filter(|components| components.len() == 3)
                            .ok_or("displacement sample entry has no three-component lane")?;
                        for component in entry {
                            values.push(real(Some(component))?);
                        }
                    }
                }
            }
        }
        (146, _) => {
            values.push(fields.pointer("analysis_note")?);
            values.push(fields.integer("subcase_number")?);
            values.push(fields.real("time")?);
            let count_value = fields
                .value("declared_value_count")
                .and_then(Value::as_i64)
                .ok_or("result record has no typed value count")?;
            let width = usize::try_from(count_value)
                .map_err(|_| "result value count is negative".to_owned())?;
            values.push(count_value.to_string());
            let nodes = samples(fields, "nodes")?;
            values.push(count(&fields.values, "declared_node_count", nodes.len())?);
            for node in &nodes {
                let node = node
                    .as_object()
                    .ok_or("result sample has no typed fields")?;
                values.push(identifier(node.get("identifier"))?);
                values.push(pointer(fields, node.get("node"))?);
                let sample = sample_reals(node, "values")?;
                if sample.len() != width {
                    return Err("result sample does not hold the declared value count".into());
                }
                values.extend(sample);
            }
            subscript = self::subscript(fields, "analysis_case_number")?;
        }
        (148, _) => {
            values.push(fields.pointer("analysis_note")?);
            values.push(fields.integer("subcase_number")?);
            values.push(fields.real("time")?);
            values.push(fields.integer("declared_value_count")?);
            values.push(fields.integer("result_report_flag")?);
            let elements = samples(fields, "elements")?;
            values.push(count(
                &fields.values,
                "declared_element_count",
                elements.len(),
            )?);
            for element in &elements {
                let element = element
                    .as_object()
                    .ok_or("element sample has no typed fields")?;
                values.push(identifier(element.get("identifier"))?);
                values.push(pointer(fields, element.get("element"))?);
                values.push(integer(element.get("topology_type"))?);
                values.push(integer(element.get("layers"))?);
                values.push(integer(element.get("data_layer_flag"))?);
                let locations = element
                    .get("report_locations")
                    .and_then(Value::as_array)
                    .ok_or("element sample has no typed report locations")?;
                let numbers = element
                    .get("declared_value_count")
                    .and_then(Value::as_i64)
                    .ok_or("element sample has no typed result count")?;
                let width = usize::try_from(numbers)
                    .map_err(|_| "element result count is negative".to_owned())?;
                values.push(locations.len().to_string());
                for location in locations {
                    values.push(integer(Some(location))?);
                }
                values.push(numbers.to_string());
                let sample = sample_reals(element, "values")?;
                if sample.len() != width {
                    return Err("element sample does not hold the declared result count".into());
                }
                values.extend(sample);
            }
            subscript = self::subscript(fields, "analysis_case_number")?;
        }
        (418, 0) => {
            let cases = fields.array("case_references")?;
            values.push(count(&fields.values, "declared_case_count", cases.len())?);
            values.push(fields.integer("load_constraint_type")?);
            values.push(fields.pointer("node")?);
            for case in &cases {
                values.push(pointer(fields, Some(case))?);
            }
        }
        _ => return Err("finite-element record has no exact typed parameter grammar".into()),
    }
    let mut emitted = entity(type_code, form, values);
    emitted.subscript = subscript;
    Ok(emitted)
}
