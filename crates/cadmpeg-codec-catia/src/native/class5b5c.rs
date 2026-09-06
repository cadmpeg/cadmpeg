// SPDX-License-Identifier: Apache-2.0
//! Native class-0x5b/0x5c frames and their byte-string wire projection.

use crate::wire::records::{ConsolidatedFrameFlag, ConsolidatedFrameWidth, ConsolidatedRawFrame};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Record class of a consolidated B-family class-`0x5b` or class-`0x5c` frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "u8", into = "u8")]
pub enum CatiaClass5b5c {
    /// Class `0x5b`.
    Class5b,
    /// Class `0x5c`.
    Class5c,
}

impl From<CatiaClass5b5c> for u8 {
    fn from(value: CatiaClass5b5c) -> Self {
        match value {
            CatiaClass5b5c::Class5b => 0x5b,
            CatiaClass5b5c::Class5c => 0x5c,
        }
    }
}

impl TryFrom<u8> for CatiaClass5b5c {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x5b => Ok(Self::Class5b),
            0x5c => Ok(Self::Class5c),
            other => Err(format!("class {other:#x} is not 0x5b or 0x5c")),
        }
    }
}

/// Complete consolidated class-`0x5b` or class-`0x5c` source-local control record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "Class5b5cWire", into = "Class5b5cWire")]
pub struct CatiaConsolidatedClass5b5cRecord {
    /// Stable native-record identity.
    pub id: String,
    /// Complete framed record.
    pub frame: ConsolidatedRawFrame<u64>,
    /// Zero-based bounded record-source ordinal.
    pub source_index: u64,
    /// Logical offset within the bounded record source.
    pub source_offset: u64,
    /// Record class.
    pub class: CatiaClass5b5c,
}

impl CatiaConsolidatedClass5b5cRecord {
    fn byte_len(&self) -> u64 {
        4 + u64::from(u8::from(self.frame.width)) + self.frame.payload.len() as u64
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct Class5b5cWire {
    id: String,
    byte_offset: u64,
    source_index: u64,
    source_offset: u64,
    byte_len: u64,
    width: ConsolidatedFrameWidth,
    flag: ConsolidatedFrameFlag,
    class: CatiaClass5b5c,
    header_token: u32,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    payload: Vec<u8>,
}

impl From<CatiaConsolidatedClass5b5cRecord> for Class5b5cWire {
    fn from(record: CatiaConsolidatedClass5b5cRecord) -> Self {
        Self {
            byte_len: record.byte_len(),
            id: record.id,
            byte_offset: record.frame.pos,
            source_index: record.source_index,
            source_offset: record.source_offset,
            width: record.frame.width,
            flag: record.frame.flag,
            class: record.class,
            header_token: record.frame.header_token,
            payload: record.frame.payload,
        }
    }
}

impl TryFrom<Class5b5cWire> for CatiaConsolidatedClass5b5cRecord {
    type Error = String;
    fn try_from(wire: Class5b5cWire) -> Result<Self, Self::Error> {
        let record = Self {
            id: wire.id,
            frame: ConsolidatedRawFrame {
                pos: wire.byte_offset,
                width: wire.width,
                flag: wire.flag,
                header_token: wire.header_token,
                payload: wire.payload,
            },
            source_index: wire.source_index,
            source_offset: wire.source_offset,
            class: wire.class,
        };
        if wire.byte_len != record.byte_len() {
            return Err("class-0x5b/0x5c byte_len differs from the complete frame".to_owned());
        }
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_wire_preserves_byte_payload_and_checks_derived_length() {
        let record = CatiaConsolidatedClass5b5cRecord {
            id: "catia:consolidated:class5b5c-record#0".to_owned(),
            frame: ConsolidatedRawFrame {
                pos: 42,
                width: ConsolidatedFrameWidth::Two,
                flag: ConsolidatedFrameFlag::Flag03,
                header_token: 5,
                payload: vec![0, 0, 0],
            },
            source_index: 0,
            source_offset: 42,
            class: CatiaClass5b5c::Class5b,
        };
        let mut wire = serde_json::to_value(&record).expect("serialize raw frame");
        assert_eq!(wire["byte_offset"], serde_json::json!(42));
        assert_eq!(wire["byte_len"], serde_json::json!(9));
        assert_eq!(wire["payload"], serde_json::json!("AAAA"));
        assert_eq!(
            serde_json::from_value::<CatiaConsolidatedClass5b5cRecord>(wire.clone())
                .expect("load raw frame"),
            record
        );
        wire["byte_len"] = serde_json::json!(8);
        assert!(
            serde_json::from_value::<CatiaConsolidatedClass5b5cRecord>(wire)
                .expect_err("reject truncated declared frame")
                .to_string()
                .contains("byte_len differs")
        );
    }
}
