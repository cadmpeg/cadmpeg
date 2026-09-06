// SPDX-License-Identifier: Apache-2.0
//! Wire adapters for column rows with checked compact-index tokens.
use super::*;

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
        Ok(Self {
            id: wire.id,
            section_ordinal: wire.section_ordinal,
            ordinal: wire.ordinal,
            source_entry: wire.source_entry,
            opening_data_block: wire.opening_data_block,
            opening_block_offset: wire.opening_block_offset,
            source_offset: wire.source_offset,
            first_index: located_index(
                wire.first_index,
                &wire.raw_first_index,
                wire.first_index_source_offset,
                "first_index/raw_first_index",
            )?,
            indices: resolved_indices(
                wire.indices,
                wire.raw_indices,
                wire.data_blocks,
                wire.index_source_offsets,
            )?,
            flag: crate::om::discriminators::LinkedIndexFlag::try_from(wire.flag)
                .map_err(|_| "flag: must be 3 or 7")?,
        })
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
            source_offset: value.source_offset,
            first_index: value.first_index.atom.value(),
            raw_first_index: value.first_index.atom.raw().to_vec(),
            first_index_source_offset: value.first_index.offset,
            indices: value
                .indices
                .each_ref()
                .map(|token| token.target.atom.value()),
            raw_indices: value
                .indices
                .each_ref()
                .map(|token| token.target.atom.raw().to_vec()),
            index_source_offsets: value.indices.each_ref().map(|token| token.source_offset),
            data_blocks: value.indices.map(|token| token.target.data_block),
            flag: u8::from(value.flag),
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
        let [target_block, block_a, block_b, block_c] = wire.data_blocks;
        Ok(Self {
            id: wire.id,
            section_ordinal: wire.section_ordinal,
            ordinal: wire.ordinal,
            source_entry: wire.source_entry,
            opening_data_block: wire.opening_data_block,
            opening_block_offset: wire.opening_block_offset,
            source_offset: wire.source_offset,
            first_index: located_index(
                wire.first_index,
                &wire.raw_first_index,
                wire.first_index_source_offset,
                "first_index/raw_first_index",
            )?,
            target: DataBlockIndexToken {
                target: DataBlockIndexTarget {
                    atom: CompactIndexAtom::from_wire(wire.target_index, &wire.raw_target_index)
                        .map_err(|error| format!("target_index/raw_target_index: {error}"))?,
                    data_block: target_block,
                },
                source_offset: wire.target_index_source_offset,
            },
            indices: resolved_indices(
                wire.indices,
                wire.raw_indices,
                [block_a, block_b, block_c],
                wire.index_source_offsets,
            )?,
            discriminator: wire.discriminator,
            flag: wire.flag,
            mode: wire.mode,
        })
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
            source_offset: value.source_offset,
            first_index: value.first_index.atom.value(),
            raw_first_index: value.first_index.atom.raw().to_vec(),
            first_index_source_offset: value.first_index.offset,
            target_index: value.target.target.atom.value(),
            raw_target_index: value.target.target.atom.raw().to_vec(),
            target_index_source_offset: value.target.source_offset,
            indices: value
                .indices
                .each_ref()
                .map(|token| token.target.atom.value()),
            raw_indices: value
                .indices
                .each_ref()
                .map(|token| token.target.atom.raw().to_vec()),
            index_source_offsets: value.indices.each_ref().map(|token| token.source_offset),
            data_blocks: [
                value.target.target.data_block,
                value.indices[0].target.data_block.clone(),
                value.indices[1].target.data_block.clone(),
                value.indices[2].target.data_block.clone(),
            ],
            discriminator: value.discriminator,
            flag: value.flag,
            mode: value.mode,
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
        let [target_block, block_a, block_b, block_c] = wire.data_blocks;
        Ok(Self {
            id: wire.id,
            section_ordinal: wire.section_ordinal,
            ordinal: wire.ordinal,
            source_entry: wire.source_entry,
            opening_data_block: wire.opening_data_block,
            opening_block_offset: wire.opening_block_offset,
            source_offset: wire.source_offset,
            target: DataBlockIndexToken {
                target: DataBlockIndexTarget {
                    atom: CompactIndexAtom::from_wire(wire.target_index, &wire.raw_target_index)
                        .map_err(|error| format!("target_index/raw_target_index: {error}"))?,
                    data_block: target_block,
                },
                source_offset: wire.target_index_source_offset,
            },
            indices: resolved_indices(
                wire.indices,
                wire.raw_indices,
                [block_a, block_b, block_c],
                wire.index_source_offsets,
            )?,
            mode: wire.mode,
        })
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
            source_offset: value.source_offset,
            target_index: value.target.target.atom.value(),
            raw_target_index: value.target.target.atom.raw().to_vec(),
            target_index_source_offset: value.target.source_offset,
            indices: value
                .indices
                .each_ref()
                .map(|token| token.target.atom.value()),
            raw_indices: value
                .indices
                .each_ref()
                .map(|token| token.target.atom.raw().to_vec()),
            index_source_offsets: value.indices.each_ref().map(|token| token.source_offset),
            data_blocks: [
                value.target.target.data_block,
                value.indices[0].target.data_block.clone(),
                value.indices[1].target.data_block.clone(),
                value.indices[2].target.data_block.clone(),
            ],
            mode: value.mode,
        }
    }
}

