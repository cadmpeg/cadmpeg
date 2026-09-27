// SPDX-License-Identifier: Apache-2.0
//! Borrowed native wires for hole feature records.

use super::{
    FeatureHolePackageConstructionGroupLane, FeatureSimpleHoleConstructionGroup,
    FeatureSimpleHoleRepeatedScalarLane, FeatureSimpleHoleRepeatedScalarLaneBlockReferences,
    FeatureSymbolicThreadTextFrame,
};
use crate::iter_wire::IterWire;
use serde::ser::SerializeMap;
use serde::Serialize;

impl Serialize for FeatureSymbolicThreadTextFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("symbolic_thread", &self.symbolic_thread)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("marker", &3_u8)?;
        wire.serialize_entry("value", &self.value)?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.end()
    }
}

impl Serialize for FeatureSimpleHoleRepeatedScalarLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry(
            "values",
            &IterWire(self.values.iter().map(|token| token.scalar.value().get())),
        )?;
        wire.serialize_entry(
            "raw_values",
            &IterWire(self.values.iter().map(|token| token.scalar.raw())),
        )?;
        wire.serialize_entry(
            "first_witness_offsets",
            &IterWire(self.values.iter().map(|token| token.witness_offsets[0])),
        )?;
        wire.serialize_entry(
            "second_witness_offsets",
            &IterWire(self.values.iter().map(|token| token.witness_offsets[1])),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry(
            "first_data_blocks",
            &self
                .first
                .references
                .each_ref()
                .map(|item| item.data_block.as_str()),
        )?;
        wire.serialize_entry(
            "second_data_blocks",
            &self
                .second
                .references
                .each_ref()
                .map(|item| item.data_block.as_str()),
        )?;
        if self.first.wrapped {
            wire.serialize_entry(
                "first_reference_prefix",
                &crate::om::simple_hole_references::FIRST_PREFIX,
            )?;
        }
        if self.second.wrapped {
            wire.serialize_entry(
                "second_reference_prefix",
                &crate::om::simple_hole_references::SECOND_PREFIX,
            )?;
        }
        wire.serialize_entry(
            "first_reference_offsets",
            &self
                .first
                .references
                .each_ref()
                .map(|item| item.source_offset),
        )?;
        wire.serialize_entry(
            "second_reference_offsets",
            &self
                .second
                .references
                .each_ref()
                .map(|item| item.source_offset),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureSimpleHoleConstructionGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("first_data_blocks", &self.first_data_blocks)?;
        wire.serialize_entry("second_data_blocks", &self.second_data_blocks)?;
        wire.serialize_entry(
            "operation_labels",
            &IterWire(
                self.members
                    .iter()
                    .map(|item| item.operation_label.as_str()),
            ),
        )?;
        wire.serialize_entry(
            "scalar_lanes",
            &IterWire(self.members.iter().map(|item| item.scalar_lane.as_str())),
        )?;
        wire.serialize_entry(
            "block_references",
            &IterWire(
                self.members
                    .iter()
                    .map(|item| item.block_reference.as_str()),
            ),
        )?;
        wire.end()
    }
}

impl Serialize for FeatureHolePackageConstructionGroupLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("selector", &self.selector.get())?;
        wire.serialize_entry("branch", &self.branch.get())?;
        wire.serialize_entry(
            "object_indices",
            &self.references.each_ref().map(|item| item.token.value()),
        )?;
        wire.serialize_entry(
            "raw_object_indices",
            &self.references.each_ref().map(|item| item.token.raw()),
        )?;
        wire.serialize_entry(
            "data_blocks",
            &self
                .references
                .each_ref()
                .map(|item| item.data_block.as_str()),
        )?;
        wire.serialize_entry("payload_offset", &self.payload_offset)?;
        wire.serialize_entry("source_offset", &self.source_offset)?;
        wire.serialize_entry(
            "reference_source_offsets",
            &self.references.each_ref().map(|item| item.source_offset),
        )?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        FeatureHolePackageConstructionGroupLaneWire, FeatureSimpleHoleConstructionGroupWire,
        FeatureSimpleHoleRepeatedScalarLaneBlockReferencesWire,
        FeatureSimpleHoleRepeatedScalarLaneWire, FeatureSymbolicThreadTextFrameWire,
    };
    use super::{
        FeatureHolePackageConstructionGroupLane, FeatureSimpleHoleConstructionGroup,
        FeatureSimpleHoleRepeatedScalarLane, FeatureSimpleHoleRepeatedScalarLaneBlockReferences,
        FeatureSymbolicThreadTextFrame,
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
        symbolic_thread_borrowed_wire_matches_owned_bytes_and_retained_limit,
        FeatureSymbolicThreadTextFrame,
        FeatureSymbolicThreadTextFrameWire,
        r#"{"id":"nx:feature:symbolic-thread-text#0","symbolic_thread":"thread","ordinal":0,"marker":3,"value":"CUT","source_offset":10}"#
    );
    wire_test!(
        simple_hole_scalar_borrowed_wire_matches_owned_bytes_and_retained_limit,
        FeatureSimpleHoleRepeatedScalarLane,
        FeatureSimpleHoleRepeatedScalarLaneWire,
        r#"{"id":"nx:feature:simple-hole-scalar#0","operation_label":"operation","values":[2.5,4.0],"raw_values":[[48,4,0,0,0,0,0,0],[48,16,0,0,0,0,0,0]],"first_witness_offsets":[10,18],"second_witness_offsets":[40,48]}"#
    );
    wire_test!(
        simple_hole_block_refs_borrowed_wire_matches_owned_bytes_and_retained_limit,
        FeatureSimpleHoleRepeatedScalarLaneBlockReferences,
        FeatureSimpleHoleRepeatedScalarLaneBlockReferencesWire,
        r#"{"id":"nx:feature:simple-hole-block-refs#0","operation_label":"operation","first_data_blocks":["a","b"],"second_data_blocks":["c","d"],"first_reference_offsets":[10,13],"second_reference_offsets":[30,32]}"#
    );
    wire_test!(
        simple_hole_group_borrowed_wire_matches_owned_bytes_and_retained_limit,
        FeatureSimpleHoleConstructionGroup,
        FeatureSimpleHoleConstructionGroupWire,
        r#"{"id":"nx:feature:simple-hole-group#0","first_data_blocks":["a","b"],"second_data_blocks":["c","d"],"operation_labels":["first","second"],"scalar_lanes":["scalar-a","scalar-b"],"block_references":["refs-a","refs-b"]}"#
    );
    wire_test!(
        hole_package_lane_borrowed_wire_matches_owned_bytes_and_retained_limit,
        FeatureHolePackageConstructionGroupLane,
        FeatureHolePackageConstructionGroupLaneWire,
        r#"{"id":"nx:feature:hole-package-lane#0","operation_label":"o","selector":70,"branch":17,"object_indices":[1,2,3,4],"raw_object_indices":[[240,1],[240,2],[240,3],[240,4]],"data_blocks":["a","b","c","d"],"payload_offset":20,"source_offset":120,"reference_source_offsets":[132,134,141,143]}"#
    );
}
