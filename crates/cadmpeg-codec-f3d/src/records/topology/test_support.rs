// SPDX-License-Identifier: Apache-2.0
//! Helpers the topology family test modules share.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

pub(super) fn states_the_key(key: &str, message: &str) {
    assert!(
        message.starts_with(&format!("{key}: ")),
        "the refusal of a null {key} states {message}"
    );
    assert!(
        message.contains("it does not state null"),
        "the refusal of a null {key} states {message}"
    );
}

pub(super) fn rejects_changed_fields<T: DeserializeOwned + Serialize>(
    wire: Value,
    fields: &[&str],
) {
    let admitted: T = serde_json::from_value(wire).unwrap();
    let wire = serde_json::to_value(admitted).unwrap();
    for field in fields {
        let mut invalid = wire.clone();
        invalid[field] = (wire[field].as_u64().unwrap() + 1).into();
        assert!(serde_json::from_value::<T>(invalid).is_err(), "{field}");
    }
}
