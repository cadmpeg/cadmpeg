// SPDX-License-Identifier: Apache-2.0
//! Borrowed serialization of draft construction lanes.

use super::{
    FeatureDraftConstructionBinary32Lane, FeatureDraftConstructionFixedLane,
    FeatureDraftConstructionIdentityFrame, FeatureDraftConstructionIndexLane,
    FeatureDraftConstructionIndices, FeatureDraftConstructionTerminalLane,
};
use crate::iter_wire::IterWire;
use crate::om::compact::RawCompactIndex;
use serde::ser::SerializeMap;
use serde::Serialize;

impl Serialize for FeatureDraftConstructionIndexLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        macro_rules! indices {
            ($frame:expr) => {{
                wire.serialize_entry("declared_count", &usize::from($frame.declared_count()))?;
                wire.serialize_entry(
                    "indices",
                    &IterWire($frame.indices().map(|index| index.atom.value())),
                )?;
                wire.serialize_entry(
                    "raw_indices",
                    &IterWire($frame.indices().map(|index| RawCompactIndex(index.atom))),
                )?;
            }};
        }
        match &self.indices {
            FeatureDraftConstructionIndices::Unresolved(frame) => {
                indices!(frame);
                wire.serialize_entry(
                    "source_offsets",
                    &IterWire(frame.indices().map(|index| index.offset)),
                )?;
            }
            FeatureDraftConstructionIndices::Resolved(frame) => {
                indices!(frame);
                wire.serialize_entry(
                    "data_blocks",
                    &IterWire(frame.indices().map(|index| index.target.as_str())),
                )?;
                wire.serialize_entry(
                    "source_offsets",
                    &IterWire(frame.indices().map(|index| index.offset)),
                )?;
            }
        }
        wire.end()
    }
}

impl Serialize for FeatureDraftConstructionFixedLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("graph_payload", &self.graph_payload)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry(
            "values",
            &IterWire(self.lane.iter().map(|(_, atom, _)| atom.scalar.value())),
        )?;
        wire.serialize_entry(
            "markers",
            &IterWire(self.lane.iter().map(|(_, atom, _)| atom.marker.byte())),
        )?;
        wire.serialize_entry(
            "raw_values",
            &IterWire(self.lane.iter().map(|(_, atom, _)| atom.scalar.raw())),
        )?;
        wire.serialize_entry("payload_offset", &self.lane.offset())?;
        wire.serialize_entry(
            "value_payload_offsets",
            &IterWire(self.lane.iter().map(|(offset, _, _)| offset)),
        )?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.serialize_entry(
            "value_source_offsets",
            &IterWire(self.lane.iter().map(|(_, _, source)| *source)),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureDraftConstructionBinary32Lane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("graph_payload", &self.graph_payload)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("discriminator", &self.lane.form().discriminator())?;
        wire.serialize_entry("branch", &u8::from(self.lane.form()))?;
        wire.serialize_entry(
            "values",
            &IterWire(self.lane.iter().map(|(_, atom, _)| atom.value().get())),
        )?;
        wire.serialize_entry(
            "raw_values",
            &IterWire(self.lane.iter().map(|(_, atom, _)| atom.raw())),
        )?;
        wire.serialize_entry("payload_offset", &self.lane.offset())?;
        wire.serialize_entry(
            "value_payload_offsets",
            &IterWire(self.lane.iter().map(|(offset, _, _)| offset)),
        )?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.serialize_entry(
            "value_source_offsets",
            &IterWire(self.lane.iter().map(|(_, _, source)| *source)),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureDraftConstructionIdentityFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (prefix, length) = self.frame.prefix_stack();
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry(
            "draft_construction_payload",
            &self.draft_construction_payload,
        )?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("prefix", &prefix[..length])?;
        wire.serialize_entry("form", &self.frame.form())?;
        wire.serialize_entry("identity", self.frame.identity())?;
        wire.serialize_entry("payload_offset", &self.frame.offset())?;
        wire.serialize_entry("identity_payload_offset", &self.frame.identity_offset())?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.serialize_entry("identity_source_offset", &self.identity_source_offset)?;
        wire.end()
    }
}

impl Serialize for FeatureDraftConstructionTerminalLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry(
            "indices",
            &self.lane.indices().map(|token| token.atom.value()),
        )?;
        wire.serialize_entry(
            "raw_indices",
            &self.lane.indices().map(|token| *token.atom.raw()),
        )?;
        wire.serialize_entry("tail", &self.lane.tail())?;
        wire.serialize_entry(
            "index_source_offsets",
            &self.lane.indices().map(|token| token.offset),
        )?;
        wire.serialize_entry("source_offset", &self.lane.offset())?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        FeatureDraftConstructionBinary32LaneWire, FeatureDraftConstructionFixedLaneWire,
        FeatureDraftConstructionIdentityFrameWire, FeatureDraftConstructionIndexLaneWire,
        FeatureDraftConstructionTerminalLaneWire,
    };
    use super::{
        FeatureDraftConstructionBinary32Lane, FeatureDraftConstructionFixedLane,
        FeatureDraftConstructionIdentityFrame, FeatureDraftConstructionIndexLane,
        FeatureDraftConstructionTerminalLane,
    };

    macro_rules! wire_test {
        ($name:ident, $record:ty, $wire:ty, $json:expr) => {
            #[test]
            fn $name() {
                let json = $json;
                let record: $record = serde_json::from_str(json).unwrap();
                assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
                assert_eq!(
                    serde_json::to_vec(&record).unwrap(),
                    serde_json::to_vec(&<$wire>::from(record.clone())).unwrap()
                );
                cadmpeg_test_support::native_serialization::assert_native_limit(
                    &record,
                    serde_json::from_str::<serde_json::Value>(json).unwrap(),
                );
            }
        };
    }

    wire_test!(
        draft_index_borrowed_wire_matches_owned_bytes_and_retained_limit,
        FeatureDraftConstructionIndexLane,
        FeatureDraftConstructionIndexLaneWire,
        r#"{"id":"nx:feature:draft-index#0","operation_label":"operation","declared_count":3,"indices":[7,8],"raw_indices":[[7],[8]],"data_blocks":["first","second"],"source_offsets":[110,111]}"#
    );
    wire_test!(
        draft_fixed_borrowed_wire_matches_owned_bytes_and_retained_limit,
        FeatureDraftConstructionFixedLane,
        FeatureDraftConstructionFixedLaneWire,
        r#"{"id":"nx:feature:draft-fixed#0","operation_label":"operation","graph_payload":"payload","ordinal":0,"values":[0.25,0.5],"markers":[48,176],"raw_values":[[32,0,0,0,0,0,0],[64,0,0,0,0,0,0]],"payload_offset":0,"value_payload_offsets":[18,26],"source_offset":100,"value_source_offsets":[118,126]}"#
    );
    wire_test!(
        draft_binary32_borrowed_wire_matches_owned_bytes_and_retained_limit,
        FeatureDraftConstructionBinary32Lane,
        FeatureDraftConstructionBinary32LaneWire,
        r#"{"id":"nx:feature:draft-binary32#0","operation_label":"operation","graph_payload":"payload","ordinal":0,"discriminator":[144,24,69,1,4,1,3,1,192,69,4,0,128,134,2,0,3,0],"branch":3,"values":[2.5,4.0],"raw_values":[[80,32,0,0],[80,128,0,0]],"payload_offset":0,"value_payload_offsets":[18,22],"source_offset":100,"value_source_offsets":[118,122]}"#
    );
    wire_test!(
        draft_identity_borrowed_wire_matches_owned_bytes_and_retained_limit,
        FeatureDraftConstructionIdentityFrame,
        FeatureDraftConstructionIdentityFrameWire,
        r#"{"id":"nx:feature:draft-identity#0","operation_label":"operation","draft_construction_payload":"payload","ordinal":0,"prefix":[65,129,84,240,56,2,1],"form":{"kind":"indexed_branch","first_index":340,"second_index":56,"branch":2},"identity":"abc123","payload_offset":1,"identity_payload_offset":8,"source_offset":100,"identity_source_offset":500}"#
    );
    wire_test!(
        draft_terminal_borrowed_wire_matches_owned_bytes_and_retained_limit,
        FeatureDraftConstructionTerminalLane,
        FeatureDraftConstructionTerminalLaneWire,
        r#"{"id":"nx:feature:draft-terminal#0","operation_label":"operation","indices":[128,129],"raw_indices":[[128,128],[128,129]],"tail":[1,2,3],"index_source_offsets":[100,102],"source_offset":100}"#
    );
}
