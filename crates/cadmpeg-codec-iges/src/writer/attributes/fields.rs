// SPDX-License-Identifier: Apache-2.0
//! Resolve current neutral attribute references or out-of-scope typed values.

use super::{
    string_field, AttributeTarget, AttributeValue, BTreeMap, LinkedAttributes, NativeRecord,
    SourceAttribute,
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

#[derive(Clone, Copy)]
pub(super) enum Family {
    Property,
    Definition,
    Instance,
}

fn field_name(
    native: &Value,
    attribute: &SourceAttribute,
    family: Family,
) -> Result<String, String> {
    let prefix = match family {
        Family::Property => {
            let kind = native
                .get("property_kind")
                .and_then(Value::as_str)
                .ok_or("property has no typed kind")?;
            if matches!(kind, "generic_data" | "name") {
                return Ok(String::new());
            }
            format!("iges.property.{kind}.")
        }
        Family::Definition => "iges.attribute_table.default.".into(),
        Family::Instance => "iges.attribute_table.value.".into(),
    };
    let field = attribute.name.strip_prefix(&prefix).ok_or_else(|| {
        format!(
            "attribute {} has no field in its linked native grammar",
            attribute.id
        )
    })?;
    let mut value = native;
    for part in field.split('.') {
        value = match value {
            Value::Array(values) => part
                .parse::<usize>()
                .ok()
                .and_then(|index| values.get(index)),
            Value::Object(values) => values.get(part),
            _ => None,
        }
        .ok_or_else(|| format!("attribute {} names an absent native field", attribute.id))?;
    }
    Ok(field.to_owned())
}

pub(super) fn groups<'a>(
    record: &NativeRecord,
    attributes: &LinkedAttributes<'a>,
    family: Family,
) -> Result<Vec<Fields<'a>>, String> {
    let native = Value::Object(record.fields().map_err(|error| error.to_string())?);
    let Some(linked) = attributes.get(record.id()) else {
        return Ok(vec![Fields {
            owner: None,
            values: BTreeMap::new(),
            native: Some(native),
        }]);
    };
    let mut groups = BTreeMap::<AttributeTarget, BTreeMap<String, &SourceAttribute>>::new();
    for attribute in linked {
        let field = field_name(&native, attribute, family)?;
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
