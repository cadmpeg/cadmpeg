// SPDX-License-Identifier: Apache-2.0
//! Flat native JSON for typed OM record references.

use crate::om::reference_value::{DirectReference, RecordReference, Tagged28};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum Value {
    PersistentHandle(u32),
    Tagged28(Tagged28),
    RecordOrdinal16(u16),
}

#[derive(Serialize, Deserialize)]
struct Wire<T> {
    #[serde(flatten)]
    value: Value,
    target_record: Option<T>,
}

pub(super) fn serialize<S: Serializer>(
    value: &RecordReference<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let (value, target_record) = match value {
        RecordReference::Direct(DirectReference::PersistentHandle(value)) => {
            (Value::PersistentHandle(*value), None)
        }
        RecordReference::Direct(DirectReference::Tagged28(value)) => {
            (Value::Tagged28(*value), None)
        }
        RecordReference::RecordOrdinal16 { ordinal, target } => {
            (Value::RecordOrdinal16(*ordinal), Some(target))
        }
    };
    Wire {
        value,
        target_record,
    }
    .serialize(serializer)
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<RecordReference<String>, D::Error> {
    let wire = Wire::<String>::deserialize(deserializer)?;
    match (wire.value, wire.target_record) {
        (Value::PersistentHandle(value), None) => Ok(RecordReference::Direct(
            DirectReference::PersistentHandle(value),
        )),
        (Value::Tagged28(value), None) => {
            Ok(RecordReference::Direct(DirectReference::Tagged28(value)))
        }
        (Value::RecordOrdinal16(ordinal), Some(target)) => {
            Ok(RecordReference::RecordOrdinal16 { ordinal, target })
        }
        (Value::RecordOrdinal16(_), None) => Err(serde::de::Error::custom(
            "target_record is required for record_ordinal16",
        )),
        (_, Some(_)) => Err(serde::de::Error::custom(
            "target_record must be null for a direct reference",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{DataBlockControlReference, ObjectReference};

    #[test]
    fn typed_reference_wire_preserves_fields_and_rejects_mismatches() {
        for fields in [
            r#""kind":"persistent_handle","value":4294967295,"target_record":null"#,
            r#""kind":"tagged28","value":268435455,"target_record":null"#,
            r#""kind":"record_ordinal16","value":65535,"target_record":"record#65535""#,
        ] {
            let json = format!(
                r#"{{"id":"r","record":"owner","object_id":1,"ordinal":0,{fields},"source_entry":"om","source_offset":10}}"#
            );
            let record: ObjectReference = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&record).unwrap(), json);
        }
        for fields in [
            r#""kind":"persistent_handle","value":1,"target_record":"record#1""#,
            r#""kind":"tagged28","value":268435456,"target_record":null"#,
            r#""kind":"record_ordinal16","value":65536,"target_record":"record#65536""#,
            r#""kind":"record_ordinal16","value":1,"target_record":null"#,
        ] {
            let json = format!(
                r#"{{"id":"r","record":"owner","object_id":1,"ordinal":0,{fields},"source_entry":"om","source_offset":10}}"#
            );
            assert!(
                serde_json::from_str::<ObjectReference>(&json).is_err(),
                "{json}"
            );
        }
        let json = r#"{"id":"r","data_block":"block","ordinal":0,"kind":"tagged28","value":268435455,"source_offset":10}"#;
        let record: DataBlockControlReference = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&record).unwrap(), json);
        for json in [
            json.replace("tagged28", "record_ordinal16"),
            json.replace("268435455", "268435456"),
        ] {
            assert!(serde_json::from_str::<DataBlockControlReference>(&json).is_err());
        }
    }
}
