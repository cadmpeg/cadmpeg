// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;

pub(super) fn check_lane_wire<T>(json: &str, columns: &[&str])
where
    T: serde::de::DeserializeOwned + Serialize,
{
    let lane: T = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&lane).unwrap(), json);
    for column in columns {
        let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
        malformed[*column].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<T>(malformed).is_err(), "{column}");
    }
}
