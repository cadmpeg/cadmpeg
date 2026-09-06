// SPDX-License-Identifier: Apache-2.0
//! Existing encoding and assignment JSON with derived row positions.

use super::*;
use crate::om::compact::CompactIndexAtom;

fn atom(value: u32, raw: &[u8], field: &str) -> Result<CompactIndexAtom, String> {
    CompactIndexAtom::from_wire(value, raw).map_err(|error| format!("{field}: {error}"))
}

fn row_indices(values: [u32; 3], raw: [Vec<u8>; 3]) -> [Result<CompactIndexAtom, String>; 3] {
    std::array::from_fn(|i| atom(values[i], &raw[i], "indices/raw_indices"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum EncodingWire {
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


impl From<RmDisplayColorAssignmentEncoding> for EncodingWire {
    fn from(value: RmDisplayColorAssignmentEncoding) -> Self {
        match value {
            RmDisplayColorAssignmentEncoding::Linked(row) => Self::Linked {
                object_index: row.first_index().atom.value(), raw_object_index: row.first_index().atom.raw().to_vec(), object_index_source_offset: row.first_index().offset,
                discriminator: row.discriminator(),
                target_index: row.target_index().atom.value(), raw_target_index: row.target_index().atom.raw().to_vec(), target_index_source_offset: row.target_index().offset,
                indices: row.indices().map(|index| index.atom.value()), raw_indices: row.indices().map(|index| index.atom.raw().to_vec()), index_source_offsets: row.indices().map(|index| index.offset),
                flag: row.flag(),
                mode: row.mode(),
            },
            RmDisplayColorAssignmentEncoding::Target(row) => Self::Target {
                target_index: row.target_index().atom.value(), raw_target_index: row.target_index().atom.raw().to_vec(), target_index_source_offset: row.target_index().offset,
                indices: row.indices().map(|index| index.atom.value()), raw_indices: row.indices().map(|index| index.atom.raw().to_vec()), index_source_offsets: row.indices().map(|index| index.offset),
                mode: row.mode(),
            },
        }
    }
}

impl TryFrom<EncodingWire> for RmDisplayColorAssignmentEncoding {
    type Error = String;
    fn try_from(wire: EncodingWire) -> Result<Self, Self::Error> {
        match wire {
            EncodingWire::Linked { target_index, raw_target_index, target_index_source_offset, indices, raw_indices, index_source_offsets, mode, object_index, raw_object_index, object_index_source_offset, discriminator, flag } => {
                let [a, b, c] = row_indices(indices, raw_indices);
                let indices = [a?.into(), b?.into(), c?.into()];
                let target = atom(target_index, &raw_target_index, "target_index/raw_target_index")?.into();
                let first = atom(object_index, &raw_object_index, "object_index/raw_object_index")?;
                let offset = object_index_source_offset.checked_sub(2).ok_or("object_index_source_offset precedes row prefix")?;
                let row = LinkedRow::<(), u64>::new(first, discriminator, target, indices, flag, mode, offset).ok_or("object_index_source_offset: row extent overflows")?;
                if row.target_index().offset != target_index_source_offset { return Err("target_index_source_offset differs from row layout".into()); }
                if row.indices().map(|index| index.offset) != index_source_offsets { return Err("index_source_offsets differ from row layout".into()); }
                Ok(Self::Linked(row))
            }
            EncodingWire::Target { target_index, raw_target_index, target_index_source_offset, indices, raw_indices, index_source_offsets, mode } => {
                let [a, b, c] = row_indices(indices, raw_indices);
                let indices = [a?.into(), b?.into(), c?.into()];
                let target = atom(target_index, &raw_target_index, "target_index/raw_target_index")?.into();
                let offset = target_index_source_offset.checked_sub(5).ok_or("target_index_source_offset precedes row prefix")?;
                let row = TargetRow::<(), u64>::new(target, indices, mode, offset).ok_or("target_index_source_offset: row extent overflows")?;
                if row.indices().map(|index| index.offset) != index_source_offsets { return Err("index_source_offsets differ from row layout".into()); }
                Ok(Self::Target(row))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RmDisplayColorAssignmentWire {
    /// Globally unique assignment identity.
    pub id: String,
    /// Zero-based source order.
    pub ordinal: u32,
    /// Complete self-framed row carrying the color token.
    pub encoding: RmDisplayColorAssignmentEncoding,
    /// Member addressed by the row target index when it resolves in the
    /// `RMFastLoad` object-ID table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_object_id: Option<String>,
    /// One-based part palette index.
    pub color_index: u16,
    /// Target in `part_color_definitions`.
    pub color_definition: String,
    /// Exact color-index token.
    pub raw_color_index: Vec<u8>,
    /// Owning directory entry.
    pub source_entry: String,
    /// Absolute color-token offset.
    pub source_offset: u64,
    /// Absolute row-opener offset.
    pub row_source_offset: u64,
}

impl TryFrom<RmDisplayColorAssignmentWire> for RmDisplayColorAssignment {
    type Error = String;
    fn try_from(wire: RmDisplayColorAssignmentWire) -> Result<Self, Self::Error> {
        let color_index = PaletteIndex::new(wire.color_index).ok_or("color_index: must be in 1..=216")?;
        if color_index.display_raw() != wire.raw_color_index { return Err("raw_color_index differs from color_index display token".into()); }
        let frame = DisplayColorFrame::new(wire.encoding, color_index).ok_or("source_offset: color token precedes origin")?;
        if frame.offset() != wire.source_offset { return Err("source_offset differs from the color-token position".into()); }
        if frame.encoding.offset() != wire.row_source_offset { return Err("row_source_offset differs from the encoding origin".into()); }
        Ok(Self { id: wire.id, ordinal: wire.ordinal, frame, target_object_id: wire.target_object_id, color_definition: wire.color_definition, source_entry: wire.source_entry })
    }
}

impl From<RmDisplayColorAssignment> for RmDisplayColorAssignmentWire {
    fn from(value: RmDisplayColorAssignment) -> Self {
        let source_offset = value.frame.offset();
        let row_source_offset = value.frame.encoding.offset();
        Self {
            id: value.id, ordinal: value.ordinal,
            encoding: value.frame.encoding,
            target_object_id: value.target_object_id,
            color_index: value.frame.color_index.value(),
            color_definition: value.color_definition,
            raw_color_index: value.frame.color_index.display_raw(),
            source_entry: value.source_entry,
            source_offset, row_source_offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_assignment_derives_both_offsets_from_its_row_and_token() {
        let json = r#"{"id":"color","ordinal":0,"encoding":{"kind":"target","target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"mode":7},"color_index":128,"color_definition":"definition","raw_color_index":[128,128],"source_entry":"entry","source_offset":8,"row_source_offset":10}"#;
        let assignment: RmDisplayColorAssignment = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&assignment).unwrap(), json);
        for field in ["source_offset", "row_source_offset"] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[field] = 99.into();
            assert!(serde_json::from_value::<RmDisplayColorAssignment>(wire).unwrap_err().to_string().contains(field));
        }
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["color_index"] = 127.into();
        wire["raw_color_index"] = serde_json::json!([127]);
        wire["source_offset"] = 9.into();
        let assignment: RmDisplayColorAssignment = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(assignment).unwrap(), wire);
    }
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
    fn display_color_encodings_keep_wire_order_and_reject_mismatched_tokens() {
        check_wire::<RmDisplayColorAssignmentEncoding>(
            r#"{"kind":"linked","object_index":1,"raw_object_index":[128,1],"object_index_source_offset":10,"discriminator":22,"target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"flag":3,"mode":4}"#,
            "raw_object_index", serde_json::json!([2]),
        );
        check_wire::<RmDisplayColorAssignmentEncoding>(
            r#"{"kind":"target","target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"mode":7}"#,
            "raw_indices", serde_json::json!([[3], [4], [255]]),
        );
    }
}
