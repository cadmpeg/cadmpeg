// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::fmt::Debug;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::ids::UnknownId;
use crate::tessellation::{TessellationChannel, TessellationChannelDomain};
use crate::unknown::UnknownRecord;

fn assert_base64_round_trip_and_rejection<T>(value: &T, field: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let mut json = serde_json::to_value(value).unwrap();
    assert_eq!(json[field], "AQID");
    assert_eq!(serde_json::from_value::<T>(json.clone()).unwrap(), *value);
    json[field] = serde_json::Value::String("%%%".into());
    assert!(serde_json::from_value::<T>(json).is_err());
}

#[test]
fn byte_payloads_use_nonempty_base64_and_reject_invalid_text() {
    assert_base64_round_trip_and_rejection(
        &UnknownRecord::retained(
            UnknownId("synthetic:test:unknown#0".into()),
            0,
            vec![1, 2, 3],
            Vec::new(),
        ),
        "data",
    );
    assert_base64_round_trip_and_rejection(
        &TessellationChannel {
            domain: TessellationChannelDomain::default(),
            item_size: 3,
            kind: 0,
            flags: 0,
            count: 1,
            data: vec![1, 2, 3],
            indices: Vec::new(),
        },
        "data",
    );
}
