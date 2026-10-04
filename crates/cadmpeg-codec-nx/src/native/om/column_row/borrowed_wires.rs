// SPDX-License-Identifier: Apache-2.0
//! Borrowed serialization of resolved column rows.

use super::{DataBlockIndexRow, DataBlockLinkedIndexRow, DataBlockTargetIndexRow};
use crate::om::compact::RawCompactIndex;
use serde::ser::SerializeMap;
use serde::Serialize;

macro_rules! common_open {
    ($wire:ident, $row:ident) => {
        $wire.serialize_entry("id", &$row.id)?;
        $wire.serialize_entry("section_ordinal", &$row.section_ordinal)?;
        $wire.serialize_entry("ordinal", &$row.ordinal)?;
    };
}
macro_rules! common_source {
    ($wire:ident, $row:ident) => {
        $wire.serialize_entry("source_entry", &$row.source_entry)?;
        $wire.serialize_entry("opening_data_block", &$row.opening_data_block)?;
        $wire.serialize_entry("opening_block_offset", &$row.opening_block_offset)?;
        $wire.serialize_entry("source_offset", &$row.frame.offset())?;
    };
}
macro_rules! indices {
    ($wire:ident, $row:ident) => {
        $wire.serialize_entry(
            "indices",
            &$row.frame.indices().map(|index| index.atom.value()),
        )?;
        $wire.serialize_entry(
            "raw_indices",
            &$row
                .frame
                .indices()
                .map(|index| RawCompactIndex(index.atom)),
        )?;
    };
}

impl Serialize for DataBlockIndexRow {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        common_open!(wire, self);
        wire.serialize_entry("first_index", &self.frame.first_index().atom.value())?;
        wire.serialize_entry(
            "raw_first_index",
            &RawCompactIndex(self.frame.first_index().atom),
        )?;
        wire.serialize_entry("flag", &u8::from(self.frame.flag()))?;
        indices!(wire, self);
        wire.serialize_entry(
            "data_blocks",
            &self.frame.indices().map(|index| index.target.as_str()),
        )?;
        common_source!(wire, self);
        wire.serialize_entry(
            "first_index_source_offset",
            &self.frame.first_index().offset,
        )?;
        wire.serialize_entry(
            "index_source_offsets",
            &self.frame.indices().map(|index| index.offset),
        )?;
        wire.end()
    }
}

impl Serialize for DataBlockLinkedIndexRow {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        common_open!(wire, self);
        wire.serialize_entry("first_index", &self.frame.first_index().atom.value())?;
        wire.serialize_entry(
            "raw_first_index",
            &RawCompactIndex(self.frame.first_index().atom),
        )?;
        wire.serialize_entry("discriminator", &self.frame.discriminator())?;
        wire.serialize_entry("target_index", &self.frame.target_index().atom.value())?;
        wire.serialize_entry(
            "raw_target_index",
            &RawCompactIndex(self.frame.target_index().atom),
        )?;
        indices!(wire, self);
        wire.serialize_entry(
            "data_blocks",
            &[
                self.frame.target_index().target.as_str(),
                self.frame.indices()[0].target.as_str(),
                self.frame.indices()[1].target.as_str(),
                self.frame.indices()[2].target.as_str(),
            ],
        )?;
        wire.serialize_entry("flag", &self.frame.flag())?;
        wire.serialize_entry("mode", &self.frame.mode())?;
        common_source!(wire, self);
        wire.serialize_entry(
            "first_index_source_offset",
            &self.frame.first_index().offset,
        )?;
        wire.serialize_entry(
            "target_index_source_offset",
            &self.frame.target_index().offset,
        )?;
        wire.serialize_entry(
            "index_source_offsets",
            &self.frame.indices().map(|index| index.offset),
        )?;
        wire.end()
    }
}

impl Serialize for DataBlockTargetIndexRow {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        common_open!(wire, self);
        wire.serialize_entry("target_index", &self.frame.target_index().atom.value())?;
        wire.serialize_entry(
            "raw_target_index",
            &RawCompactIndex(self.frame.target_index().atom),
        )?;
        indices!(wire, self);
        wire.serialize_entry(
            "data_blocks",
            &[
                self.frame.target_index().target.as_str(),
                self.frame.indices()[0].target.as_str(),
                self.frame.indices()[1].target.as_str(),
                self.frame.indices()[2].target.as_str(),
            ],
        )?;
        wire.serialize_entry("mode", &self.frame.mode())?;
        common_source!(wire, self);
        wire.serialize_entry(
            "target_index_source_offset",
            &self.frame.target_index().offset,
        )?;
        wire.serialize_entry(
            "index_source_offsets",
            &self.frame.indices().map(|index| index.offset),
        )?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_test_support::native_serialization::assert_native_limit;
    use serde::de::DeserializeOwned;

    fn bytes_and_limit<T, W>(json: &str)
    where
        T: DeserializeOwned + Serialize + Clone,
        W: Serialize + From<T>,
    {
        let json = json.replace("\"id\":\"row\"", "\"id\":\"nx:om-column-row:row#0\"");
        let record: T = serde_json::from_str(&json).unwrap();
        let borrowed = serde_json::to_vec(&record).unwrap();
        assert_eq!(
            borrowed,
            serde_json::to_vec(&W::from(record.clone())).unwrap()
        );
        assert_eq!(borrowed, json.as_bytes());
        assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        );
    }

    #[test]
    fn index_row_borrowed_bytes_and_limit() {
        bytes_and_limit::<DataBlockIndexRow, super::super::wire::DataBlockIndexRowWire>(
            r#"{"id":"row","section_ordinal":0,"ordinal":0,"first_index":1,"raw_first_index":[1],"flag":3,"indices":[2,3,4,5],"raw_indices":[[2],[3],[4],[5]],"data_blocks":["a","b","c","d"],"source_entry":"entry","opening_data_block":"opening","opening_block_offset":0,"source_offset":10,"first_index_source_offset":13,"index_source_offsets":[17,18,19,20]}"#,
        );
    }

    #[test]
    fn linked_row_borrowed_bytes_and_limit() {
        bytes_and_limit::<DataBlockLinkedIndexRow, super::super::wire::DataBlockLinkedIndexRowWire>(
            r#"{"id":"row","section_ordinal":0,"ordinal":0,"first_index":1,"raw_first_index":[1],"discriminator":22,"target_index":2,"raw_target_index":[2],"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"data_blocks":["a","b","c","d"],"flag":3,"mode":4,"source_entry":"entry","opening_data_block":"opening","opening_block_offset":0,"source_offset":10,"first_index_source_offset":12,"target_index_source_offset":16,"index_source_offsets":[21,22,23]}"#,
        );
    }

    #[test]
    fn target_row_borrowed_bytes_and_limit() {
        bytes_and_limit::<DataBlockTargetIndexRow, super::super::wire::DataBlockTargetIndexRowWire>(
            r#"{"id":"row","section_ordinal":0,"ordinal":0,"target_index":2,"raw_target_index":[2],"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"data_blocks":["a","b","c","d"],"mode":7,"source_entry":"entry","opening_data_block":"opening","opening_block_offset":0,"source_offset":10,"target_index_source_offset":15,"index_source_offsets":[20,21,22]}"#,
        );
    }
}
