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

/// Admit preceding allocations, then refuse the named operation one unit below its need.
///
/// # Panics
///
/// Panics if the route has no named boundary, changes dimension, or fails for another reason.
pub fn resource_limit_at<T>(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
    run: impl Fn(u64) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::CodecError;
    let mut cap = 0;
    for _ in 0..8192 {
        match run(cap) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, dimension);
                let need = limit
                    .used
                    .checked_add(limit.additional)
                    .expect("resource need fits");
                assert!(need > cap, "{operation}: {limit:?}");
                if limit.operation == operation {
                    let error = run(need - 1)
                        .err()
                        .expect("one unit below the resource boundary");
                    assert!(matches!(error, CodecError::ResourceLimit(ref refusal)
                        if refusal.dimension == dimension
                            && refusal.operation == operation
                            && refusal.used + refusal.additional == need));
                    return error;
                }
                cap = need;
            }
            Err(error) => panic!("unexpected refusal before {operation}: {error:?}"),
            Ok(_) => panic!("missing resource boundary: {operation}"),
        }
    }
    panic!("resource route exceeds the boundary count: {operation}");
}
