// SPDX-License-Identifier: Apache-2.0
//! Native edge-definition wire projection from its class and raw payload.

use crate::families::consolidated::records::{
    consolidated_edge_definition_data, ConsolidatedEdgeDefinitionClass,
    ConsolidatedEdgeDefinitionData,
};
use crate::wire::records::{ConsolidatedFrameFlag, ConsolidatedFrameWidth, ConsolidatedRawFrame};
use serde::{Deserialize, Serialize};

/// Exact class-specific edge-definition frame owned by one consolidated edge node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "EdgeDefinitionWire", into = "EdgeDefinitionWire")]
pub(crate) struct CatiaConsolidatedEdgeDefinition {
    /// Complete raw frame.
    pub(super) frame: ConsolidatedRawFrame<u64>,
    /// Edge-definition class in `0x23..=0x25`.
    pub(crate) class: ConsolidatedEdgeDefinitionClass,
    /// Class-specific data admitted during binary decode or wire validation.
    pub(super) data: Option<ConsolidatedEdgeDefinitionData>,
}

#[derive(Serialize, Deserialize)]
struct EdgeDefinitionWire {
    byte_offset: u64,
    width: ConsolidatedFrameWidth,
    flag: ConsolidatedFrameFlag,
    class: ConsolidatedEdgeDefinitionClass,
    header_token: u32,
    payload: Vec<u8>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_data"
    )]
    data: Option<ConsolidatedEdgeDefinitionData>,
}

impl CatiaConsolidatedEdgeDefinition {
    pub(crate) fn data(&self) -> Option<ConsolidatedEdgeDefinitionData> {
        consolidated_edge_definition_data(self.class.into(), &self.frame.payload)
    }
}

impl From<CatiaConsolidatedEdgeDefinition> for EdgeDefinitionWire {
    fn from(value: CatiaConsolidatedEdgeDefinition) -> Self {
        Self {
            byte_offset: value.frame.pos,
            width: value.frame.width,
            flag: value.frame.flag,
            class: value.class,
            header_token: value.frame.header_token,
            payload: value.frame.payload,
            data: value.data,
        }
    }
}

impl TryFrom<EdgeDefinitionWire> for CatiaConsolidatedEdgeDefinition {
    type Error = String;
    fn try_from(wire: EdgeDefinitionWire) -> Result<Self, Self::Error> {
        let data = wire.data;
        let value = Self {
            frame: ConsolidatedRawFrame {
                pos: wire.byte_offset,
                width: wire.width,
                flag: wire.flag,
                header_token: wire.header_token,
                payload: wire.payload,
            },
            class: wire.class,
            data,
        };
        if value.data != value.data() {
            return Err("edge-definition data differs from its class and payload".to_owned());
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::CatiaConsolidatedEdgeDefinition;
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionClass;
    use crate::wire::records::ConsolidatedFrameFlag;
    use crate::wire::records::ConsolidatedFrameWidth;
    use crate::wire::records::ConsolidatedRawFrame;

    #[test]
    fn native_edge_definition_scalar_lane_refuses_before_retention() {
        let mut payload = vec![0x82, 0x05, 0xe7, 0x0a, 0x87, 0x0d];
        for value in [1.0_f64, 2.0, 1.0e-6, 3.0, 4.0, 1.0, 5.0, 1.0e-6] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let service = crate::test_support::with_service_context(|ctx| {
            crate::families::consolidated::records::consolidated_edge_definition_data_charged(
                ctx, 0x25, &payload,
            )
        }).expect("service profile admits scalar lane");
        assert!(service.is_some());
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            crate::families::consolidated::records::consolidated_edge_definition_data_charged(
                ctx, 0x25, &payload,
            )
        });
        assert!(matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_native_edge_definition_scalars"));
    }

    #[test]
    fn wire_data_is_derived_and_conflicts_are_rejected() {
        let value = CatiaConsolidatedEdgeDefinition {
            frame: ConsolidatedRawFrame {
                pos: 12,
                width: ConsolidatedFrameWidth::try_from(1).expect("one-byte width"),
                flag: ConsolidatedFrameFlag::try_from(3).expect("frame flag"),
                header_token: 5,
                payload: vec![0x81, 0x05, 0x0f, 0x87],
            },
            class: ConsolidatedEdgeDefinitionClass::Class24,
            data: Some(crate::families::consolidated::records::ConsolidatedEdgeDefinitionData::Compact24 { operand: 1 }),
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

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_data, ConsolidatedEdgeDefinitionData, "data");
