// SPDX-License-Identifier: Apache-2.0
//! Typed JSON values carried by source attributes.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::scalar::FiniteReal;

/// A JSON value with exact signed and unsigned integer domains.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AttributeJson {
    /// JSON null, distinct from an unreadable attribute slot.
    Null,
    /// A JSON boolean.
    Boolean(bool),
    /// An integer in the signed 64-bit domain.
    Integer(i64),
    /// An integer above the signed domain, through u64::MAX.
    Unsigned(
        #[serde(deserialize_with = "deserialize_unsigned")]
        #[cfg_attr(feature = "schema", schemars(schema_with = "upper_unsigned_schema"))]
        u64,
    ),
    /// A finite floating-point value.
    Float(#[cfg_attr(feature = "schema", schemars(with = "f64"))] FiniteReal),
    /// Authored text.
    String(String),
    /// Ordered JSON values.
    Array(Vec<AttributeJson>),
    /// JSON object members ordered by key.
    Object(BTreeMap<String, AttributeJson>),
}

fn deserialize_unsigned<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let value = u64::deserialize(d)?;
    if value > i64::MAX as u64 {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "attribute unsigned integer belongs to the signed domain",
        ))
    }
}

#[cfg(feature = "schema")]
fn upper_unsigned_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = u64::json_schema(generator);
    schema.insert(
        "minimum".into(),
        serde_json::Value::from(i64::MAX as u64 + 1),
    );
    schema.insert("maximum".into(), serde_json::Value::from(u64::MAX));
    schema
}
