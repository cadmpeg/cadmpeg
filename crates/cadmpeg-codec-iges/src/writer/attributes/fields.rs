// SPDX-License-Identifier: Apache-2.0
//! Resolve current neutral attribute references or out-of-scope typed values.

use super::{
    AttributeTarget, AttributeValue, BTreeMap, NativeRecord, SourceAttribute, string_field,
};
use serde_json::Value;

pub(super) struct Fields<'a> {
    pub(super) owner: Option<AttributeTarget>,
    values: BTreeMap<String, &'a SourceAttribute>,
    native: Option<Value>,
}

impl Fields<'_> {
    pub(super) fn has_native_values(&self) -> bool {
        self.native.is_some()
    }
    pub(super) fn is_self_owned(&self, record: &NativeRecord) -> bool {
        self.owner.is_none()
            || matches!(&self.owner, Some(AttributeTarget::Source { source_id })
            if source_id == record.id() || Some(source_id.as_str()) == string_field(record, "source_entity").as_deref())
    }
    pub(super) fn attribute_ids(&self) -> impl Iterator<Item = String> + '_ {
        self.values
            .values()
            .map(|attribute| attribute.id.to_string())
    }
    pub(super) fn name(&self, field: &str) -> Option<&str> {
        self.values
            .get(field)
            .map(|attribute| attribute.name.as_str())
    }
    pub(super) fn values(&self, field: &str) -> Result<Vec<AttributeValue>, String> {
        if let Some(attribute) = self.values.get(field) {
            return Ok(attribute.values.clone());
        }
        let Some(value) = self.native_value(field) else {
            return Ok(Vec::new());
        };
        if value.is_null() {
            return Ok(Vec::new());
        }
        let value = if value.get("display_template").is_some() {
            value
                .get("value")
                .ok_or("native attribute default has no value slot")?
        } else {
            value
        };
        let value = if value.get("kind").and_then(Value::as_str).is_some() {
            match value.get("kind").and_then(Value::as_str) {
                Some("omitted") => return Ok(Vec::new()),
                Some("integer" | "real" | "string") => value
                    .get("value")
                    .ok_or("typed native literal has no value")?,
                _ => return Err("native literal has no interpreted value".into()),
            }
        } else {
            value
        };
        let value = if let Some(value) = value.as_i64() {
            AttributeValue::Integer(value)
        } else if let Some(value) = value.as_f64() {
            AttributeValue::float(value).ok_or("native number is not finite")?
        } else if let Some(value) = value.as_str() {
            AttributeValue::String(
                String::from_utf8(crate::text::field(&Value::String(value.to_owned()))?)
                    .map_err(|_| "native text is not UTF-8")?,
            )
        } else if let Some(value) = value.as_bool() {
            AttributeValue::Boolean(value)
        } else {
            return Err("native field has no scalar semantic value".into());
        };
        Ok(vec![value])
    }
    pub(super) fn one(&self, field: &str) -> Result<Option<AttributeValue>, String> {
        let mut values = self.values(field)?;
        if values.len() > 1 {
            return Err(format!(
                "scalar field {field} has more than one neutral value"
            ));
        }
        Ok(values.pop())
    }
    pub(super) fn integer(&self, field: &str) -> Result<Option<i64>, String> {
        match self.one(field)? {
            Some(AttributeValue::Integer(value)) => Ok(Some(value)),
            None => Ok(None),
            _ => Err(format!("field {field} is not integer-valued")),
        }
    }
    pub(super) fn boolean(&self, field: &str, invert: bool) -> Result<String, String> {
        match self.one(field)? {
            Some(AttributeValue::Boolean(value)) => Ok(i64::from(value != invert).to_string()),
            None => Ok(String::new()),
            _ => Err(format!("field {field} is not logical-valued")),
        }
    }
    pub(super) fn native_value(&self, field: &str) -> Option<&Value> {
        let mut value = self.native.as_ref()?;
        for part in field.split('.') {
            value = match value {
                Value::Array(values) => values.get(part.parse::<usize>().ok()?)?,
                Value::Object(values) => values.get(part)?,
                _ => return None,
            };
        }
        Some(value)
    }
}

pub(super) fn groups<'a>(
    record: &NativeRecord,
    attributes: &BTreeMap<&str, &'a SourceAttribute>,
    held: bool,
) -> Result<Vec<Fields<'a>>, String> {
    let Some(links) = record.field("neutral_values") else {
        if held {
            return Err("the held neutral model supplies no projection of this property".into());
        }
        let native = Value::Object(record.fields().map_err(|error| error.to_string())?);
        let owners: Vec<_> = record
            .field("owners")
            .and_then(|owners| owners.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|owner| {
                owner.as_str().map(|source_id| AttributeTarget::Source {
                    source_id: source_id.to_owned(),
                })
            })
            .collect();
        return Ok(if owners.is_empty() {
            vec![Fields {
                owner: None,
                values: BTreeMap::new(),
                native: Some(native),
            }]
        } else {
            owners
                .into_iter()
                .map(|owner| Fields {
                    owner: Some(owner),
                    values: BTreeMap::new(),
                    native: Some(native.clone()),
                })
                .collect()
        });
    };
    let links = links
        .as_object()
        .ok_or("property neutral reference map is malformed")?;
    let mut groups = BTreeMap::<AttributeTarget, BTreeMap<String, &SourceAttribute>>::new();
    for (field, references) in links {
        for reference in references
            .as_array()
            .ok_or("property reference list is malformed")?
        {
            let id = reference
                .as_str()
                .ok_or("property reference is not an identity")?;
            let attribute = *attributes
                .get(id)
                .ok_or_else(|| format!("current neutral attribute {id} does not resolve"))?;
            if groups
                .entry(attribute.target.clone())
                .or_default()
                .insert(field.clone(), attribute)
                .is_some()
            {
                return Err(format!(
                    "property field {field} has multiple values for the same current owner"
                ));
            }
        }
    }
    if groups.is_empty() {
        return Ok(vec![Fields {
            owner: None,
            values: BTreeMap::new(),
            native: None,
        }]);
    }
    Ok(groups
        .into_iter()
        .map(|(owner, values)| Fields {
            owner: Some(owner),
            values,
            native: None,
        })
        .collect())
}

pub(super) fn collection_size(record: &NativeRecord, key: &str) -> Result<usize, String> {
    let native = Value::Object(record.fields().map_err(|error| error.to_string())?);
    let mut value = &native;
    for part in key.split('.') {
        value = match value {
            Value::Array(values) => part
                .parse::<usize>()
                .ok()
                .and_then(|index| values.get(index)),
            Value::Object(values) => values.get(part),
            _ => None,
        }
        .ok_or_else(|| format!("collection {key} has no current typed size"))?;
    }
    value
        .as_array()
        .map(Vec::len)
        .ok_or_else(|| format!("collection {key} has no current typed size"))
}
