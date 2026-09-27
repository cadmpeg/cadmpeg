// SPDX-License-Identifier: Apache-2.0
//! Borrowed serialization of the structured EXTRUDE branch.

use super::FeatureExtrudePayload32Branch;
use crate::iter_wire::IterWire;
use crate::om::compact::RawCompactIndex;
use serde::ser::SerializeMap;
use serde::Serialize;

struct RawFeatureToken(crate::om::reference_index::FeatureReferenceToken);

impl Serialize for RawFeatureToken {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.raw().serialize(serializer)
    }
}

impl Serialize for FeatureExtrudePayload32Branch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        let frame = &self.frame;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("body_object_index", &frame.terminal().value())?;
        wire.serialize_entry("scalar", &frame.scalar().value().get())?;
        wire.serialize_entry("raw_scalar", &frame.scalar().raw())?;
        wire.serialize_entry(
            "atoms_be",
            &IterWire(frame.atoms().map(|(token, _, _)| token.raw())),
        )?;
        wire.serialize_entry(
            "atom_source_offsets",
            &IterWire(frame.atoms().map(|(_, _, offset)| offset)),
        )?;
        wire.serialize_entry(
            "atom_indices",
            &IterWire(frame.atoms().map(|(token, _, _)| token.value())),
        )?;
        wire.serialize_entry(
            "atom_data_blocks",
            &IterWire(frame.atoms().map(|(_, binding, _)| binding.as_deref())),
        )?;
        wire.serialize_entry(
            "first_indices",
            &IterWire(frame.first_indices().map(|(token, _, _)| token.value())),
        )?;
        wire.serialize_entry(
            "raw_first_indices",
            &IterWire(
                frame
                    .first_indices()
                    .map(|(token, _, _)| RawCompactIndex(token)),
            ),
        )?;
        wire.serialize_entry(
            "first_index_source_offsets",
            &IterWire(frame.first_indices().map(|(_, _, offset)| offset)),
        )?;
        wire.serialize_entry(
            "first_data_blocks",
            &IterWire(
                frame
                    .first_indices()
                    .map(|(_, binding, _)| binding.as_deref()),
            ),
        )?;
        wire.serialize_entry(
            "second_indices",
            &IterWire(frame.second_indices().map(|(token, _, _)| token.value())),
        )?;
        wire.serialize_entry(
            "raw_second_indices",
            &IterWire(
                frame
                    .second_indices()
                    .map(|(token, _, _)| RawCompactIndex(token)),
            ),
        )?;
        wire.serialize_entry(
            "second_index_source_offsets",
            &IterWire(frame.second_indices().map(|(_, _, offset)| offset)),
        )?;
        wire.serialize_entry(
            "second_data_blocks",
            &IterWire(
                frame
                    .second_indices()
                    .map(|(_, binding, _)| binding.as_deref()),
            ),
        )?;
        wire.serialize_entry("terminal_object_index", &frame.terminal().value())?;
        wire.serialize_entry(
            "raw_terminal_object_index",
            &RawFeatureToken(frame.terminal()),
        )?;
        wire.serialize_entry("terminal_source_offset", &frame.terminal_offset())?;
        wire.serialize_entry("source_offset", &frame.origin())?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_test_support::native_serialization::assert_native_limit;

    #[test]
    fn extrude_32_branch_borrowed_bytes_and_limit() {
        let json = r#"{"id":"nx:feature:extrude32#0","operation_label":"operation","body_object_index":42,"scalar":1.0,"raw_scalar":[47,240,0,0,0,0,0,0],"atoms_be":[1031799040],"atom_source_offsets":[33],"atom_indices":[1],"atom_data_blocks":["block#1"],"first_indices":[2],"raw_first_indices":[[2]],"first_index_source_offsets":[39],"first_data_blocks":[null],"second_indices":[3],"raw_second_indices":[[3]],"second_index_source_offsets":[42],"second_data_blocks":["block#3"],"terminal_object_index":42,"raw_terminal_object_index":[42],"terminal_source_offset":45,"source_offset":20}"#;
        let record: FeatureExtrudePayload32Branch = serde_json::from_str(json).unwrap();
        let borrowed = serde_json::to_vec(&record).unwrap();
        let owned = serde_json::to_vec(&super::super::FeatureExtrudePayload32BranchWire::from(
            record.clone(),
        ))
        .unwrap();
        assert_eq!(borrowed, owned);
        assert_eq!(borrowed, json.as_bytes());
        assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}
