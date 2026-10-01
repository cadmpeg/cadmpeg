// SPDX-License-Identifier: Apache-2.0
//! Native edge-definition wire projection from its class and raw payload.

use crate::families::consolidated::records::{
    consolidated_edge_definition_data, ConsolidatedEdgeDefinitionClass,
    ConsolidatedEdgeDefinitionData,
};
use crate::wire::records::{ConsolidatedFrameFlag, ConsolidatedFrameWidth, ConsolidatedRawFrame};
use serde::{Deserialize, Serialize};

/// Exact class-specific edge-definition frame owned by one consolidated edge node.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "EdgeDefinitionWire")]
pub(crate) struct CatiaConsolidatedEdgeDefinition {
    /// Complete raw frame.
    frame: ConsolidatedRawFrame<u64>,
    /// Edge-definition class in `0x23..=0x25`.
    class: ConsolidatedEdgeDefinitionClass,
    /// Class-specific data admitted during binary decode or wire validation.
    data: Option<ConsolidatedEdgeDefinitionData>,
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

#[derive(Serialize)]
struct EdgeDefinitionWireRef<'a> {
    byte_offset: u64,
    width: ConsolidatedFrameWidth,
    flag: ConsolidatedFrameFlag,
    class: ConsolidatedEdgeDefinitionClass,
    header_token: u32,
    payload: &'a [u8],
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<&'a ConsolidatedEdgeDefinitionData>,
}

impl Serialize for CatiaConsolidatedEdgeDefinition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        EdgeDefinitionWireRef {
            byte_offset: self.frame.pos,
            width: self.frame.width(),
            flag: self.frame.flag,
            class: self.class,
            header_token: self.frame.header_token(),
            payload: &self.frame.payload,
            data: self.data(),
        }
        .serialize(serializer)
    }
}

