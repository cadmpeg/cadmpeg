// SPDX-License-Identifier: Apache-2.0
//! Type 406 field grammars, regenerated from current canonical values.

use super::{
    AttributeValue, BTreeSet, Entity, NativeRecord, Targets, entity, fields::Fields,
    fields::collection_size, hollerith, typed_value,
};
use serde_json::Value;

fn integer(fields: &Fields<'_>, key: &str) -> Result<String, String> {
    Ok(fields
        .integer(key)?
        .map(|value| value.to_string())
        .unwrap_or_default())
}
fn real(fields: &Fields<'_>, key: &str) -> Result<String, String> {
    match fields.one(key)? {
        None => Ok(String::new()),
        Some(AttributeValue::Float(value)) => Ok(super::super::number(value)),
        Some(AttributeValue::Integer(value)) => Ok(value.to_string()),
        _ => Err(format!("field {key} is not real-valued")),
    }
}
fn text(fields: &Fields<'_>, key: &str) -> Result<String, String> {
    match fields.one(key)? {
        None => Ok(String::new()),
        Some(AttributeValue::String(value)) => hollerith(&value),
        _ => Err(format!("field {key} is not text-valued")),
    }
}

pub(super) fn encode(
    record: &NativeRecord,
    fields: &Fields<'_>,
    targets: &Targets,
) -> Result<Entity, String> {
    let form = record
        .field("form")
        .and_then(|form| form.as_i64())
        .ok_or("property has no typed form")?;
    let mut values = Vec::new();
    macro_rules! ints { ($($key:literal),+ $(,)?) => {{ $(values.push(integer(fields, $key)?);)+ }}; }
    macro_rules! reals { ($($key:literal),+ $(,)?) => {{ $(values.push(real(fields, $key)?);)+ }}; }
    macro_rules! texts { ($($key:literal),+ $(,)?) => {{ $(values.push(text(fields, $key)?);)+ }}; }
    match form {
        2 => ints!(
            "electrical_vias",
            "electrical_components",
            "electrical_circuitry"
        ),
        3 => {
            ints!("function_code");
            texts!("description");
        }
        4 => ints!("fill_code", "obsolete_pointer"),
        5 => {
            reals!("width");
            ints!("cornering", "extension_flag", "justification");
            reals!("extension");
        }
        6 => {
            reals!("drill_diameter", "finished_diameter");
            ints!("plated", "lower_layer", "upper_layer");
        }
        7 | 8 => texts!("value"),
        15 => values.push(text(
            fields,
            if fields.name("").is_some() {
                ""
            } else {
                "value"
            },
        )?),
        9 => texts!("generic", "military", "vendor", "internal"),
        10 => ints!(
            "line_font",
            "view",
            "level",
            "blank",
            "line_weight",
            "color"
        ),
        11 => {
            let independent = collection_size(record, "independent_variables")?;
            let dependent = fields
                .integer("declared_dependent_count")?
                .and_then(|count| usize::try_from(count).ok())
                .filter(|count| *count > 0)
                .ok_or("tabular property has no positive dependent count")?;
            ints!("property_type");
            values.extend([dependent.to_string(), independent.to_string()]);
            for index in 0..independent {
                values.push(integer(
                    fields,
                    &format!("independent_variables.{index}.variable_type"),
                )?);
            }
            let mut point_count = 1_usize;
            for index in 0..independent {
                let count =
                    collection_size(record, &format!("independent_variables.{index}.values"))?;
                if count == 0 {
                    return Err("tabular independent variable has no values".into());
                }
                point_count = point_count
                    .checked_mul(count)
                    .ok_or("tabular value count overflows")?;
                values.push(count.to_string());
            }
            for index in 0..independent {
                for offset in
                    0..collection_size(record, &format!("independent_variables.{index}.values"))?
                {
                    values.push(real(
                        fields,
                        &format!("independent_variables.{index}.values.{offset}"),
                    )?);
                }
            }
            let dependent_values = collection_size(record, "dependent_values")?;
            if point_count.checked_mul(dependent) != Some(dependent_values) {
                return Err("tabular values do not fit the current independent grid".into());
            }
            for index in 0..dependent_values {
                values.push(real(fields, &format!("dependent_values.{index}"))?);
            }
        }
        12 => {
            for index in 0..collection_size(record, "names")? {
                values.push(text(fields, &format!("names.{index}"))?);
            }
        }
        13 => {
            reals!("size");
            texts!("name", "standard");
        }
        14 => {
            for index in 0..collection_size(record, "values")? {
                values.push(text(fields, &format!("values.{index}"))?);
            }
        }
        18 => reals!("percent"),
        19 => ints!("pattern_code"),
        20 => values.push(fields.boolean("highlighted", false)?),
        21 => values.push(fields.boolean("pickable", true)?),
        22 => {
            values.extend([
                fields.boolean("finite", false)?,
                fields.boolean("lines", false)?,
                fields.boolean("weighted", true)?,
            ]);
            for key in ["origin.0", "origin.1", "spacing.0", "spacing.1"] {
                values.push(real(fields, key)?);
            }
            for key in ["counts.0", "counts.1"] {
                values.push(integer(fields, key)?);
            }
        }
        23 => {
            ints!("associativity_type");
            texts!("name");
        }
        24 => {
            let count = collection_size(record, "definitions")?;
            values.push(count.to_string());
            for index in 0..count {
                values.push(integer(
                    fields,
                    &format!("definitions.{index}.exchange_level"),
                )?);
                values.push(text(
                    fields,
                    &format!("definitions.{index}.native_identifier"),
                )?);
                values.push(integer(
                    fields,
                    &format!("definitions.{index}.physical_layer"),
                )?);
                values.push(text(
                    fields,
                    &format!("definitions.{index}.functional_identifier"),
                )?);
            }
        }
        25 => {
            texts!("identification");
            let count = collection_size(record, "levels")?;
            values.push(count.to_string());
            for index in 0..count {
                values.push(integer(fields, &format!("levels.{index}"))?);
            }
        }
        26 => {
            reals!("drill_diameter", "finished_diameter");
            ints!("function_code");
        }
        27 => return generic(record, fields, targets),
        28 => {
            ints!("secondary_position", "units_indicator", "character_set");
            texts!("suffix");
            ints!("fraction_flag", "precision");
        }
        29 => {
            ints!("secondary_flag", "tolerance_type", "placement");
            reals!("upper", "lower");
            values.push(fields.boolean("suppress_plus", false)?);
            ints!("fraction_flag", "precision");
        }
        30 => {
            ints!("dimension_type", "label_position", "character_set");
            texts!("label");
            ints!("decimal_symbol");
            reals!("witness_line_angle");
            ints!(
                "text_alignment",
                "text_level",
                "text_placement",
                "arrow_orientation"
            );
            reals!("initial_value");
            let count = collection_size(record, "supplemental_notes")?;
            values.push(count.to_string());
            for index in 0..count {
                for key in ["position", "first_text", "last_text"] {
                    values.push(integer(
                        fields,
                        &format!("supplemental_notes.{index}.{key}"),
                    )?);
                }
            }
        }
        31 => {
            for index in 0..4 {
                for coordinate in 0..2 {
                    values.push(real(fields, &format!("corners.{index}.{coordinate}"))?);
                }
            }
        }
        32 => texts!("name", "organization", "date"),
        33 => {
            ints!("sheet_number");
            texts!("revision");
        }
        34 | 35 => {
            let count = collection_size(record, "ranges")?;
            values.push(count.to_string());
            for index in 0..count {
                for key in ["text_index", "first_character", "last_character"] {
                    values.push(integer(fields, &format!("ranges.{index}.{key}"))?);
                }
            }
        }
        36 => {
            ints!("u");
            if fields.one("v")?.is_some() {
                ints!("v");
            }
        }
        _ => {
            return Err(format!(
                "Type 406 Form {form} has no typed property grammar"
            ));
        }
    }
    let mut parameters = vec![values.len().to_string()];
    parameters.extend(values);
    Ok(entity(406, form, format!("{};", parameters.join(","))))
}

