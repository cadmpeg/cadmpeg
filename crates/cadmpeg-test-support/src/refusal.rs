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

/// Refuse the named operation one unit below its need, with every earlier
/// charge admitted.
///
/// `run` decodes with `cap` as the limit of `dimension`. A probe run arms a
/// [`RefusalProbe`](cadmpeg_core::decode::refusal_probe::RefusalProbe) and no
/// cap, so the first positive charge of `operation` refuses at its prior usage.
/// A replay sets the cap one unit below that need and must refuse at the same
/// operation by the ordinary limit. A decode charges in the same order on every
/// run, so a replay that refuses elsewhere is a defect of the decode, not of the
/// probe.
///
/// # Panics
///
/// Panics if the route has no named boundary, changes dimension, reaches a
/// different boundary on replay, or fails for another reason.
pub fn resource_limit_at<T>(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
    mut run: impl FnMut(u64) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::CodecError;
    let probed = {
        let _probe = RefusalProbe::arm(dimension, operation, None);
        run(u64::MAX)
    };
    let limit = match probed {
        Err(CodecError::ResourceLimit(limit)) if limit.operation == operation => limit,
        Err(error) => panic!("unexpected refusal before {operation}: {error:?}"),
        Ok(_) => panic!("missing resource boundary: {operation}"),
    };
    assert_eq!(limit.dimension, dimension);
    let need = limit
        .used
        .checked_add(limit.additional)
        .expect("resource need fits");
    let error = run(need - 1)
        .err()
        .expect("one unit below the resource boundary");
    assert!(
        matches!(error, CodecError::ResourceLimit(ref refusal)
            if refusal.dimension == dimension
                && refusal.operation == operation
                && refusal.used + refusal.additional == need),
        "{operation}: the replay one unit below the need refused elsewhere, so the decode charges in a run-dependent order: {error:?}"
    );
    error
}