impl CatiaConsolidatedEdgeDefinition {
    pub(crate) fn from_source(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        definition: crate::families::consolidated::records::ConsolidatedEdgeDefinition,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let data =
            crate::families::consolidated::records::consolidated_edge_definition_data_charged(
                ctx,
                definition.class.into(),
                &definition.frame.payload,
            )?;
        Ok(Self {
            frame: definition.frame.into(),
            class: definition.class,
            data,
        })
    }
    pub(crate) fn data(&self) -> Option<&ConsolidatedEdgeDefinitionData> {
        self.data.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn frame(&self) -> &ConsolidatedRawFrame<u64> {
        &self.frame
    }
    #[cfg(test)]
    pub(crate) fn class(&self) -> ConsolidatedEdgeDefinitionClass {
        self.class
    }
}

#[cfg(test)]
impl From<CatiaConsolidatedEdgeDefinition> for EdgeDefinitionWire {
    fn from(value: CatiaConsolidatedEdgeDefinition) -> Self {
        Self {
            byte_offset: value.frame.pos,
            width: value.frame.width(),
            flag: value.frame.flag,
            class: value.class,
            header_token: value.frame.header_token(),
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
            frame: ConsolidatedRawFrame::new(
                wire.byte_offset,
                wire.width,
                wire.flag,
                wire.header_token,
                wire.payload,
            )?,
            class: wire.class,
            data,
        };
        if value.data != consolidated_edge_definition_data(value.class.into(), &value.frame.payload)
        {
            return Err("edge-definition data differs from its class and payload".to_owned());
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{CatiaConsolidatedEdgeDefinition, EdgeDefinitionWire};
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionClass;
    use crate::wire::records::ConsolidatedFrameFlag;
    use crate::wire::records::ConsolidatedFrameWidth;
    use crate::wire::records::ConsolidatedRawFrame;

    fn edge_definition() -> CatiaConsolidatedEdgeDefinition {
        CatiaConsolidatedEdgeDefinition {
            frame: ConsolidatedRawFrame::new(
                12,
                ConsolidatedFrameWidth::One,
                ConsolidatedFrameFlag::Flag03,
                5,
                vec![0x81, 0x05, 0x0f, 0x87],
            )
            .expect("checked fixture frame"),
            class: ConsolidatedEdgeDefinitionClass::Class24,
            data: Some(
                crate::families::consolidated::records::ConsolidatedEdgeDefinitionData::Compact24 {
                    operand: 1,
                },
            ),
        }
    }

    #[test]
    fn native_edge_definition_access_borrows_the_admitted_value() {
        let source = crate::families::consolidated::records::ConsolidatedEdgeDefinition {
            frame: ConsolidatedRawFrame::new(
                12,
                ConsolidatedFrameWidth::One,
                ConsolidatedFrameFlag::Flag03,
                5,
                vec![0x81, 0x05, 0x0f, 0x87],
            )
            .expect("checked frame"),
            class: ConsolidatedEdgeDefinitionClass::Class24,
        };
        let value = crate::test_support::with_service_context(|ctx| {
            CatiaConsolidatedEdgeDefinition::from_source(ctx, source)
        })
        .expect("admitted definition");
        let data = value.data().expect("compact data");
        assert!(std::ptr::eq(data, value.data().expect("same data")));
        assert_eq!(
            data,
            &crate::families::consolidated::records::ConsolidatedEdgeDefinitionData::Compact24 {
                operand: 1
            }
        );
    }

    #[test]
    fn edge_definition_borrowed_wire_preserves_json_bytes() {
        let value = edge_definition();
        let owned: EdgeDefinitionWire = value.clone().into();
        assert_eq!(
            serde_json::to_vec(&value).expect("borrowed definition JSON"),
            serde_json::to_vec(&owned).expect("owned definition JSON")
        );
    }

    #[test]
    fn edge_definition_retained_limit_refuses_json_record() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            #[serde(flatten)]
            definition: &'a CatiaConsolidatedEdgeDefinition,
        }
        let value = edge_definition();

        let record = Record {
            id: "catia:test:definition#0",
            definition: &value,
        };
        let arena_name = "edge_definitions";
        let json_len = serde_json::to_vec(&record).expect("definition JSON").len();
        let limit = u64::try_from(json_len + arena_name.len() - 1).expect("small JSON");
        let refused = crate::test_support::with_retained_limit(limit, |ctx| {
            let mut namespace = cadmpeg_ir::NativeNamespace::default();
            namespace.set_arena(ctx, arena_name, std::slice::from_ref(&record))
        });
        let error = refused.expect_err("record exceeds retained-byte limit");
        assert!(error.to_string().contains("RetainedBytes"), "{error}");
        crate::test_support::with_service_context(|ctx| {
            let mut namespace = cadmpeg_ir::NativeNamespace::default();
            namespace
                .set_arena(ctx, arena_name, std::slice::from_ref(&record))
                .expect("service profile admits edge definition");
        });
    }

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
        })
        .expect("service profile admits scalar lane");
        assert!(service.is_some());
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            crate::families::consolidated::records::consolidated_edge_definition_data_charged(
                ctx, 0x25, &payload,
            )
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_native_edge_definition_scalars")
        );
    }

    #[test]
    fn wire_data_is_derived_and_conflicts_are_rejected() {
        let value = CatiaConsolidatedEdgeDefinition {
            frame: ConsolidatedRawFrame::new(
                12,
                ConsolidatedFrameWidth::try_from(1).expect("one-byte width"),
                ConsolidatedFrameFlag::try_from(3).expect("frame flag"),
                5,
                vec![0x81, 0x05, 0x0f, 0x87],
            )
            .expect("checked fixture frame"),
            class: ConsolidatedEdgeDefinitionClass::Class24,
            data: Some(
                crate::families::consolidated::records::ConsolidatedEdgeDefinitionData::Compact24 {
                    operand: 1,
                },
            ),
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
