// SPDX-License-Identifier: Apache-2.0
//! Exact Type 322/422 schema and row reconstruction from canonical attributes.

use super::{fields::Fields, *};
use cadmpeg_ir::native::NativeNamespace;
use serde_json::Value;

fn value(fields: &Fields<'_>, key: &str, kind: i64, targets: &Targets) -> Result<String, String> {
    let mut value = fields.one(key)?;
    if fields.has_native_values() {
        value = match (kind, value) {
            (4, Some(AttributeValue::Integer(sequence))) => {
                let sequence = u32::try_from(sequence)
                    .ok()
                    .filter(|sequence| sequence % 2 == 1)
                    .ok_or("native table pointer has no directory identity")?;
                Some(AttributeValue::Reference(format!(
                    "iges:entity:directory#{sequence}"
                )))
            }
            (6, Some(AttributeValue::Integer(0))) => Some(AttributeValue::Boolean(false)),
            (6, Some(AttributeValue::Integer(1))) => Some(AttributeValue::Boolean(true)),
            (_, value) => value,
        };
    }
    match (kind, value) {
        (0 | 5, None) => Ok(String::new()),
        (0 | 5, Some(_)) => Err(format!("no-value field {key} now holds a neutral value")),
        (_, None) => Err(format!("typed table field {key} has no current value")),
        (kind, Some(value)) => {
            let (actual, value) = typed_value(&value, targets)?;
            if actual == kind || (kind == 2 && actual == 1) {
                Ok(value)
            } else {
                Err(format!(
                    "table field {key} no longer has schema type {kind}"
                ))
            }
        }
    }
}

fn count(value: &Value, field: &str) -> Result<usize, String> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| format!("attribute schema has no bounded {field}"))
}

fn definition(
    record: &NativeRecord,
    fields: &Fields<'_>,
    targets: &Targets,
) -> Result<Entity, String> {
    let form = record
        .field("form")
        .and_then(|form| form.as_i64())
        .filter(|form| (0..=2).contains(form))
        .ok_or("attribute definition has no valid form")?;
    let name = string_field(record, "name")
        .map(|name| hollerith(&name))
        .transpose()?
        .unwrap_or_default();
    let list_type = record
        .field("attribute_list_type")
        .and_then(|value| value.as_i64())
        .filter(|value| (0..=9999).contains(value))
        .ok_or("attribute list classification is invalid")?;
    let attributes = record
        .field("attributes")
        .and_then(|value| value.as_array().cloned())
        .filter(|attributes| !attributes.is_empty())
        .ok_or("attribute definition has no descriptors")?;
    let mut parameters = vec![name, list_type.to_string(), attributes.len().to_string()];
    let mut identities = BTreeSet::new();
    for (index, attribute) in attributes.iter().enumerate() {
        let attribute_type = attribute
            .get("attribute_type")
            .and_then(Value::as_i64)
            .filter(|value| (0..=9999).contains(value))
            .ok_or("attribute descriptor type is invalid")?;
        if !identities.insert(attribute_type) {
            return Err("attribute descriptor types are not unique".into());
        }
        let kind = attribute
            .get("value_data_type")
            .and_then(Value::as_i64)
            .filter(|value| (0..=6).contains(value))
            .ok_or("attribute descriptor value type is invalid")?;
        let width = if attribute
            .get("declared_value_count")
            .is_none_or(Value::is_null)
        {
            1
        } else {
            count(attribute, "declared_value_count")?
        };
        parameters.extend([
            attribute_type.to_string(),
            kind.to_string(),
            width.to_string(),
        ]);
        if form == 0 {
            continue;
        }
        let templates = attribute.get("display_templates").and_then(Value::as_array);
        let original_values = attribute.get("values").and_then(Value::as_array);
        if templates
            .map(Vec::len)
            .or_else(|| original_values.map(Vec::len))
            != Some(width)
        {
            return Err("attribute default values do not fit the declared descriptor width".into());
        }
        for offset in 0..width {
            let key = format!("attributes.{index}.values.{offset}");
            parameters.push(value(fields, &key, kind, targets)?);
            if form == 2 {
                let template = templates
                    .and_then(|templates| templates.get(offset))
                    .or_else(|| original_values?.get(offset)?.get("display_template"));
                parameters.push(match template.and_then(Value::as_str) {
                    Some(template) => {
                        let [index] = targets
                            .get(template)
                            .ok_or("attribute display template was not emitted")?
                            .as_slice()
                        else {
                            return Err(
                                "attribute display template has no unique emitted identity".into(),
                            );
                        };
                        reference_marker(*index)
                    }
                    None => "0".into(),
                });
            }
        }
    }
    let mut entity = entity(322, form, format!("{};", parameters.join(",")));
    entity.status = EntityStatus::Definition;
    Ok(entity)
}

