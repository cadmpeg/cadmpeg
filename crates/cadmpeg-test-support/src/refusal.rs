// SPDX-License-Identifier: Apache-2.0
//! Refusal oracles for a wire key that is present and null.
//!
//! A native wire type states the key it refuses. These helpers read one key
//! through the type's own `Deserialize` implementation and check the message.

/// The message `T` states when `key` is present and null.
///
/// # Panics
///
/// Panics when `T` admits the null value.
pub fn refusal<T: serde::de::DeserializeOwned>(key: &str) -> String {
    let mut wire = serde_json::json!({});
    wire[key] = serde_json::Value::Null;
    let Err(refused) = serde_json::from_value::<T>(wire) else {
        panic!("{key}: null was admitted")
    };
    refused.to_string()
}

/// Assert that a refusal message names its key and not the null value.
///
/// # Panics
///
/// Panics when `message` does not name `key`.
pub fn states_the_key(key: &str, message: &str) {
    assert!(
        message.starts_with(&format!("{key}: ")),
        "the refusal of a null {key} states {message}"
    );
    assert!(
        message.contains("it does not state null"),
        "the refusal of a null {key} states {message}"
    );
}
