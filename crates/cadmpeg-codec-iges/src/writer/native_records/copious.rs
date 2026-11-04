// SPDX-License-Identifier: Apache-2.0
//! Type 106 copious data rebuilt from typed records.

use super::{entity, integer, real, Entity, Fields, Value};

/// The tuple width the decoder reads for a form and interpretation flag,
/// mirroring its layout table: pairs carry a common Z displacement, triples
/// and sextuples do not.
fn pair_form(form: i64) -> bool {
    matches!(form, 1 | 11 | 20 | 21 | 31..=38 | 40 | 63)
}

pub(super) fn shape(record: &super::NativeRecord) -> Result<(u32, i64), String> {
    let form = record
        .field("form")
        .as_ref()
        .and_then(Value::as_i64)
        .ok_or("record has no typed form".to_owned())?;
    Ok((106, form))
}

pub(super) fn encode(fields: &mut Fields<'_>, type_code: u32, form: i64) -> Result<Entity, String> {
    let interpretation_value = match fields.value("interpretation") {
        Some(value) if !value.is_null() => value
            .as_i64()
            .ok_or("copious data interpretation flag is not an integer".to_owned())?,
        _ => return Err("copious data has no interpretation flag".into()),
    };
    let tuples = fields.array("tuples")?;
    let mut values = vec![
        interpretation_value.to_string(),
        match fields.value("declared_tuple_count") {
            Some(value) if !value.is_null() => integer(Some(value))?,
            _ => tuples.len().to_string(),
        },
    ];
    if interpretation_value == 1 && pair_form(form) {
        values.push(real(fields.value("common_z"))?);
    }
    for tuple in &tuples {
        let tuple = tuple
            .as_array()
            .ok_or("copious data tuple has no components")?;
        for component in tuple {
            values.push(real(Some(component))?);
        }
    }
    Ok(entity(type_code, form, values))
}
