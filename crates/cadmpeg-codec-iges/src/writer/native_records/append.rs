// SPDX-License-Identifier: Apache-2.0
//! Order native definitions by their current typed references.

use super::*;

fn encode(arena: &str, record: &NativeRecord, targets: &Targets) -> Result<Entity, String> {
    let (type_code, form) = shape(arena, record)?;
    let mut fields = Fields {
        values: record.fields().map_err(|error| error.to_string())?,
        targets,
        references: BTreeSet::new(),
    };
    let mut result = match arena {
        "text_fonts" | "line_fonts" | "text_templates" => {
            fonts::encode(arena, &mut fields, type_code, form)?
        }
        "groups" | "associativities" => structure::encode(&mut fields, type_code, form)?,
        "flashes" => {
            let mut parameters = coordinates(fields.value("reference_point"), 2)?;
            parameters.extend([
                fields.real("dimension_1")?,
                fields.real("dimension_2")?,
                fields.real("rotation")?,
                fields.pointer("reference_entity")?,
            ]);
            entity(type_code, form, parameters)
        }
        "units_data" => {
            let units = fields.array("units")?;
            let mut parameters = vec![count(&fields.values, "declared_count", units.len())?];
            for unit in units {
                let unit = unit.as_object().ok_or("unit has no typed fields")?;
                parameters.extend([
                    text(unit.get("unit_type"))?,
                    text(unit.get("unit_value"))?,
                    real(unit.get("scale_factor"))?,
                ]);
            }
            entity(type_code, form, parameters)
        }
        _ => return Err("record has no native writer".into()),
    };
    result.source_targets.push(record.id().to_owned());
    if let Some(source) = fields.value("source_entity").and_then(Value::as_str) {
        result.source_targets.push(source.to_owned());
    }
    if let Some(number) = fields
        .value("fallback_line_font_number")
        .and_then(Value::as_i64)
    {
        if number < 0 {
            return Err("native definition has an unresolved fallback line font".into());
        }
        result.line_font = Some(DirectoryLineFont::Number(number));
    }
    if matches!(type_code, 304 | 310 | 312) {
        result.status = EntityStatus::Definition;
    }
    result
}

fn line_fonts(ir: &CadIr, entities: &mut [Entity], targets: &Targets) {
    let Some(namespace) = ir.native.namespace("iges") else {
        return;
    };
    for record in namespace
        .arenas()
        .get("display_attributes")
        .into_iter()
        .flatten()
    {
        let Some(source) = record.field("source_entity").and_then(Value::as_str) else {
            continue;
        };
        let font = if let Some(definition) =
            record.field("line_font_definition").and_then(Value::as_str)
        {
            let Some([index]) = targets.get(definition).map(Vec::as_slice) else {
                continue;
            };
            if entities
                .get(*index)
                .is_none_or(|entity| entity.type_code != 304)
            {
                continue;
            }
            DirectoryLineFont::Definition(*index)
        } else {
            let Some(number) = record
                .field("line_font_number")
                .and_then(Value::as_i64)
                .filter(|number| *number >= 0)
            else {
                continue;
            };
            DirectoryLineFont::Number(number)
        };
        if let Some(indices) = targets.get(source) {
            for index in indices {
                if let Some(entity) = entities.get_mut(*index) {
                    entity.line_font = Some(font);
                }
            }
        }
    }
}

pub(super) fn append(
    ir: &CadIr,
    entities: &mut Vec<Entity>,
    version: crate::IgesVersion,
) -> NativeExport {
    let mut targets = attributes::targets(ir, entities);
    let mut pending = ir
        .native
        .namespace("iges")
        .into_iter()
        .flat_map(|namespace| {
            namespace
                .arenas()
                .iter()
                .filter(|(arena, _)| ARENAS.contains(&arena.as_str()))
                .flat_map(|(arena, records)| {
                    records.iter().map(move |record| (arena.as_str(), record))
                })
        })
        .collect::<Vec<_>>();
    let mut result = NativeExport {
        regenerated: BTreeSet::new(),
        losses: Vec::new(),
    };
    loop {
        let before = pending.len();
        let mut deferred = Vec::new();
        for (arena, record) in pending {
            let Ok(entity) = encode(arena, record, &targets) else {
                deferred.push((arena, record));
                continue;
            };
            if !version.admits(&entity) {
                deferred.push((arena, record));
                continue;
            }
            let index = entities.len();
            for id in &entity.source_targets {
                targets.entry(id.clone()).or_default().push(index);
            }
            entities.push(entity);
            result.regenerated.insert(record.id().to_owned());
        }
        pending = deferred;
        if pending.len() == before {
            break;
        }
    }
    for (arena, record) in pending {
        let reason = encode(arena, record, &targets).map_or_else(
            |reason| reason,
            |_| {
                format!(
                    "the selected IGES {} dialect does not define this record",
                    version.name()
                )
            },
        );
        result
            .losses
            .push(IgesLossCode::PassthroughRecordOmitted.note(format!(
                "IGES native record {} is omitted: {reason}",
                record.id()
            )));
    }
    line_fonts(ir, entities, &targets);
    result
}
