// SPDX-License-Identifier: Apache-2.0
//! Types 202 through 230 rebuilt from typed annotation records.

use super::{entity, integer, real, text, Entity, Fields, Value};

/// A reference that is zero when the typed record holds no reference.
fn optional_pointer(fields: &mut Fields<'_>, value: Option<&Value>) -> Result<String, String> {
    match value {
        Some(value) if !value.is_null() => fields.reference(Some(value)),
        _ => Ok("0".into()),
    }
}

fn text_or_empty(value: Option<&Value>) -> Result<String, String> {
    match value {
        Some(value) if !value.is_null() => text(Some(value)),
        _ => Ok(String::new()),
    }
}

/// The declared count exactly as admitted, falling back to the current
/// typed member count when the declaration is missing.
fn declared_count(value: Option<&Value>, actual: usize) -> Result<String, String> {
    match value {
        Some(value) if !value.is_null() => integer(Some(value)),
        _ => Ok(actual.to_string()),
    }
}

fn text_run(fields: &mut Fields<'_>, values: &mut Vec<String>, run: &Value) -> Result<(), String> {
    let run = run
        .as_object()
        .ok_or("annotation text has no typed fields")?;
    values.push(integer(run.get("declared_character_count"))?);
    let size = run
        .get("box_size")
        .and_then(Value::as_array)
        .filter(|values| values.len() == 2)
        .cloned()
        .unwrap_or_default();
    for component in &size {
        values.push(real(Some(component))?);
    }
    match run.get("font_definition").filter(|value| !value.is_null()) {
        Some(definition) => values.push(format!("-{}", fields.reference(Some(definition))?)),
        None => values.push(integer(run.get("font_code"))?),
    }
    values.push(real(run.get("slant_angle"))?);
    values.push(real(run.get("rotation_angle"))?);
    values.push(integer(run.get("mirror"))?);
    values.push(integer(run.get("vertical"))?);
    let start = run
        .get("start")
        .and_then(Value::as_array)
        .filter(|values| values.len() == 3)
        .cloned()
        .unwrap_or_default();
    for component in &start {
        values.push(real(Some(component))?);
    }
    values.push(text_or_empty(run.get("text"))?);
    Ok(())
}

pub(super) fn shape(arena: &str, record: &super::NativeRecord) -> Result<(u32, i64), String> {
    let form = || -> Result<i64, String> {
        record
            .field("form")
            .as_ref()
            .and_then(Value::as_i64)
            .ok_or("record has no typed form".into())
    };
    if arena != "annotations" {
        return Err("record is not an annotation".into());
    }
    let kind_field = record.field("kind");
    let kind = kind_field
        .as_ref()
        .and_then(Value::as_str)
        .ok_or_else(|| "annotation has no typed kind".to_owned())?;
    match kind {
        "general_note" => Ok((212, form()?)),
        "new_general_note" => Ok((213, 0)),
        "leader" => Ok((214, form()?)),
        "angular_dimension" => Ok((202, 0)),
        "curve_dimension" => Ok((204, 0)),
        "diameter_dimension" => Ok((206, 0)),
        "flag_note" => Ok((208, 0)),
        "general_label" => Ok((210, 0)),
        "linear_dimension" => Ok((216, form()?)),
        "ordinate_dimension" => Ok((218, form()?)),
        "point_dimension" => Ok((220, 0)),
        "radius_dimension" => Ok((222, form()?)),
        "general_symbol" => Ok((228, form()?)),
        "sectioned_area" => Ok((230, form()?)),
        _ => Err("annotation has no typed kind".into()),
    }
}