fn generic(
    record: &NativeRecord,
    fields: &Fields<'_>,
    targets: &Targets,
) -> Result<Entity, String> {
    let (name, values) = if let Some(name) = fields.name("") {
        (name.to_owned(), fields.values("")?)
    } else {
        let Some(AttributeValue::String(name)) = fields.one("name")? else {
            return Err("generic attribute has no current name".into());
        };
        let mut values = Vec::new();
        for index in 0..collection_size(record, "values")? {
            if let Some(mut value) = fields.one(&format!("values.{index}.value"))? {
                match fields.integer(&format!("values.{index}.data_type"))? {
                    Some(6) => {
                        value = match value {
                            AttributeValue::Integer(0) => AttributeValue::Boolean(false),
                            AttributeValue::Integer(1) => AttributeValue::Boolean(true),
                            _ => return Err("native logical attribute is not 0 or 1".into()),
                        };
                    }
                    Some(4) => {
                        value = match value {
                            AttributeValue::Integer(sequence) => AttributeValue::Reference(
                                format!("iges:entity:directory#{sequence}"),
                            ),
                            _ => {
                                return Err(
                                    "native entity attribute is not a directory pointer".into()
                                );
                            }
                        };
                    }
                    _ => {}
                }
                values.push(value);
            }
        }
        (name, values)
    };
    if name.trim().is_empty() {
        return Err("generic attribute has no current name".into());
    }
    let mut pairs = values
        .iter()
        .map(|value| typed_value(value, targets))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(schema) = record
        .field("values")
        .and_then(|values| values.as_array().cloned())
    {
        let omitted: BTreeSet<_> = schema
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                (slot
                    .get("value")
                    .and_then(|value| value.get("kind"))
                    .and_then(Value::as_str)
                    == Some("omitted"))
                .then_some(index)
            })
            .collect();
        if !omitted.is_empty() {
            if schema.len().checked_sub(omitted.len()) != Some(pairs.len()) {
                return Err(
                    "generic values no longer fit their typed no-value schema slots".into(),
                );
            }
            let mut current = pairs.into_iter();
            pairs = schema
                .iter()
                .enumerate()
                .map(|(index, slot)| {
                    if omitted.contains(&index) {
                        let kind = slot
                            .get("data_type")
                            .and_then(Value::as_i64)
                            .filter(|kind| matches!(kind, 0 | 5))
                            .ok_or("omitted generic slot does not have a no-value type")?;
                        Ok((kind, String::new()))
                    } else {
                        current.next().ok_or("generic current value slot is absent")
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
    }
    if pairs.is_empty() {
        pairs.push((0, String::new()));
    }
    let mut parameters = vec![
        (2 + 2 * pairs.len()).to_string(),
        hollerith(&name)?,
        pairs.len().to_string(),
    ];
    for (kind, value) in pairs {
        parameters.extend([kind.to_string(), value]);
    }
    Ok(entity(406, 27, format!("{};", parameters.join(","))))
}
