// SPDX-License-Identifier: Apache-2.0
//! Persistent object frames with exact non-null compact identity tokens.

use crate::om::compact::{CompactIndexAtom, LocatedCompactIndex};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ObjectFrameWire")]
pub(in crate::native) struct DataBlockObjectFrame {
    pub(in crate::native) id: String,
    pub(super) data_block: String,
    pub(super) ordinal: u32,
    pub(in crate::native) object: LocatedCompactIndex<u64>,
}

#[derive(Serialize)]
struct ObjectFrameRef<'a> {
    id: &'a str,
    data_block: &'a str,
    ordinal: u32,
    object_id: u32,
    raw_object_id: &'a [u8],
    source_offset: u64,
}

impl Serialize for DataBlockObjectFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ObjectFrameRef {
            id: &self.id,
            data_block: &self.data_block,
            ordinal: self.ordinal,
            object_id: self.object.atom.value(),
            raw_object_id: self.object.atom.raw(),
            source_offset: self.object.offset,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct ObjectFrameWire {
    id: String,
    data_block: String,
    ordinal: u32,
    object_id: u32,
    raw_object_id: Vec<u8>,
    source_offset: u64,
}

#[cfg(test)]
impl From<DataBlockObjectFrame> for ObjectFrameWire {
    fn from(value: DataBlockObjectFrame) -> Self {
        Self {
            id: value.id,
            data_block: value.data_block,
            ordinal: value.ordinal,
            object_id: value.object.atom.value(),
            raw_object_id: value.object.atom.raw().to_vec(),
            source_offset: value.object.offset,
        }
    }
}

impl TryFrom<ObjectFrameWire> for DataBlockObjectFrame {
    type Error = String;
    fn try_from(wire: ObjectFrameWire) -> Result<Self, Self::Error> {
        let atom = CompactIndexAtom::from_wire(wire.object_id, &wire.raw_object_id)
            .map_err(|error| format!("object_id/raw_object_id: {error}"))?;
        Ok(Self {
            id: wire.id,
            data_block: wire.data_block,
            ordinal: wire.ordinal,
            object: LocatedCompactIndex {
                atom,
                offset: wire.source_offset,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    // Wire fixtures exercise both legal compact encodings and malformed input.
    #![allow(clippy::unwrap_used)]
    use super::DataBlockObjectFrame;

    #[test]
    fn object_frames_derive_identity_from_the_exact_non_null_token() {
        for (value, raw) in [
            (0, "[0]"),
            (115, "[115]"),
            (370, "[129,114]"),
            (0, "[128,0]"),
            (32511, "[254,255]"),
        ] {
            let wire = format!(
                r#"{{"id":"frame","data_block":"block","ordinal":1,"object_id":{value},"raw_object_id":{raw},"source_offset":100}}"#
            );
            let frame: DataBlockObjectFrame = serde_json::from_str(&wire).unwrap();
            assert_eq!(frame.object.atom.value(), value);
            assert_eq!(serde_json::to_string(&frame).unwrap(), wire);
            assert_eq!(
                serde_json::to_vec(&frame).unwrap(),
                serde_json::to_vec(&super::ObjectFrameWire::from(frame.clone())).unwrap()
            );
            let mut invalid = serde_json::to_value(frame).unwrap();
            invalid["object_id"] = serde_json::json!(value + 1);
            assert!(serde_json::from_value::<DataBlockObjectFrame>(invalid)
                .unwrap_err()
                .to_string()
                .contains("object_id/raw_object_id"));
        }
        for raw in ["[]", "[255]", "[128]", "[0,0]"] {
            let wire = format!(
                r#"{{"id":"frame","data_block":"block","ordinal":1,"object_id":0,"raw_object_id":{raw},"source_offset":100}}"#
            );
            assert!(serde_json::from_str::<DataBlockObjectFrame>(&wire)
                .unwrap_err()
                .to_string()
                .contains("object_id/raw_object_id"));
        }
    }

    #[test]
    fn object_frame_native_limit_refuses_before_raw_token_copy() {
        let wire = serde_json::json!({
            "id": "nx:feature:object-frame#0", "data_block": "nx:om:data-block#0",
            "ordinal": 1, "object_id": 370, "raw_object_id": [129, 114],
            "source_offset": 100
        });
        let frame: DataBlockObjectFrame = serde_json::from_value(wire.clone()).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(&frame, wire);
    }
}
