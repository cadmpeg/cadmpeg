// SPDX-License-Identifier: Apache-2.0
//! Read serialized output in tests without adding production inspection APIs.

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Read a required field through the value's wire representation.
///
/// # Panics
///
/// Panics if serialization fails, the field is absent, or its type differs.
pub fn field<T: DeserializeOwned>(value: &impl Serialize, key: &str) -> T {
    let mut wire = serde_json::to_value(value).expect("serialize tested value");
    let field = wire
        .as_object_mut()
        .expect("tested value is an object")
        .remove(key)
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
    wire.as_object_mut()
        .expect("tested value is an object")
        .remove(key)
        .map_or_else(T::default, |field| {
            serde_json::from_value(field).unwrap_or_else(|error| panic!("field {key}: {error}"))
        })
}
