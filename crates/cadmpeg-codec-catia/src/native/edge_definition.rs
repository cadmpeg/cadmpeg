// SPDX-License-Identifier: Apache-2.0
//! Native edge-definition wire projection from its class and raw payload.

use crate::families::consolidated::records::{
    ConsolidatedEdgeDefinitionClass, ConsolidatedEdgeDefinitionData,
    consolidated_edge_definition_data,
};
use crate::wire::records::{ConsolidatedFrameFlag, ConsolidatedFrameWidth};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Exact class-specific edge-definition frame owned by one consolidated edge node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "EdgeDefinitionWire", into = "EdgeDefinitionWire")]
pub struct CatiaConsolidatedEdgeDefinition {
    /// Record byte offset.
    pub byte_offset: u64,
    /// Header-token width in bytes.
    pub width: crate::wire::records::ConsolidatedFrameWidth,
    /// Independent framing flag.
    pub flag: crate::wire::records::ConsolidatedFrameFlag,
    /// Edge-definition class in `0x23..=0x25`.
    pub class: ConsolidatedEdgeDefinitionClass,
    /// Width-coded header token.
    pub header_token: u32,
    /// Complete class-specific payload.
    pub payload: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct EdgeDefinitionWire {
    byte_offset: u64,
    width: ConsolidatedFrameWidth,
    flag: ConsolidatedFrameFlag,
    class: ConsolidatedEdgeDefinitionClass,
    header_token: u32,
    payload: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<ConsolidatedEdgeDefinitionData>,
}

impl CatiaConsolidatedEdgeDefinition {
    pub(crate) fn data(&self) -> Option<ConsolidatedEdgeDefinitionData> {
        consolidated_edge_definition_data(self.class.into(), &self.payload)
    }
}

impl From<CatiaConsolidatedEdgeDefinition> for EdgeDefinitionWire {
    fn from(value: CatiaConsolidatedEdgeDefinition) -> Self {
        let data = value.data();
        Self {
            byte_offset: value.byte_offset,
            width: value.width,
            flag: value.flag,
            class: value.class,
            header_token: value.header_token,
            payload: value.payload,
            data,
        }
    }
}

impl TryFrom<EdgeDefinitionWire> for CatiaConsolidatedEdgeDefinition {
    type Error = String;
    fn try_from(wire: EdgeDefinitionWire) -> Result<Self, Self::Error> {
        let value = Self {
            byte_offset: wire.byte_offset,
            width: wire.width,
            flag: wire.flag,
            class: wire.class,
            header_token: wire.header_token,
            payload: wire.payload,
        };
        if wire.data != value.data() {
            return Err("edge-definition data differs from its class and payload".to_owned());
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_data_is_derived_and_conflicts_are_rejected() {
        let value = CatiaConsolidatedEdgeDefinition {
            byte_offset: 12,
            width: ConsolidatedFrameWidth::try_from(1).expect("one-byte width"),
            flag: ConsolidatedFrameFlag::try_from(3).expect("frame flag"),
            class: ConsolidatedEdgeDefinitionClass::Class24,
            header_token: 5,
            payload: vec![0x81, 0x05, 0x0f, 0x87],
        };
        let mut wire = serde_json::to_value(&value).expect("serialize definition");
        assert_eq!(wire["class"], serde_json::json!(0x24));
        assert_eq!(
            wire["data"],
            serde_json::json!({"kind": "compact24", "operand": 1})
        );
        assert_eq!(
            serde_json::from_value::<CatiaConsolidatedEdgeDefinition>(wire.clone())
                .expect("load derived data"),
            value
        );
        wire["data"]["operand"] = serde_json::json!(2);
        assert!(
            serde_json::from_value::<CatiaConsolidatedEdgeDefinition>(wire)
                .expect_err("reject conflicting data")
                .to_string()
                .contains("data differs")
        );
    }
}
