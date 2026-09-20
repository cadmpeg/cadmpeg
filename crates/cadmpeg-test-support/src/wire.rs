// SPDX-License-Identifier: Apache-2.0
//! Read serialized output in tests without adding production inspection APIs.

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Read a required field through the value's wire representation.
/// Nested fields use slash-separated JSON pointer components.
///
/// # Panics
///
/// Panics if serialization fails, the field is absent, or its type differs.
pub fn field<T: DeserializeOwned>(value: &impl Serialize, key: &str) -> T {
    let mut wire = serde_json::to_value(value).expect("serialize tested value");
    let field = wire
        .pointer_mut(&format!("/{key}"))
        .map(serde_json::Value::take)
        .unwrap_or_else(|| panic!("tested value has no field {key}"));
    serde_json::from_value(field).unwrap_or_else(|error| panic!("field {key}: {error}"))
}

/// Read a field whose wire contract omits the default value.
///
/// # Panics
///
/// Panics if serialization fails or a present field has an unexpected type.
pub fn field_or_default<T: DeserializeOwned + Default>(value: &impl Serialize, key: &str) -> T {
    let mut wire = serde_json::to_value(value).expect("serialize tested value");
    wire.pointer_mut(&format!("/{key}"))
        .map(serde_json::Value::take)
        .map_or_else(T::default, |field| {
            serde_json::from_value(field).unwrap_or_else(|error| panic!("field {key}: {error}"))
        })
}

/// Read a decode coverage measure, including the contract's implicit zero.
///
/// # Panics
///
/// Panics if the report cannot be serialized or its coverage has the wrong type.
pub fn coverage_count(report: &impl Serialize, key: &str) -> usize {
    let coverage: std::collections::BTreeMap<String, usize> = field_or_default(report, "coverage");
    coverage.get(key).copied().unwrap_or(0)
}
