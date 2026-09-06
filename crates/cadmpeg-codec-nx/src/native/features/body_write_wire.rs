// SPDX-License-Identifier: Apache-2.0
//! Stable JSON columns for checked body-write frames.

use serde::{Serialize, Deserialize};
use super::FeatureOperationBodyWrite;
use crate::om::body_write::{BodyWriteFrame, BodyWriteIndex, BodyImageTag};

/// Exact body-write frame retained from one feature operation.
#[derive(Serialize, Deserialize)]
pub(super) struct BodyWriteWire {
    /// Globally unique relation identity.
    pub id: String,
    /// Owning operation-label identity, absent for an unlabeled record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_label: Option<String>,
    /// Owning bounded operation-record identity.
    pub operation_record: String,
    /// Zero-based body-write order within the operation payload.
    pub ordinal: u32,
    /// Persistent identity of the body written by this operation.
    pub body_identity: u8,
    /// Partition-local Parasolid GROUP node owned by this feature.
    pub group_node: u32,
    /// Exact serialized GROUP-node token.
    pub raw_group_node: Vec<u8>,
    /// Absolute offset of the GROUP-node token.
    pub group_node_source_offset: u64,
    /// Tagged body-image field discriminator.
    pub endpoint_tag: u8,
    /// Offset-store object containing the body's serialized image.
    pub body_image_object_index: u32,
    /// Unambiguous offset-store block selected by the body-image object index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_image_data_block: Option<String>,
    /// Exact serialized body-image object token.
    pub raw_body_image_object_index: Vec<u8>,
    /// Absolute offset of the body-image object token.
    pub body_image_object_index_source_offset: u64,
    /// Exact serialized frame byte length.
    pub byte_len: u64,
    /// Absolute offset of the opening `01 02` marker.
    pub source_offset: u64,
}

impl From<FeatureOperationBodyWrite> for BodyWriteWire {
    fn from(value: FeatureOperationBodyWrite) -> Self {
        Self {
            id: value.id, operation_label: value.operation_label, operation_record: value.operation_record, ordinal: value.ordinal,
            body_identity: value.frame.body_identity(), group_node: value.frame.group_node().value(), raw_group_node: value.frame.group_node().raw().to_vec(),
            group_node_source_offset: value.frame.group_node_offset(), endpoint_tag: value.frame.endpoint_tag().code(),
            body_image_object_index: value.frame.body_image().value(), body_image_data_block: value.body_image_data_block,
            raw_body_image_object_index: value.frame.body_image().raw().to_vec(), body_image_object_index_source_offset: value.frame.body_image_offset(),
            byte_len: u64::from(value.frame.byte_len()), source_offset: value.frame.offset(),
        }
    }
}

impl TryFrom<BodyWriteWire> for FeatureOperationBodyWrite {
    type Error = String;
    fn try_from(wire: BodyWriteWire) -> Result<Self, Self::Error> {
        let group = BodyWriteIndex::from_wire(wire.group_node, &wire.raw_group_node)
            .map_err(|error| format!("group_node/raw_group_node: {error}"))?;
        let image = BodyWriteIndex::from_wire(wire.body_image_object_index, &wire.raw_body_image_object_index)
            .map_err(|error| format!("body_image_object_index/raw_body_image_object_index: {error}"))?;
        let frame = BodyWriteFrame::<u64>::new(wire.body_identity, group, BodyImageTag::try_from(wire.endpoint_tag)?, image, wire.source_offset)
            .ok_or("source_offset: body-write frame end overflows")?;
        if wire.byte_len != u64::from(frame.byte_len()) { return Err("byte_len disagrees with body-write frame".into()); }
        if wire.group_node_source_offset != frame.group_node_offset() { return Err("group_node_source_offset disagrees with body-write frame".into()); }
        if wire.body_image_object_index_source_offset != frame.body_image_offset() { return Err("body_image_object_index_source_offset disagrees with body-write frame".into()); }
        Ok(Self { id: wire.id, operation_label: wire.operation_label, operation_record: wire.operation_record, ordinal: wire.ordinal,
            frame, body_image_data_block: wire.body_image_data_block })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIRE: &str = r#"{"id":"write","operation_record":"record","ordinal":0,"body_identity":255,"group_node":0,"raw_group_node":[160,0,0],"group_node_source_offset":103,"endpoint_tag":21,"body_image_object_index":0,"body_image_data_block":"block","raw_body_image_object_index":[241,0,0],"body_image_object_index_source_offset":111,"byte_len":15,"source_offset":100}"#;

    #[test]
    fn body_write_wire_preserves_alias_tokens_and_checks_derived_fields() {
        let write: FeatureOperationBodyWrite = serde_json::from_str(WIRE).unwrap();
        assert_eq!(serde_json::to_string(&write).unwrap(), WIRE);
        for (field, value) in [
            ("group_node", serde_json::json!(1)), ("raw_group_node", serde_json::json!([128,0])),
            ("endpoint_tag", serde_json::json!(17)), ("body_image_object_index", serde_json::json!(1)),
            ("raw_body_image_object_index", serde_json::json!([255])), ("group_node_source_offset", serde_json::json!(104)),
            ("body_image_object_index_source_offset", serde_json::json!(112)), ("byte_len", serde_json::json!(14)),
            ("source_offset", serde_json::json!(u64::MAX)),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(WIRE).unwrap();
            invalid[field] = value;
            assert!(serde_json::from_value::<FeatureOperationBodyWrite>(invalid).unwrap_err().to_string().contains(field));
        }
    }
}
