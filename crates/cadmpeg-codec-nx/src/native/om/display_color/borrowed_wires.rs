// SPDX-License-Identifier: Apache-2.0
//! Borrowed color-assignment row encodings and display token.

use super::{RmDisplayColorAssignment, RmDisplayColorAssignmentEncoding};
use crate::om::compact::RawCompactIndex;
use serde::ser::SerializeMap;
use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EncodingView {
    Linked {
        object_index: u32,
        raw_object_index: RawCompactIndex,
        object_index_source_offset: u64,
        discriminator: crate::om::discriminators::LinkedIndexDiscriminator,
        target_index: u32,
        raw_target_index: RawCompactIndex,
        target_index_source_offset: u64,
        indices: [u32; 3],
        raw_indices: [RawCompactIndex; 3],
        index_source_offsets: [u64; 3],
        flag: crate::om::discriminators::LinkedIndexFlag,
        mode: crate::om::discriminators::IndexRowMode,
    },
    Target {
        target_index: u32,
        raw_target_index: RawCompactIndex,
        target_index_source_offset: u64,
        indices: [u32; 3],
        raw_indices: [RawCompactIndex; 3],
        index_source_offsets: [u64; 3],
        mode: crate::om::discriminators::IndexRowMode,
    },
}

impl From<&RmDisplayColorAssignmentEncoding> for EncodingView {
    fn from(value: &RmDisplayColorAssignmentEncoding) -> Self {
        match value {
            RmDisplayColorAssignmentEncoding::Linked(row) => Self::Linked {
                object_index: row.first_index().atom.value(),
                raw_object_index: RawCompactIndex(row.first_index().atom),
                object_index_source_offset: row.first_index().offset,
                discriminator: row.discriminator(),
                target_index: row.target_index().atom.value(),
                raw_target_index: RawCompactIndex(row.target_index().atom),
                target_index_source_offset: row.target_index().offset,
                indices: row.indices().map(|index| index.atom.value()),
                raw_indices: row.indices().map(|index| RawCompactIndex(index.atom)),
                index_source_offsets: row.indices().map(|index| index.offset),
                flag: row.flag(),
                mode: row.mode(),
            },
            RmDisplayColorAssignmentEncoding::Target(row) => Self::Target {
                target_index: row.target_index().atom.value(),
                raw_target_index: RawCompactIndex(row.target_index().atom),
                target_index_source_offset: row.target_index().offset,
                indices: row.indices().map(|index| index.atom.value()),
                raw_indices: row.indices().map(|index| RawCompactIndex(index.atom)),
                index_source_offsets: row.indices().map(|index| index.offset),
                mode: row.mode(),
            },
        }
    }
}

impl Serialize for RmDisplayColorAssignmentEncoding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EncodingView::from(self).serialize(serializer)
    }
}

impl Serialize for RmDisplayColorAssignment {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        let (raw, width) = self.frame.color_index.display_token();
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("encoding", &self.frame.encoding)?;
        if let Some(target) = &self.target_object_id {
            wire.serialize_entry("target_object_id", target)?;
        }
        wire.serialize_entry("color_index", &self.frame.color_index.value())?;
        wire.serialize_entry("color_definition", &self.color_definition)?;
        wire.serialize_entry("raw_color_index", &raw[..width])?;
        wire.serialize_entry("source_entry", &self.source_entry)?;
        wire.serialize_entry("source_offset", &self.frame.offset())?;
        wire.serialize_entry("row_source_offset", &self.frame.encoding.offset())?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_test_support::native_serialization::assert_native_limit;

    #[test]
    fn color_encoding_borrowed_bytes_match_owned_wire() {
        for json in [
            r#"{"kind":"linked","object_index":1,"raw_object_index":[128,1],"object_index_source_offset":10,"discriminator":22,"target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"flag":3,"mode":4}"#,
            r#"{"kind":"target","target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"mode":7}"#,
        ] {
            let value: RmDisplayColorAssignmentEncoding = serde_json::from_str(json).unwrap();
            assert_eq!(
                serde_json::to_vec(&value).unwrap(),
                serde_json::to_vec(&super::super::wire::EncodingWire::from(value.clone())).unwrap()
            );
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
        }
    }

    #[test]
    fn color_assignment_borrowed_bytes_and_limit() {
        let json = r#"{"id":"nx:rm-display-color-assignments:assignment#0","ordinal":0,"encoding":{"kind":"target","target_index":2,"raw_target_index":[2],"target_index_source_offset":15,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[20,21,22],"mode":7},"color_index":128,"color_definition":"definition","raw_color_index":[128,128],"source_entry":"entry","source_offset":8,"row_source_offset":10}"#;
        let value: RmDisplayColorAssignment = serde_json::from_str(json).unwrap();
        let borrowed = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            borrowed,
            serde_json::to_vec(&super::super::wire::RmDisplayColorAssignmentWire::from(
                value.clone()
            ))
            .unwrap()
        );
        assert_eq!(borrowed, json.as_bytes());
        assert_native_limit(
            &value,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}
