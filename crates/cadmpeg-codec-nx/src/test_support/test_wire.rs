// SPDX-License-Identifier: Apache-2.0
//! Shared JSON wire oracles for the crate's `#[cfg(test)]` suites.

use serde::Serialize;

/// Assert that `json` round-trips through `T` in wire order, that `field` set
/// to `invalid` is refused, and that every source-offset lane refuses
/// `u64::MAX`.
pub(crate) fn check_wire<T: serde::de::DeserializeOwned + Serialize + std::fmt::Debug>(
    json: &str,
    field: &str,
    invalid: serde_json::Value,
) {
    let row: T = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&row).unwrap(), json);
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    wire[field] = invalid;
    let error = serde_json::from_value::<T>(wire).unwrap_err();
    assert!(error.to_string().contains(field), "{error}");
    let original: serde_json::Value = serde_json::from_str(json).unwrap();
    for field in [
        "first_index_source_offset",
        "object_index_source_offset",
        "target_index_source_offset",
        "source_offset",
    ] {
        if original.get(field).is_none() {
            continue;
        }
        let mut invalid = original.clone();
        invalid[field] = serde_json::json!(u64::MAX);
        let error = serde_json::from_value::<T>(invalid).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
    if let Some(offsets) = original
        .get("index_source_offsets")
        .and_then(serde_json::Value::as_array)
    {
        for index in 0..offsets.len() {
            let mut invalid = original.clone();
            invalid["index_source_offsets"][index] = serde_json::json!(u64::MAX);
            let error = serde_json::from_value::<T>(invalid).unwrap_err();
            assert!(
                error.to_string().contains("index_source_offsets"),
                "{error}"
            );
        }
    }
}
