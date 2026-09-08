// SPDX-License-Identifier: Apache-2.0
//! Stable JSON columns for checked common-frame structures.

use super::{FeatureOperationCommonFrame, FeatureOperationTerminalFrame};
use crate::om::common_frame::{CommonFrame, CommonFramePrefix, CommonFrameSuffix, TerminalFrame};
use serde::{Deserialize, Serialize};

/// Exactly framed common record in one bounded feature operation.
#[derive(Serialize, Deserialize)]
pub(super) struct CommonFrameWire {
    /// Globally unique common-frame identity.
    pub id: String,
    /// Owning bounded operation record.
    pub operation_record: String,
    /// Zero-based frame order within the operation payload.
    pub ordinal: u32,
    /// Three compact prefix indices.
    pub indices: [u32; 3],
    /// Exact compact-index tokens in order.
    pub raw_indices: [Vec<u8>; 3],
    /// Fixed marker selecting the index layout.
    pub marker: [u8; 3],
    /// Exact eight-byte state lane following the fixed state marker.
    ///
    /// The first three bytes remain an untyped operation-state prefix. The
    /// admitted field mappings begin at byte three; callers must not treat the
    /// prefix, or any other state byte, as feature suppression without the
    /// separate serialized owner and typed-value joins.
    pub state: [u8; 8],
    /// Whether legacy operation modules are inactive, when the stored field is boolean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_inactive_modules: Option<bool>,
    /// Whether the operation modifies Parasolid data, when the stored field is boolean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifies_parasolid_data: Option<bool>,
    /// Exact two-byte `m_splitTrackingData` representation.
    #[serde(default)]
    pub split_tracking_data: [u8; 2],
    /// Serialized operation group count.
    #[serde(default)]
    pub group_count: u8,
    /// Duplicated frame-local ordinal.
    pub local_ordinal: u32,
    /// Exact canonical token repeated for the local ordinal.
    pub raw_local_ordinal: Vec<u8>,
    /// Nullable object reference following the duplicated ordinal.
    pub object_index: Option<u32>,
    /// Exact canonical nullable object-reference token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native offset-store data-block arena, when found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Exact serialized frame byte length.
    pub byte_len: u64,
    /// Absolute offset of the first compact index token.
    pub source_offset: u64,
    /// Absolute offsets of the compact prefix-index tokens.
    pub index_source_offsets: [u64; 3],
    /// Absolute offset of the first state byte.
    pub state_source_offset: u64,
    /// Absolute offset of the first local-ordinal token.
    pub local_ordinal_source_offset: u64,
    /// Absolute offset of the object-reference token.
    pub object_index_source_offset: u64,
}

/// Canonical terminal common-frame suffix of one feature operation.
#[derive(Serialize, Deserialize)]
pub(super) struct TerminalFrameWire {
    /// Globally unique frame identity.
    pub id: String,
    /// Owning bounded operation record.
    pub operation_record: String,
    /// Exact common frame when it occurs immediately before this suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub immediate_common_frame: Option<String>,
    /// Duplicated frame-local ordinal.
    pub local_ordinal: u32,
    /// Exact canonical token repeated for the local ordinal.
    pub raw_local_ordinal: Vec<u8>,
    /// Nullable object reference following the duplicated ordinal.
    pub object_index: Option<u32>,
    /// Exact canonical nullable object-reference token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native offset-store data-block arena, when found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute offset of the first local-ordinal token.
    pub source_offset: u64,
    /// Absolute offset of the object-reference token.
    pub object_index_source_offset: u64,
}

impl From<FeatureOperationCommonFrame> for CommonFrameWire {
    fn from(value: FeatureOperationCommonFrame) -> Self {
        let frame = value.frame;
        Self {
            id: value.id,
            operation_record: value.operation_record,
            ordinal: value.ordinal,
            indices: frame.prefix().indices(),
            raw_indices: frame.prefix().raw_indices(),
            marker: frame.prefix().marker(),
            state: frame.state(),
            legacy_inactive_modules: frame.legacy_inactive_modules(),
            modifies_parasolid_data: frame.modifies_parasolid_data(),
            split_tracking_data: frame.split_tracking_data(),
            group_count: frame.group_count(),
            local_ordinal: frame.suffix().local_ordinal(),
            raw_local_ordinal: frame.suffix().raw_local_ordinal().to_vec(),
            object_index: frame.suffix().object_index(),
            raw_object_index: frame.suffix().raw_object_index().to_vec(),
            data_block: frame.suffix().target().cloned().flatten(),
            byte_len: frame.byte_len() as u64,
            source_offset: frame.offset(),
            index_source_offsets: frame.index_offsets(),
            state_source_offset: frame.state_offset(),
            local_ordinal_source_offset: frame.local_ordinal_offset(),
            object_index_source_offset: frame.object_index_offset(),
        }
    }
}

