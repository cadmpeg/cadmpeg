// SPDX-License-Identifier: Apache-2.0
//! Helpers the record family test modules share.

/// The message `T` states when `key` is present and null.
pub(super) fn refusal<T: serde::de::DeserializeOwned>(key: &str) -> String {
    let mut wire = serde_json::json!({});
    wire[key] = serde_json::Value::Null;
    let Err(refused) = serde_json::from_value::<T>(wire) else {
        panic!("{key}: null was admitted")
    };
    refused.to_string()
}
