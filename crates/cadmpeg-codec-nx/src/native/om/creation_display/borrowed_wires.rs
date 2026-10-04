// SPDX-License-Identifier: Apache-2.0
//! Borrowed serialization of creation-display row forms.

use super::{RmCreationDisplayDataEncoding, RmCreationDisplayDataRelation, CLASS_NAME};
use crate::om::compact::RawCompactIndex;
use serde::ser::SerializeMap;
use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EncodingView {
    Index {
        flag: u8,
        indices: [u32; 4],
        raw_indices: [RawCompactIndex; 4],
        index_source_offsets: [u64; 4],
    },
    Linked {
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

impl From<&RmCreationDisplayDataEncoding> for EncodingView {
    fn from(value: &RmCreationDisplayDataEncoding) -> Self {
        match value {
            RmCreationDisplayDataEncoding::Index(row) => Self::Index {
                flag: u8::from(row.flag()),
                indices: row.indices().map(|index| index.atom.value()),
                raw_indices: row.indices().map(|index| RawCompactIndex(index.atom)),
                index_source_offsets: row.indices().map(|index| index.offset),
            },
            RmCreationDisplayDataEncoding::Linked { row, .. } => Self::Linked {
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
            RmCreationDisplayDataEncoding::Target { row, .. } => Self::Target {
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

impl Serialize for RmCreationDisplayDataRelation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        let first = match &self.encoding {
            RmCreationDisplayDataEncoding::Index(row) => Some(row.first_index()),
            RmCreationDisplayDataEncoding::Linked { row, .. } => Some(row.first_index()),
            RmCreationDisplayDataEncoding::Target { .. } => None,
        };
        if let Some(first) = first {
            wire.serialize_entry("first_index", &first.atom.value())?;
            wire.serialize_entry("raw_first_index", &RawCompactIndex(first.atom))?;
            wire.serialize_entry("first_index_source_offset", &first.offset)?;
        }
        wire.serialize_entry("class_name", CLASS_NAME)?;
        wire.serialize_entry("class_definition", &self.class_definition)?;
        wire.serialize_entry("encoding", &EncodingView::from(&self.encoding))?;
        let target = match &self.encoding {
            RmCreationDisplayDataEncoding::Index(_) => None,
            RmCreationDisplayDataEncoding::Linked {
                target_object_id, ..
            }
            | RmCreationDisplayDataEncoding::Target {
                target_object_id, ..
            } => target_object_id.as_ref(),
        };
        if let Some(target) = target {
            wire.serialize_entry("target_object_id", target)?;
        }
        wire.serialize_entry("source_entry", &self.source_entry)?;
        wire.serialize_entry("source_offset", &self.encoding.offset())?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_test_support::native_serialization::assert_native_limit;

    #[test]
    fn creation_display_borrowed_bytes_and_limit_for_each_form() {
        let encodings = [
            r#"{"kind":"index","flag":3,"indices":[2,3,4,5],"raw_indices":[[2],[3],[4],[5]],"index_source_offsets":[13,14,15,16]}"#,
            r#"{"kind":"linked","discriminator":22,"target_index":2,"raw_target_index":[2],"target_index_source_offset":12,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[17,18,19],"flag":3,"mode":4}"#,
            r#"{"kind":"target","target_index":2,"raw_target_index":[2],"target_index_source_offset":10,"indices":[3,4,5],"raw_indices":[[3],[4],[5]],"index_source_offsets":[15,16,17],"mode":7}"#,
        ];
        for (ordinal, encoding) in encodings.into_iter().enumerate() {
            let first = match ordinal {
                0 => r#","first_index":1,"raw_first_index":[128,1],"first_index_source_offset":8"#,
                1 => r#","first_index":1,"raw_first_index":[128,1],"first_index_source_offset":7"#,
                _ => "",
            };
            let json = format!(
                r#"{{"id":"nx:rm-creation-display:relation#0","ordinal":0{first},"class_name":"UGS::RM_creation_display_data","class_definition":"definition","encoding":{encoding},"source_entry":"entry","source_offset":5}}"#
            );
            let record: RmCreationDisplayDataRelation = serde_json::from_str(&json).unwrap();
            let borrowed = serde_json::to_vec(&record).unwrap();
            let owned = serde_json::to_vec(
                &super::super::wire::RmCreationDisplayDataRelationWire::from(record.clone()),
            )
            .unwrap();
            assert_eq!(borrowed, owned);
            assert_eq!(borrowed, json.as_bytes());
            assert_native_limit(
                &record,
                serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            );
        }
    }
}