pub(super) fn encode(
    arena: &str,
    fields: &mut Fields<'_>,
    type_code: u32,
    form: i64,
) -> Result<Entity, String> {
    if arena != "annotations" {
        return Err("record is not an annotation".into());
    }
    let kind = fields
        .value("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "annotation has no typed kind".to_owned())?
        .to_owned();
    let mut values = Vec::new();
    match kind.as_str() {
        "general_note" => {
            let strings = fields.array("strings")?;
            values.push(declared_count(
                fields.value("declared_string_count"),
                strings.len(),
            )?);
            for run in &strings {
                text_run(fields, &mut values, run)?;
            }
        }
        "new_general_note" => {
            let size = fields
                .value("containment_size")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 2)
                .cloned()
                .unwrap_or_default();
            for component in &size {
                values.push(real(Some(component))?);
            }
            values.push(integer(fields.value("justification"))?);
            let origin = fields
                .value("containment_origin")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            for component in &origin {
                values.push(real(Some(component))?);
            }
            values.push(real(fields.value("containment_angle"))?);
            let baseline = fields
                .value("baseline_origin")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            for component in &baseline {
                values.push(real(Some(component))?);
            }
            values.push(real(fields.value("normal_interline_spacing"))?);
            let strings = fields.array("strings")?;
            values.push(declared_count(
                fields.value("declared_string_count"),
                strings.len(),
            )?);
            for run in &strings {
                let run = run
                    .as_object()
                    .ok_or("annotation text has no typed fields")?;
                values.push(integer(run.get("fixed_or_variable"))?);
                let size = run
                    .get("character_size")
                    .and_then(Value::as_array)
                    .filter(|values| values.len() == 2)
                    .cloned()
                    .unwrap_or_default();
                for component in &size {
                    values.push(real(Some(component))?);
                }
                values.push(real(run.get("character_spacing"))?);
                values.push(real(run.get("line_spacing"))?);
                values.push(integer(run.get("font_style"))?);
                values.push(real(run.get("character_angle"))?);
                values.push(text_or_empty(run.get("control_codes"))?);
                text_run(
                    fields,
                    &mut values,
                    run.get("text")
                        .ok_or("annotation text has no typed block")?,
                )?;
            }
        }
        "leader" => {
            let tails = fields
                .value("segment_tails")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            values.push(declared_count(
                fields.value("declared_segment_count"),
                tails.len(),
            )?);
            let size = fields
                .value("arrowhead_size")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 2)
                .cloned()
                .unwrap_or_default();
            for component in &size {
                values.push(real(Some(component))?);
            }
            let head = fields
                .value("arrowhead")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            let depth = head.get(2).cloned().unwrap_or(Value::Null);
            values.push(real(Some(&depth))?);
            for component in head.iter().take(2) {
                values.push(real(Some(component))?);
            }
            for tail in &tails {
                let tail = tail
                    .as_array()
                    .filter(|values| values.len() == 3)
                    .cloned()
                    .unwrap_or_default();
                for component in tail.iter().take(2) {
                    values.push(real(Some(component))?);
                }
            }
        }
        "flag_note" => {
            let origin = fields
                .value("origin")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            for component in &origin {
                values.push(real(Some(component))?);
            }
            values.push(real(fields.value("rotation"))?);
            values.push({
                let held = fields.value("note").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
            let leaders = fields.array("leaders")?;
            values.push(declared_count(
                fields.value("declared_leader_count"),
                leaders.len(),
            )?);
            for leader in &leaders {
                values.push(optional_pointer(fields, Some(leader))?);
            }
        }
        "general_label" => {
            values.push({
                let held = fields.value("note").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
            let leaders = fields.array("leaders")?;
            values.push(declared_count(
                fields.value("declared_leader_count"),
                leaders.len(),
            )?);
            for leader in &leaders {
                values.push(optional_pointer(fields, Some(leader))?);
            }
        }
        "linear_dimension" | "angular_dimension" | "curve_dimension" | "diameter_dimension" => {
            values.push({
                let held = fields.value("note").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
            values.extend(dimension_links(fields, &kind)?);
        }
        "ordinate_dimension" => {
            values.push({
                let held = fields.value("note").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
            values.push({
                let held = fields.value("ordinate").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
            if form == 1 {
                values.push({
                    let held = fields.value("supplemental_leader").cloned();
                    optional_pointer(fields, held.as_ref())
                }?);
            }
        }
        "point_dimension" => {
            values.push({
                let held = fields.value("note").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
            values.push({
                let held = fields.value("leader").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
            values.push({
                let held = fields.value("enclosure").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
        }
        "radius_dimension" => {
            values.push({
                let held = fields.value("note").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
            let leaders = leaders_pair(fields);
            values.push(optional_pointer(fields, leaders[0].as_ref())?);
            let center = fields
                .value("center")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 2)
                .cloned()
                .unwrap_or_default();
            for component in &center {
                values.push(real(Some(component))?);
            }
            if form == 1 {
                values.push(optional_pointer(fields, leaders[1].as_ref())?);
            }
        }
        "general_symbol" => {
            values.push({
                let held = fields.value("note").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
            let geometry = fields.array("geometry")?;
            values.push(declared_count(
                fields.value("declared_geometry_count"),
                geometry.len(),
            )?);
            for member in &geometry {
                values.push(optional_pointer(fields, Some(member))?);
            }
            let leaders = fields.array("leaders")?;
            values.push(declared_count(
                fields.value("declared_leader_count"),
                leaders.len(),
            )?);
            for leader in &leaders {
                values.push(optional_pointer(fields, Some(leader))?);
            }
        }
        "sectioned_area" => {
            values.push({
                let held = fields.value("boundary").cloned();
                optional_pointer(fields, held.as_ref())
            }?);
            values.push(integer(fields.value("fill_pattern"))?);
            let anchor = fields
                .value("pattern_anchor")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 3)
                .cloned()
                .unwrap_or_default();
            for component in &anchor {
                values.push(real(Some(component))?);
            }
            values.push(real(fields.value("pattern_spacing"))?);
            values.push(real(fields.value("pattern_angle"))?);
            let islands = fields.array("islands")?;
            values.push(declared_count(
                fields.value("declared_island_count"),
                islands.len(),
            )?);
            for island in &islands {
                values.push(optional_pointer(fields, Some(island))?);
            }
        }
        _ => return Err("annotation has no typed parameter grammar".into()),
    }
    Ok(entity(type_code, form, values))
}

/// The dimension links after the note pointer, in parameter order.
fn dimension_links(fields: &mut Fields<'_>, kind: &str) -> Result<Vec<String>, String> {
    let mut values = Vec::new();
    let pair = |fields: &mut Fields<'_>, name: &str| -> Result<[String; 2], String> {
        let items = fields
            .value(name)
            .and_then(Value::as_array)
            .filter(|values| values.len() == 2)
            .cloned()
            .unwrap_or_default();
        let mut pair = [String::new(), String::new()];
        for (index, item) in items.iter().take(2).enumerate() {
            pair[index] = optional_pointer(fields, Some(item))?;
        }
        Ok(pair)
    };
    match kind {
        "angular_dimension" => {
            let witnesses = pair(fields, "witnesses")?;
            values.extend(witnesses);
            let vertex = fields
                .value("vertex")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 2)
                .cloned()
                .unwrap_or_default();
            for component in &vertex {
                values.push(real(Some(component))?);
            }
            values.push(real(fields.value("radius"))?);
            let leaders = pair(fields, "leaders")?;
            values.extend(leaders);
        }
        "curve_dimension" => {
            let curves = pair(fields, "curves")?;
            values.extend(curves);
            let leaders = pair(fields, "leaders")?;
            values.extend(leaders);
            let witnesses = pair(fields, "witnesses")?;
            values.extend(witnesses);
        }
        "diameter_dimension" => {
            let leaders = pair(fields, "leaders")?;
            values.extend(leaders);
            let center = fields
                .value("center")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 2)
                .cloned()
                .unwrap_or_default();
            for component in &center {
                values.push(real(Some(component))?);
            }
        }
        "linear_dimension" => {
            let leaders = pair(fields, "leaders")?;
            values.extend(leaders);
            let witnesses = pair(fields, "witnesses")?;
            values.extend(witnesses);
        }
        _ => return Err("dimension has no typed link grammar".into()),
    }
    Ok(values)
}

/// The two leader slots of a radius dimension, in parameter order.
fn leaders_pair(fields: &Fields<'_>) -> [Option<Value>; 2] {
    let leaders = fields
        .value("leaders")
        .and_then(Value::as_array)
        .filter(|values| values.len() == 2)
        .cloned()
        .unwrap_or_default();
    let mut pair = [None, None];
    for (index, leader) in leaders.into_iter().take(2).enumerate() {
        pair[index] = Some(leader);
    }
    pair
}

/// Apply the current neutral text to a native note's formatted text runs.
pub(super) fn apply_text(
    ir: &cadmpeg_ir::CadIr,
    record: &super::NativeRecord,
    fields: &mut Fields<'_>,
) -> Result<Option<String>, String> {
    let mut matching = ir
        .model
        .semantic_annotations
        .iter()
        .filter(|annotation| annotation.native_ref == record.id());
    let Some(annotation) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        return Err("multiple semantic annotations name one native note".into());
    }
    let kind = fields.value("kind").and_then(Value::as_str);
    if !matches!(kind, Some("general_note" | "new_general_note")) {
        return Ok(Some(annotation.id.to_string()));
    }
    let nested = kind == Some("new_general_note");
    let Some(runs) = fields
        .values
        .get_mut("strings")
        .and_then(Value::as_array_mut)
    else {
        return Err("current note has no formatted text runs".into());
    };
    if annotation.text.len() != runs.len() {
        if annotation.text.is_empty()
            && runs.iter().all(|run| {
                let run = if nested {
                    run.get("text").unwrap_or(&Value::Null)
                } else {
                    run
                };
                run.get("text").is_none_or(Value::is_null)
            })
        {
            return Ok(Some(annotation.id.to_string()));
        }
        return Err("current note text does not match its formatted runs".into());
    }
    let single_run = runs.len() == 1;
    for (run, text) in runs.iter_mut().zip(&annotation.text) {
        if text.is_empty()
            || ((single_run || nested) && text == " ")
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        {
            return Err("current note text has no exact printable IGES spelling".into());
        }
        let run = if nested {
            run.get_mut("text")
        } else {
            Some(run)
        };
        let run = run
            .and_then(Value::as_object_mut)
            .ok_or("current note has an invalid formatted run")?;
        if run.get("font_code").and_then(Value::as_i64) == Some(2001) {
            let bytes = text.as_bytes();
            if !bytes.len().is_multiple_of(4)
                || !bytes.chunks_exact(4).all(|character| {
                    character.chunks_exact(2).all(|pair| {
                        std::str::from_utf8(pair)
                            .ok()
                            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                            .is_some_and(|value| (0x21..=0x7e).contains(&value))
                    })
                })
            {
                return Err("current note text violates the IGES font 2001 encoding".into());
            }
        }
        run.insert("declared_character_count".into(), Value::from(text.len()));
        run.insert(
            "text".into(),
            serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::from(
                text.as_bytes().to_vec(),
            ))
            .map_err(|error| error.to_string())?,
        );
    }
    Ok(Some(annotation.id.to_string()))
}