fn instance(
    record: &NativeRecord,
    definition: &NativeRecord,
    definition_index: usize,
    fields: &Fields<'_>,
    targets: &Targets,
) -> Result<Entity, String> {
    let form = record
        .field("form")
        .and_then(|form| form.as_i64())
        .filter(|form| (0..=1).contains(form))
        .ok_or("attribute instance has no valid form")?;
    if definition.field("form").and_then(|form| form.as_i64()) != Some(0) {
        return Err("attribute instance definition is not Form 0".into());
    }
    let descriptors = definition
        .field("attributes")
        .and_then(|value| value.as_array().cloned())
        .ok_or("attribute instance descriptor schema is absent")?;
    let widths = if let Some(widths) = record
        .field("row_widths")
        .and_then(|widths| widths.as_array().cloned())
    {
        widths
            .iter()
            .map(|width| {
                width
                    .as_u64()
                    .and_then(|width| usize::try_from(width).ok())
                    .ok_or("attribute row width is invalid")
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        record
            .field("rows")
            .and_then(|rows| {
                rows.as_array().map(|rows| {
                    rows.iter()
                        .filter_map(|row| row.as_array().map(Vec::len))
                        .collect()
                })
            })
            .ok_or("attribute rows are absent")?
    };
    if widths.is_empty() || (form == 0 && widths.len() != 1) {
        return Err("attribute instance row population is invalid".into());
    }
    let mut parameters = if form == 1 {
        vec![widths.len().to_string()]
    } else {
        Vec::new()
    };
    for (row, width) in widths.into_iter().enumerate() {
        let mut column = 0_usize;
        for descriptor in &descriptors {
            let kind = descriptor
                .get("value_data_type")
                .and_then(Value::as_i64)
                .ok_or("attribute value type is absent")?;
            let count = if descriptor
                .get("declared_value_count")
                .is_none_or(Value::is_null)
            {
                1
            } else {
                count(descriptor, "declared_value_count")?
            };
            let end = column
                .checked_add(count)
                .filter(|end| *end <= width)
                .ok_or("attribute descriptor exceeds its actual row width")?;
            while column < end {
                parameters.push(value(
                    fields,
                    &format!("rows.{row}.{column}"),
                    kind,
                    targets,
                )?);
                column += 1;
            }
        }
        if column != width {
            return Err("attribute row has values outside its current schema".into());
        }
    }
    let mut entity = entity(422, form, format!("{};", parameters.join(",")));
    entity.structure = Some(definition_index);
    entity.status = EntityStatus::Independent;
    Ok(entity)
}

pub(super) fn append(
    ir: &CadIr,
    namespace: &NativeNamespace,
    attributes: &BTreeMap<&str, &SourceAttribute>,
    entities: &mut Vec<Entity>,
    targets: &mut Targets,
    version: crate::IgesVersion,
    result: &mut AttributeExport,
) -> Result<(), CodecError> {
    let mut definitions = BTreeMap::new();
    for record in namespace
        .arenas()
        .get("attribute_table_definitions")
        .into_iter()
        .flatten()
    {
        let encoded = fields::groups(record, attributes, ir.scope.model).and_then(|groups| {
            let [fields] = groups.as_slice() else {
                return Err("attribute defaults have incompatible current owners".into());
            };
            let entity = definition(record, fields, targets)?;
            let index = append_owned(entities, entity, &[], version)?;
            Ok((index, fields.attribute_ids().collect::<Vec<_>>()))
        });
        match encoded {
            Ok((index, preserved)) => {
                definitions.insert(record.id().to_owned(), (record, index));
                result.regenerated.insert(record.id().to_owned());
                result.preserved.extend(preserved);
                targets
                    .entry(record.id().to_owned())
                    .or_default()
                    .push(index);
                if let Some(source) = string_field(record, "source_entity") {
                    targets.entry(source).or_default().push(index);
                }
            }
            Err(reason) => omitted(&mut result.losses, record.id(), reason),
        }
    }
    for record in namespace
        .arenas()
        .get("attribute_table_instances")
        .into_iter()
        .flatten()
    {
        let encoded = fields::groups(record, attributes, ir.scope.model).and_then(|groups| {
            let [fields] = groups.as_slice() else {
                return Err("attribute rows have incompatible current owners".into());
            };
            let definition_id =
                string_field(record, "definition").ok_or("attribute row definition is absent")?;
            let (definition, index) = definitions
                .get(&definition_id)
                .ok_or("attribute row definition is not emitted")?;
            let entity = instance(record, definition, *index, fields, targets)?;
            let index = append_owned(entities, entity, &[], version)?;
            Ok((index, fields.attribute_ids().collect::<Vec<_>>()))
        });
        match encoded {
            Ok((index, preserved)) => {
                result.regenerated.insert(record.id().to_owned());
                result.preserved.extend(preserved);
                targets
                    .entry(record.id().to_owned())
                    .or_default()
                    .push(index);
                if let Some(source) = string_field(record, "source_entity") {
                    targets.entry(source).or_default().push(index);
                }
            }
            Err(reason) => omitted(&mut result.losses, record.id(), reason),
        }
    }
    Ok(())
}