impl TryFrom<CommonFrameWire> for FeatureOperationCommonFrame {
    type Error = &'static str;
    fn try_from(wire: CommonFrameWire) -> Result<Self, Self::Error> {
        let prefix = CommonFramePrefix::from_wire(wire.indices, &wire.raw_indices, wire.marker)?;
        let suffix = CommonFrameSuffix::from_wire(
            wire.local_ordinal,
            &wire.raw_local_ordinal,
            wire.object_index,
            &wire.raw_object_index,
        )?;
        let frame = CommonFrame::<u64, Option<String>>::new(
            prefix,
            wire.state,
            suffix.with_target(wire.data_block)?,
            wire.source_offset,
        )
        .ok_or("source_offset: common-frame end overflows")?;
        if wire.byte_len != frame.byte_len() as u64 {
            return Err("byte_len disagrees with common frame");
        }
        if wire.index_source_offsets != frame.index_offsets() {
            return Err("index_source_offsets disagree with common frame");
        }
        if wire.state_source_offset != frame.state_offset() {
            return Err("state_source_offset disagrees with common frame");
        }
        if wire.local_ordinal_source_offset != frame.local_ordinal_offset() {
            return Err("local_ordinal_source_offset disagrees with common frame");
        }
        if wire.object_index_source_offset != frame.object_index_offset() {
            return Err("object_index_source_offset disagrees with common frame");
        }
        if wire.legacy_inactive_modules != frame.legacy_inactive_modules() {
            return Err("legacy_inactive_modules disagrees with state");
        }
        if wire.modifies_parasolid_data != frame.modifies_parasolid_data() {
            return Err("modifies_parasolid_data disagrees with state");
        }
        if wire.split_tracking_data != frame.split_tracking_data() {
            return Err("split_tracking_data disagrees with state");
        }
        if wire.group_count != frame.group_count() {
            return Err("group_count disagrees with state");
        }
        Ok(Self {
            id: wire.id,
            operation_record: wire.operation_record,
            ordinal: wire.ordinal,
            frame,
        })
    }
}

impl From<FeatureOperationTerminalFrame> for TerminalFrameWire {
    fn from(value: FeatureOperationTerminalFrame) -> Self {
        Self {
            id: value.id,
            operation_record: value.operation_record,
            immediate_common_frame: value.immediate_common_frame,
            local_ordinal: value.frame.suffix().local_ordinal(),
            raw_local_ordinal: value.frame.suffix().raw_local_ordinal().to_vec(),
            object_index: value.frame.suffix().object_index(),
            raw_object_index: value.frame.suffix().raw_object_index().to_vec(),
            data_block: value.frame.suffix().target().cloned().flatten(),
            source_offset: value.frame.offset(),
            object_index_source_offset: value.frame.object_index_offset(),
        }
    }
}

