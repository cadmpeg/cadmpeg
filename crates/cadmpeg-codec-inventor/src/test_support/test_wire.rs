// SPDX-License-Identifier: Apache-2.0
//! Shared native-wire refusal oracles for the crate's `#[cfg(test)]` suites.

/// Deserialize `T` from a wire object whose `key` is null and return the
/// refusal message.
pub(crate) fn refusal<T: serde::de::DeserializeOwned>(key: &str) -> String {
    let mut wire = serde_json::json!({});
    wire[key] = serde_json::Value::Null;
    let Err(refused) = serde_json::from_value::<T>(wire) else {
        panic!("{key}: null was admitted")
    };
    refused.to_string()
}

/// Assert that a refusal message names its key and not the null value.
pub(crate) fn states_the_key(key: &str, message: &str) {
    assert!(
        message.starts_with(&format!("{key}: ")),
        "the refusal of a null {key} states {message}"
    );
    assert!(
        message.contains("it does not state null"),
        "the refusal of a null {key} states {message}"
    );
}
