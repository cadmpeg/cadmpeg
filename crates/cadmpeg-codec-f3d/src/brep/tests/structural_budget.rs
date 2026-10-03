// SPDX-License-Identifier: Apache-2.0
//! Logical collection slots of the structural value contract.

use cadmpeg_core::decode::u64_from_index;
use serde::Serialize;
use serde_value::Value;

pub(super) fn projection_items(value: &impl Serialize) -> u64 {
    fn slots(value: &Value) -> u64 {
        match value {
            Value::Map(fields) => {
                u64_from_index(fields.len())
                    + fields
                        .iter()
                        .map(|(key, value)| slots(key) + slots(value))
                        .sum::<u64>()
            }
            Value::Seq(values) => {
                u64_from_index(values.len()) + values.iter().map(slots).sum::<u64>()
            }
            Value::Option(Some(value)) | Value::Newtype(value) => 1 + slots(value),
            _ => 0,
        }
    }
    slots(&serde_value::to_value(value).unwrap())
}

pub(super) fn projection_bytes(value: &impl Serialize) -> u64 {
    u64_from_index(serde_json::to_vec(value).unwrap().len())
}