pub(super) fn located_index(
    value: u32,
    raw: &[u8],
    offset: u64,
    field: &str,
) -> Result<LocatedCompactIndex<u64>, String> {
    Ok(LocatedCompactIndex {
        atom: CompactIndexAtom::from_wire(value, raw)
            .map_err(|error| format!("{field}: {error}"))?,
        offset,
    })
}

pub(super) fn located_indices<const N: usize>(
    values: [u32; N],
    raw: [Vec<u8>; N],
    offsets: [u64; N],
) -> Result<[LocatedCompactIndex<u64>; N], String> {
    values
        .into_iter()
        .zip(raw)
        .zip(offsets)
        .map(|((value, raw), offset)| located_index(value, &raw, offset, "indices/raw_indices"))
        .collect::<Result<Vec<_>, String>>()?
        .try_into()
        .map_err(|_| "indices: column width mismatch".into())
}

fn resolved_indices<const N: usize>(
    values: [u32; N],
    raw: [Vec<u8>; N],
    blocks: [String; N],
    offsets: [u64; N],
) -> Result<[DataBlockIndexToken; N], String> {
    values
        .into_iter()
        .zip(raw)
        .zip(blocks)
        .zip(offsets)
        .map(|(((value, raw), data_block), source_offset)| {
            Ok(DataBlockIndexToken {
                target: DataBlockIndexTarget {
                    atom: CompactIndexAtom::from_wire(value, &raw)
                        .map_err(|error| format!("indices/raw_indices: {error}"))?,
                    data_block,
                },
                source_offset,
            })
        })
        .collect::<Result<Vec<_>, String>>()?
        .try_into()
        .map_err(|_| "indices: column width mismatch".into())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum RmDisplayColorAssignmentEncodingWire {
    /// Linked row with an unresolved leading object identity.
    Linked {
        /// Unresolved leading object identity.
        object_index: u32,
        /// Exact leading-object token.
        raw_object_index: Vec<u8>,
        /// Absolute leading-object token offset.
        object_index_source_offset: u64,
        /// Row discriminator.
        discriminator: crate::om::discriminators::LinkedIndexDiscriminator,
        /// Target index.
        target_index: u32,
        /// Exact target-index token.
        raw_target_index: Vec<u8>,
        /// Absolute target-index token offset.
        target_index_source_offset: u64,
        /// Three post-marker indices.
        indices: [u32; 3],
        /// Exact post-marker index tokens.
        raw_indices: [Vec<u8>; 3],
        /// Absolute post-marker token offsets.
        index_source_offsets: [u64; 3],
        /// Row flag.
        flag: crate::om::discriminators::LinkedIndexFlag,
        /// Row mode.
        mode: crate::om::discriminators::IndexRowMode,
    },
    /// Target-index row without a leading object identity.
    Target {
        /// Target index.
        target_index: u32,
        /// Exact target-index token.
        raw_target_index: Vec<u8>,
        /// Absolute target-index token offset.
        target_index_source_offset: u64,
        /// Three post-marker indices.
        indices: [u32; 3],
        /// Exact post-marker index tokens.
        raw_indices: [Vec<u8>; 3],
        /// Absolute post-marker token offsets.
        index_source_offsets: [u64; 3],
        /// Row mode.
        mode: crate::om::discriminators::IndexRowMode,
    },
}

impl From<RmDisplayColorAssignmentEncoding> for RmDisplayColorAssignmentEncodingWire {
    fn from(value: RmDisplayColorAssignmentEncoding) -> Self {
        match value {
            RmDisplayColorAssignmentEncoding::Linked {
                object_index,
                discriminator,
                target_index,
                indices,
                flag,
                mode,
            } => Self::Linked {
                object_index: object_index.atom.value(),
                raw_object_index: object_index.atom.raw().to_vec(),
                object_index_source_offset: object_index.offset,
                discriminator,
                target_index: target_index.atom.value(),
                raw_target_index: target_index.atom.raw().to_vec(),
                target_index_source_offset: target_index.offset,
                indices: indices.map(|token| token.atom.value()),
                raw_indices: indices.map(|token| token.atom.raw().to_vec()),
                index_source_offsets: indices.map(|token| token.offset),
                flag,
                mode,
            },
            RmDisplayColorAssignmentEncoding::Target {
                target_index,
                indices,
                mode,
            } => Self::Target {
                target_index: target_index.atom.value(),
                raw_target_index: target_index.atom.raw().to_vec(),
                target_index_source_offset: target_index.offset,
                indices: indices.map(|token| token.atom.value()),
                raw_indices: indices.map(|token| token.atom.raw().to_vec()),
                index_source_offsets: indices.map(|token| token.offset),
                mode,
            },
        }
    }
}
impl TryFrom<RmDisplayColorAssignmentEncodingWire> for RmDisplayColorAssignmentEncoding {
    type Error = String;
    fn try_from(value: RmDisplayColorAssignmentEncodingWire) -> Result<Self, Self::Error> {
        Ok(match value {
            RmDisplayColorAssignmentEncodingWire::Linked {
                object_index,
                raw_object_index,
                object_index_source_offset,
                discriminator,
                target_index,
                raw_target_index,
                target_index_source_offset,
                indices,
                raw_indices,
                index_source_offsets,
                flag,
                mode,
            } => Self::Linked {
                object_index: located_index(
                    object_index,
                    &raw_object_index,
                    object_index_source_offset,
                    "object_index/raw_object_index",
                )?,
                discriminator,
                target_index: located_index(
                    target_index,
                    &raw_target_index,
                    target_index_source_offset,
                    "target_index/raw_target_index",
                )?,
                indices: located_indices(indices, raw_indices, index_source_offsets)?,
                flag,
                mode,
            },
            RmDisplayColorAssignmentEncodingWire::Target {
                target_index,
                raw_target_index,
                target_index_source_offset,
                indices,
                raw_indices,
                index_source_offsets,
                mode,
            } => Self::Target {
                target_index: located_index(
                    target_index,
                    &raw_target_index,
                    target_index_source_offset,
                    "target_index/raw_target_index",
                )?,
                indices: located_indices(indices, raw_indices, index_source_offsets)?,
                mode,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_wire<T: serde::de::DeserializeOwned + Serialize + std::fmt::Debug>(
        json: &str,
        field: &str,
        invalid: serde_json::Value,
    ) {
        let row: T = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&row).unwrap(), json);
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = invalid;
        let error = serde_json::from_value::<T>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }

    #[test]
    fn column_rows_keep_wire_order_and_reject_mismatched_tokens() {
        check_wire::<DataBlockIndexRow>(
            r#"{"id":"row","section_ordinal":0,"ordinal":0,"first_index":1,"raw_first_index":[1],"flag":3,"indices":[2,3,4,5],"raw_indices":[[2],[3],[4],[5]],"data_blocks":["a","b","c","d"],"source_entry":"entry","opening_data_block":"opening","opening_block_offset":0,"source_offset":10,"first_index_source_offset":13,"index_source_offsets":[17,18,19,20]}"#,
            "raw_first_index",
            serde_json::json!([255]),
        );
        check_wire::<DataBlockLinkedIndexRow>(
            r#"{"id":"row","section_ordinal":0,"ordinal":0,"first_index":1,"raw_first_index":[1],"discriminator":22,"target_index":2,"raw_target_index":[2],"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"data_blocks":["a","b","c","d"],"flag":3,"mode":4,"source_entry":"entry","opening_data_block":"opening","opening_block_offset":0,"source_offset":10,"first_index_source_offset":12,"target_index_source_offset":16,"index_source_offsets":[21,22,23]}"#,
            "raw_target_index",
            serde_json::json!([3]),
        );
        check_wire::<DataBlockTargetIndexRow>(
            r#"{"id":"row","section_ordinal":0,"ordinal":0,"target_index":2,"raw_target_index":[2],"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"data_blocks":["a","b","c","d"],"mode":7,"source_entry":"entry","opening_data_block":"opening","opening_block_offset":0,"source_offset":10,"target_index_source_offset":15,"index_source_offsets":[20,21,22]}"#,
            "raw_indices",
            serde_json::json!([[3], [4], [6]]),
        );
    }
    #[test]
    fn display_color_encodings_keep_wire_order_and_reject_mismatched_tokens() {
        check_wire::<RmDisplayColorAssignmentEncoding>(
            r#"{"kind":"linked","object_index":1,"raw_object_index":[128,1],"object_index_source_offset":10,"discriminator":22,"target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"flag":3,"mode":4}"#,
            "raw_object_index",
            serde_json::json!([2]),
        );
        check_wire::<RmDisplayColorAssignmentEncoding>(
            r#"{"kind":"target","target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"mode":7}"#,
            "raw_indices",
            serde_json::json!([[3], [4], [255]]),
        );
    }

    #[test]
    fn creation_display_relations_keep_all_three_wire_forms() {
        let encodings = [
            r#"{"kind":"index","flag":3,"indices":[2,3,4,5],"raw_indices":[[2],[3],[4],[5]],"index_source_offsets":[20,21,22,23]}"#,
            r#"{"kind":"linked","discriminator":22,"target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"flag":3,"mode":4}"#,
            r#"{"kind":"target","target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"mode":7}"#,
        ];
        for (ordinal, encoding) in encodings.into_iter().enumerate() {
            let first = if ordinal < 2 {
                r#","first_index":1,"raw_first_index":[128,1],"first_index_source_offset":10"#
            } else {
                ""
            };
            let json = format!(
                r#"{{"id":"relation","ordinal":0{first},"class_name":"class","class_definition":"definition","encoding":{encoding},"source_entry":"entry","source_offset":5}}"#
            );
            let relation: RmCreationDisplayDataRelation = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&relation).unwrap(), json);
            let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
            wire["encoding"]["raw_indices"][0] = serde_json::json!([255]);
            let error = serde_json::from_value::<RmCreationDisplayDataRelation>(wire).unwrap_err();
            assert!(error.to_string().contains("raw_indices"), "{error}");
        }
    }
}
