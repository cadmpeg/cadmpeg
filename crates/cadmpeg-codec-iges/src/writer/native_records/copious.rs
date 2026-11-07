// SPDX-License-Identifier: Apache-2.0
//! Type 106 copious data rebuilt from typed records.

use super::{entity, integer, real, Entity, Fields, Value};

pub(in crate::writer) fn matching_points(
    ir: &super::CadIr,
    record: &super::NativeRecord,
) -> Option<Vec<String>> {
    if record.field("form")?.as_i64()? != 3 || record.field("interpretation")?.as_i64()? != 3 {
        return None;
    }
    if !matches!(
        super::source_status(ir, record),
        Some(super::EntityStatus::Independent)
    ) {
        return None;
    }
    let source = record.field("source_entity")?;
    let source = source.as_str()?;
    let sequence = source.strip_prefix("iges:entity:directory#")?;
    let directory = ir
        .native
        .namespace("iges")?
        .arenas()
        .get("entities")?
        .iter()
        .find(|row| row.id() == source)?;
    if directory.field("transform")?.as_i64()? != 0 {
        return None;
    }
    let tuples = record.field("tuples")?;
    let tuples = tuples.as_array()?;
    if record.field("declared_tuple_count")?.as_u64()?
        != cadmpeg_core::decode::u64_from_index(tuples.len())
    {
        return None;
    }
    let mut ids = Vec::new();
    for (index, tuple) in tuples.iter().enumerate() {
        let values = tuple.as_array()?;
        if values.len() != 6 {
            return None;
        }
        let id = format!("iges:model:point#D{sequence}-{}", index + 1);
        let point = ir
            .model
            .points
            .iter()
            .find(|point| point.id.as_str() == id)?;
        let [x, y, z] = point
            .position()
            .coordinates()
            .map(cadmpeg_ir::scalar::FiniteReal::get);
        if values[0].as_f64()? != x || values[1].as_f64()? != y || values[2].as_f64()? != z {
            return None;
        }
        ids.push(id);
    }
    Some(ids)
}

pub(in crate::writer) fn contains_point(ir: &super::CadIr, id: &str) -> bool {
    ir.native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("copious_data"))
        .into_iter()
        .flatten()
        .any(|record| {
            matching_points(ir, record)
                .is_some_and(|ids| ids.iter().any(|candidate| candidate == id))
        })
}

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