impl TryFrom<TerminalFrameWire> for FeatureOperationTerminalFrame {
    type Error = &'static str;
    fn try_from(wire: TerminalFrameWire) -> Result<Self, Self::Error> {
        let suffix = CommonFrameSuffix::from_wire(
            wire.local_ordinal,
            &wire.raw_local_ordinal,
            wire.object_index,
            &wire.raw_object_index,
        )?;
        let frame = TerminalFrame::<u64, Option<String>>::new(
            suffix.with_target(wire.data_block)?,
            wire.source_offset,
        )
        .ok_or("source_offset: terminal-frame end overflows")?;
        if wire.object_index_source_offset != frame.object_index_offset() {
            return Err("object_index_source_offset disagrees with terminal frame");
        }
        Ok(Self {
            id: wire.id,
            operation_record: wire.operation_record,
            immediate_common_frame: wire.immediate_common_frame,
            frame,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMON: &str = r#"{"id":"common","operation_record":"record","ordinal":0,"indices":[0,4097,0],"raw_indices":[[0],[144,1],[128,0]],"marker":[1,3,2],"state":[1,2,3,0,1,86,169,7],"legacy_inactive_modules":false,"modifies_parasolid_data":true,"split_tracking_data":[86,169],"group_count":7,"local_ordinal":1,"raw_local_ordinal":[1],"object_index":null,"raw_object_index":[255],"byte_len":20,"source_offset":100,"index_source_offsets":[100,101,103],"state_source_offset":108,"local_ordinal_source_offset":116,"object_index_source_offset":118}"#;

    #[test]
    fn common_frame_preserves_exact_wire_and_checks_every_derived_column() {
        let frame: FeatureOperationCommonFrame = serde_json::from_str(COMMON).unwrap();
        assert_eq!(serde_json::to_string(&frame).unwrap(), COMMON);
        for (field, value) in [
            ("indices", serde_json::json!([0, 4098, 0])),
            ("marker", serde_json::json!([1, 1, 1])),
            ("legacy_inactive_modules", serde_json::json!(true)),
            ("modifies_parasolid_data", serde_json::json!(false)),
            ("split_tracking_data", serde_json::json!([0, 0])),
            ("group_count", serde_json::json!(8)),
            ("raw_local_ordinal", serde_json::json!([128, 1])),
            ("raw_object_index", serde_json::json!([0])),
            ("byte_len", serde_json::json!(21)),
            ("index_source_offsets", serde_json::json!([100, 102, 103])),
            ("state_source_offset", serde_json::json!(109)),
            ("local_ordinal_source_offset", serde_json::json!(117)),
            ("object_index_source_offset", serde_json::json!(119)),
            ("source_offset", serde_json::json!(u64::MAX)),
            ("data_block", serde_json::json!("block")),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(COMMON).unwrap();
            wire[field] = value;
            let error = serde_json::from_value::<FeatureOperationCommonFrame>(wire)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(field) || (field == "indices" && error.contains("index")),
                "{error}"
            );
        }
    }

    #[test]
    fn common_frame_preserves_delete_prefix_unknown_flags_and_resolved_target() {
        let mut wire: serde_json::Value = serde_json::from_str(COMMON).unwrap();
        wire["indices"] = serde_json::json!([0, 0, 0]);
        wire["raw_indices"] = serde_json::json!([[0], [0], [0]]);
        wire["marker"] = serde_json::json!([1, 1, 1]);
        wire["byte_len"] = serde_json::json!(18);
        wire["index_source_offsets"] = serde_json::json!([100, 101, 102]);
        wire["state_source_offset"] = serde_json::json!(106);
        wire["local_ordinal_source_offset"] = serde_json::json!(114);
        wire["object_index_source_offset"] = serde_json::json!(116);
        wire["state"] = serde_json::json!([0, 0, 0, 2, 3, 5, 6, 9]);
        wire.as_object_mut()
            .unwrap()
            .remove("legacy_inactive_modules");
        wire.as_object_mut()
            .unwrap()
            .remove("modifies_parasolid_data");
        wire["split_tracking_data"] = serde_json::json!([5, 6]);
        wire["group_count"] = serde_json::json!(9);
        wire["object_index"] = serde_json::json!(0);
        wire["raw_object_index"] = serde_json::json!([0]);
        wire["data_block"] = serde_json::json!("block");
        let frame: FeatureOperationCommonFrame = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&frame).unwrap(), wire);
    }

    #[test]
    fn terminal_frame_preserves_nullable_wire_and_derives_object_position() {
        for (value, raw) in [("null", "[255]"), ("4096", "[144,16,0]")] {
            let wire = format!(
                r#"{{"id":"terminal","operation_record":"record","local_ordinal":128,"raw_local_ordinal":[128,128],"object_index":{value},"raw_object_index":{raw},"source_offset":100,"object_index_source_offset":104}}"#
            );
            let frame: FeatureOperationTerminalFrame = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&frame).unwrap(), wire);
            let mut invalid: serde_json::Value = serde_json::from_str(&wire).unwrap();
            invalid["object_index_source_offset"] = serde_json::json!(103);
            assert!(
                serde_json::from_value::<FeatureOperationTerminalFrame>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("object_index_source_offset")
            );
        }
    }
}
