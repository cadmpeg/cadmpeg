// SPDX-License-Identifier: Apache-2.0
//! Existing flat wire fields for complete column-row frames.

use super::*;
use crate::om::compact::CompactIndexAtom;
use crate::om::column_row::RowIndex;

fn atom(value: u32, raw: &[u8], field: &str) -> Result<CompactIndexAtom, String> {
    CompactIndexAtom::from_wire(value, raw).map_err(|error| format!("{field}: {error}"))
}

fn indices<const N: usize>(values: [u32; N], raw: [Vec<u8>; N]) -> [Result<CompactIndexAtom, String>; N] {
    std::array::from_fn(|i| atom(values[i], &raw[i], "indices/raw_indices"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DataBlockIndexRowWire {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// First non-null compact index.
    pub first_index: u32,
    /// Exact serialized leading-index token.
    pub raw_first_index: Vec<u8>,
    /// Serialized `03` or `07` row flag.
    pub flag: u8,
    /// Four ordered non-null compact indices after the row flag.
    pub indices: [u32; 4],
    /// Exact serialized four-index tokens in row order.
    pub raw_indices: [Vec<u8>; 4],
    /// Four same-section blocks addressed by the compact indices.
    pub data_blocks: [String; 4],
    /// Directory entry containing the offset-only store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
    /// Absolute file offset of the opening discriminator.
    pub source_offset: u64,
    /// Absolute file offset of the first compact index.
    pub first_index_source_offset: u64,
    /// Four ordered absolute file offsets of the compact indices.
    pub index_source_offsets: [u64; 4],
}

impl TryFrom<DataBlockIndexRowWire> for DataBlockIndexRow {
    type Error = String;
    fn try_from(wire: DataBlockIndexRowWire) -> Result<Self, Self::Error> {
        let [a, b, c, d] = indices(wire.indices, wire.raw_indices);
        let [a_block, b_block, c_block, d_block] = wire.data_blocks;
        let indices = [RowIndex { atom: a?, target: a_block }, RowIndex { atom: b?, target: b_block }, RowIndex { atom: c?, target: c_block }, RowIndex { atom: d?, target: d_block }];
        let first = atom(wire.first_index, &wire.raw_first_index, "first_index/raw_first_index")?;
        let flag = crate::om::discriminators::LinkedIndexFlag::try_from(wire.flag).map_err(|_| "flag: must be 3 or 7")?;
        let frame = IndexRow::<String, u64>::new(first, flag, indices, wire.source_offset).ok_or("source_offset: row extent overflows")?;
        if frame.first_index().offset != wire.first_index_source_offset { return Err("first_index_source_offset differs from the row layout".into()); }
        if frame.indices().map(|index| index.offset) != wire.index_source_offsets { return Err("index_source_offsets differ from the row layout".into()); }
        Ok(Self { frame, id: wire.id, section_ordinal: wire.section_ordinal, ordinal: wire.ordinal, source_entry: wire.source_entry, opening_data_block: wire.opening_data_block, opening_block_offset: wire.opening_block_offset })
    }
}
impl From<DataBlockIndexRow> for DataBlockIndexRowWire {
    fn from(value: DataBlockIndexRow) -> Self {
        Self {
            id: value.id,
            section_ordinal: value.section_ordinal,
            ordinal: value.ordinal,
            source_entry: value.source_entry,
            opening_data_block: value.opening_data_block,
            opening_block_offset: value.opening_block_offset,
            source_offset: value.frame.offset(),
            first_index: value.frame.first_index().atom.value(),
            raw_first_index: value.frame.first_index().atom.raw().to_vec(),
            first_index_source_offset: value.frame.first_index().offset,
            indices: value.frame.indices().map(|index| index.atom.value()),
            raw_indices: value.frame.indices().map(|index| index.atom.raw().to_vec()),
            index_source_offsets: value.frame.indices().map(|index| index.offset),
            data_blocks: value.frame.indices().map(|index| index.target.clone()),
            flag: u8::from(value.frame.flag()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DataBlockLinkedIndexRowWire {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// Unresolved leading compact index.
    pub first_index: u32,
    /// Exact serialized leading-index token.
    pub raw_first_index: Vec<u8>,
    /// Serialized `16`, `17`, or `18` discriminator.
    pub discriminator: crate::om::discriminators::LinkedIndexDiscriminator,
    /// Target compact block index.
    pub target_index: u32,
    /// Exact serialized target-index token.
    pub raw_target_index: Vec<u8>,
    /// Three compact block indices after `ff ff 90 fe`.
    pub indices: [u32; 3],
    /// Exact serialized post-marker tokens in row order.
    pub raw_indices: [Vec<u8>; 3],
    /// Target block followed by the three post-marker blocks.
    pub data_blocks: [String; 4],
    /// Serialized `03` or `07` flag.
    pub flag: crate::om::discriminators::LinkedIndexFlag,
    /// Serialized `04` or `07` mode.
    pub mode: crate::om::discriminators::IndexRowMode,
    /// Directory entry containing the store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
    /// Absolute file offset of the opening discriminator.
    pub source_offset: u64,
    /// Absolute file offset of the leading compact index.
    pub first_index_source_offset: u64,
    /// Absolute file offset of the target compact index.
    pub target_index_source_offset: u64,
    /// Absolute file offsets of the three post-marker indices.
    pub index_source_offsets: [u64; 3],
}

impl TryFrom<DataBlockLinkedIndexRowWire> for DataBlockLinkedIndexRow {
    type Error = String;
    fn try_from(wire: DataBlockLinkedIndexRowWire) -> Result<Self, Self::Error> {
        let [a, b, c] = indices(wire.indices, wire.raw_indices);
        let [target_block, a_block, b_block, c_block] = wire.data_blocks;
        let indices = [RowIndex { atom: a?, target: a_block }, RowIndex { atom: b?, target: b_block }, RowIndex { atom: c?, target: c_block }];
        let first = atom(wire.first_index, &wire.raw_first_index, "first_index/raw_first_index")?;
        let target = RowIndex { atom: atom(wire.target_index, &wire.raw_target_index, "target_index/raw_target_index")?, target: target_block };
        let frame = LinkedRow::<String, u64>::new(first, wire.discriminator, target, indices, wire.flag, wire.mode, wire.source_offset).ok_or("source_offset: row extent overflows")?;
        if frame.first_index().offset != wire.first_index_source_offset { return Err("first_index_source_offset differs from the row layout".into()); }
        if frame.target_index().offset != wire.target_index_source_offset { return Err("target_index_source_offset differs from the row layout".into()); }
        if frame.indices().map(|index| index.offset) != wire.index_source_offsets { return Err("index_source_offsets differ from the row layout".into()); }
        Ok(Self { frame, id: wire.id, section_ordinal: wire.section_ordinal, ordinal: wire.ordinal, source_entry: wire.source_entry, opening_data_block: wire.opening_data_block, opening_block_offset: wire.opening_block_offset })
    }
}
impl From<DataBlockLinkedIndexRow> for DataBlockLinkedIndexRowWire {
    fn from(value: DataBlockLinkedIndexRow) -> Self {
        Self {
            id: value.id,
            section_ordinal: value.section_ordinal,
            ordinal: value.ordinal,
            source_entry: value.source_entry,
            opening_data_block: value.opening_data_block,
            opening_block_offset: value.opening_block_offset,
            source_offset: value.frame.offset(),
            first_index: value.frame.first_index().atom.value(),
            raw_first_index: value.frame.first_index().atom.raw().to_vec(),
            first_index_source_offset: value.frame.first_index().offset,
            target_index: value.frame.target_index().atom.value(),
            raw_target_index: value.frame.target_index().atom.raw().to_vec(),
            target_index_source_offset: value.frame.target_index().offset,
            indices: value.frame.indices().map(|index| index.atom.value()),
            raw_indices: value.frame.indices().map(|index| index.atom.raw().to_vec()),
            index_source_offsets: value.frame.indices().map(|index| index.offset),
            data_blocks: [value.frame.target_index().target.clone(), value.frame.indices()[0].target.clone(), value.frame.indices()[1].target.clone(), value.frame.indices()[2].target.clone()],
            discriminator: value.frame.discriminator(),
            flag: value.frame.flag(),
            mode: value.frame.mode(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DataBlockTargetIndexRowWire {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// Target compact block index.
    pub target_index: u32,
    /// Exact serialized target-index token.
    pub raw_target_index: Vec<u8>,
    /// Three compact block indices after `ff ff 90 fe`.
    pub indices: [u32; 3],
    /// Exact serialized post-marker tokens in row order.
    pub raw_indices: [Vec<u8>; 3],
    /// Target block followed by the three post-marker blocks.
    pub data_blocks: [String; 4],
    /// Serialized `04` or `07` mode.
    pub mode: crate::om::discriminators::IndexRowMode,
    /// Directory entry containing the store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
    /// Absolute file offset of the opening discriminator.
    pub source_offset: u64,
    /// Absolute file offset of the target compact index.
    pub target_index_source_offset: u64,
    /// Absolute file offsets of the three post-marker indices.
    pub index_source_offsets: [u64; 3],
}

impl TryFrom<DataBlockTargetIndexRowWire> for DataBlockTargetIndexRow {
    type Error = String;
    fn try_from(wire: DataBlockTargetIndexRowWire) -> Result<Self, Self::Error> {
        let [a, b, c] = indices(wire.indices, wire.raw_indices);
        let [target_block, a_block, b_block, c_block] = wire.data_blocks;
        let indices = [RowIndex { atom: a?, target: a_block }, RowIndex { atom: b?, target: b_block }, RowIndex { atom: c?, target: c_block }];
        let target = RowIndex { atom: atom(wire.target_index, &wire.raw_target_index, "target_index/raw_target_index")?, target: target_block };
        let frame = TargetRow::<String, u64>::new(target, indices, wire.mode, wire.source_offset).ok_or("source_offset: row extent overflows")?;
        if frame.target_index().offset != wire.target_index_source_offset { return Err("target_index_source_offset differs from the row layout".into()); }
        if frame.indices().map(|index| index.offset) != wire.index_source_offsets { return Err("index_source_offsets differ from the row layout".into()); }
        Ok(Self { frame, id: wire.id, section_ordinal: wire.section_ordinal, ordinal: wire.ordinal, source_entry: wire.source_entry, opening_data_block: wire.opening_data_block, opening_block_offset: wire.opening_block_offset })
    }
}
impl From<DataBlockTargetIndexRow> for DataBlockTargetIndexRowWire {
    fn from(value: DataBlockTargetIndexRow) -> Self {
        Self {
            id: value.id,
            section_ordinal: value.section_ordinal,
            ordinal: value.ordinal,
            source_entry: value.source_entry,
            opening_data_block: value.opening_data_block,
            opening_block_offset: value.opening_block_offset,
            source_offset: value.frame.offset(),
            target_index: value.frame.target_index().atom.value(),
            raw_target_index: value.frame.target_index().atom.raw().to_vec(),
            target_index_source_offset: value.frame.target_index().offset,
            indices: value.frame.indices().map(|index| index.atom.value()),
            raw_indices: value.frame.indices().map(|index| index.atom.raw().to_vec()),
            index_source_offsets: value.frame.indices().map(|index| index.offset),
            data_blocks: [value.frame.target_index().target.clone(), value.frame.indices()[0].target.clone(), value.frame.indices()[1].target.clone(), value.frame.indices()[2].target.clone()],
            mode: value.frame.mode(),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    fn check_wire<T: serde::de::DeserializeOwned + Serialize + std::fmt::Debug>(json: &str, field: &str, invalid: serde_json::Value) {
        let row: T = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&row).unwrap(), json);
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = invalid;
        let error = serde_json::from_value::<T>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
        let original: serde_json::Value = serde_json::from_str(json).unwrap();
        for field in ["first_index_source_offset", "object_index_source_offset", "target_index_source_offset", "source_offset"] {
            if original.get(field).is_none() { continue; }
            let mut invalid = original.clone();
            invalid[field] = serde_json::json!(u64::MAX);
            let error = serde_json::from_value::<T>(invalid).unwrap_err();
            assert!(error.to_string().contains(field), "{error}");
        }
        if let Some(offsets) = original.get("index_source_offsets").and_then(serde_json::Value::as_array) {
            for index in 0..offsets.len() {
                let mut invalid = original.clone();
                invalid["index_source_offsets"][index] = serde_json::json!(u64::MAX);
                let error = serde_json::from_value::<T>(invalid).unwrap_err();
                assert!(error.to_string().contains("index_source_offsets"), "{error}");
            }
        }
    }

    #[test]
    fn column_rows_keep_wire_order_and_reject_mismatched_tokens() {
        check_wire::<DataBlockIndexRow>(
            r#"{"id":"row","section_ordinal":0,"ordinal":0,"first_index":1,"raw_first_index":[1],"flag":3,"indices":[2,3,4,5],"raw_indices":[[2],[3],[4],[5]],"data_blocks":["a","b","c","d"],"source_entry":"entry","opening_data_block":"opening","opening_block_offset":0,"source_offset":10,"first_index_source_offset":13,"index_source_offsets":[17,18,19,20]}"#,
            "raw_first_index", serde_json::json!([255]),
        );
        check_wire::<DataBlockLinkedIndexRow>(
            r#"{"id":"row","section_ordinal":0,"ordinal":0,"first_index":1,"raw_first_index":[1],"discriminator":22,"target_index":2,"raw_target_index":[2],"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"data_blocks":["a","b","c","d"],"flag":3,"mode":4,"source_entry":"entry","opening_data_block":"opening","opening_block_offset":0,"source_offset":10,"first_index_source_offset":12,"target_index_source_offset":16,"index_source_offsets":[21,22,23]}"#,
            "raw_target_index", serde_json::json!([3]),
        );
        check_wire::<DataBlockTargetIndexRow>(
            r#"{"id":"row","section_ordinal":0,"ordinal":0,"target_index":2,"raw_target_index":[2],"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"data_blocks":["a","b","c","d"],"mode":7,"source_entry":"entry","opening_data_block":"opening","opening_block_offset":0,"source_offset":10,"target_index_source_offset":15,"index_source_offsets":[20,21,22]}"#,
            "raw_indices", serde_json::json!([[3], [4], [6]]),
        );
    }
}
