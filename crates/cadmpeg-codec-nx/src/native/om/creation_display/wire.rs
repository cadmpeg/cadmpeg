// SPDX-License-Identifier: Apache-2.0
//! Flat native fields projected from the complete creation-display row.

use super::{
    Deserialize, IndexRow, LinkedRow, RmCreationDisplayDataEncoding, RmCreationDisplayDataRelation,
    Serialize, TargetRow, CLASS_NAME,
};
use crate::om::compact::CompactIndexAtom;

fn atom(value: u32, raw: &[u8], field: &str) -> Result<CompactIndexAtom, String> {
    CompactIndexAtom::from_wire(value, raw).map_err(|error| format!("{field}: {error}"))
}

// This conversion consumes the input carrier at the typed construction boundary.
#[allow(clippy::needless_pass_by_value)]
fn row_indices<const N: usize>(
    values: [u32; N],
    raw: [Vec<u8>; N],
) -> [Result<CompactIndexAtom, String>; N] {
    std::array::from_fn(|i| atom(values[i], &raw[i], "indices/raw_indices"))
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RmCreationDisplayDataEncodingWire {
    Index {
        flag: u8,
        indices: [u32; 4],
        raw_indices: [Vec<u8>; 4],
        index_source_offsets: [u64; 4],
    },
    Linked {
        discriminator: crate::om::discriminators::LinkedIndexDiscriminator,
        target_index: u32,
        raw_target_index: Vec<u8>,
        target_index_source_offset: u64,
        indices: [u32; 3],
        raw_indices: [Vec<u8>; 3],
        index_source_offsets: [u64; 3],
        flag: crate::om::discriminators::LinkedIndexFlag,
        mode: crate::om::discriminators::IndexRowMode,
    },
    Target {
        target_index: u32,
        raw_target_index: Vec<u8>,
        target_index_source_offset: u64,
        indices: [u32; 3],
        raw_indices: [Vec<u8>; 3],
        index_source_offsets: [u64; 3],
        mode: crate::om::discriminators::IndexRowMode,
    },
}

#[derive(Serialize, Deserialize)]
pub(super) struct RmCreationDisplayDataRelationWire {
    id: String,
    ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw_first_index: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_index_source_offset: Option<u64>,
    class_name: String,
    class_definition: String,
    encoding: RmCreationDisplayDataEncodingWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target_object_id: Option<String>,
    source_entry: String,
    source_offset: u64,
}

impl From<RmCreationDisplayDataRelation> for RmCreationDisplayDataRelationWire {
    fn from(value: RmCreationDisplayDataRelation) -> Self {
        let source_offset = value.encoding.offset();
        let (first_index, raw_first_index, first_index_source_offset, encoding, target_object_id) =
            match value.encoding {
                RmCreationDisplayDataEncoding::Index(row) => (
                    Some(row.first_index().atom.value()),
                    Some(row.first_index().atom.raw().to_vec()),
                    Some(row.first_index().offset),
                    RmCreationDisplayDataEncodingWire::Index {
                        indices: row.indices().map(|index| index.atom.value()),
                        raw_indices: row.indices().map(|index| index.atom.raw().to_vec()),
                        index_source_offsets: row.indices().map(|index| index.offset),
                        flag: u8::from(row.flag()),
                    },
                    None,
                ),
                RmCreationDisplayDataEncoding::Linked {
                    row,
                    target_object_id,
                } => (
                    Some(row.first_index().atom.value()),
                    Some(row.first_index().atom.raw().to_vec()),
                    Some(row.first_index().offset),
                    RmCreationDisplayDataEncodingWire::Linked {
                        discriminator: row.discriminator(),
                        target_index: row.target_index().atom.value(),
                        raw_target_index: row.target_index().atom.raw().to_vec(),
                        target_index_source_offset: row.target_index().offset,
                        indices: row.indices().map(|index| index.atom.value()),
                        raw_indices: row.indices().map(|index| index.atom.raw().to_vec()),
                        index_source_offsets: row.indices().map(|index| index.offset),
                        flag: row.flag(),
                        mode: row.mode(),
                    },
                    target_object_id,
                ),
                RmCreationDisplayDataEncoding::Target {
                    row,
                    target_object_id,
                } => (
                    None,
                    None,
                    None,
                    RmCreationDisplayDataEncodingWire::Target {
                        target_index: row.target_index().atom.value(),
                        raw_target_index: row.target_index().atom.raw().to_vec(),
                        target_index_source_offset: row.target_index().offset,
                        indices: row.indices().map(|index| index.atom.value()),
                        raw_indices: row.indices().map(|index| index.atom.raw().to_vec()),
                        index_source_offsets: row.indices().map(|index| index.offset),
                        mode: row.mode(),
                    },
                    target_object_id,
                ),
            };
        Self {
            id: value.id,
            ordinal: value.ordinal,
            first_index,
            raw_first_index,
            first_index_source_offset,
            class_name: CLASS_NAME.to_owned(),
            class_definition: value.class_definition,
            encoding,
            target_object_id,
            source_entry: value.source_entry,
            source_offset,
        }
    }
}

impl TryFrom<RmCreationDisplayDataRelationWire> for RmCreationDisplayDataRelation {
    type Error = String;
    fn try_from(wire: RmCreationDisplayDataRelationWire) -> Result<Self, Self::Error> {
        if wire.class_name != CLASS_NAME {
            return Err("class_name must be UGS::RM_creation_display_data".into());
        }
        let encoding = match (wire.encoding, wire.first_index, wire.raw_first_index, wire.first_index_source_offset, wire.target_object_id) {
            (RmCreationDisplayDataEncodingWire::Index { indices, raw_indices, index_source_offsets, flag }, Some(first_index), Some(raw_first_index), Some(first_index_source_offset), None) => {
                let [a, b, c, d] = row_indices(indices, raw_indices);
                let indices = [a?.into(), b?.into(), c?.into(), d?.into()];
                let first = atom(first_index, &raw_first_index, "first_index/raw_first_index")?;
                let flag = crate::om::discriminators::LinkedIndexFlag::try_from(flag).map_err(|_| "flag: must be 3 or 7")?;
                let row = IndexRow::<(), u64>::new(first, flag, indices, wire.source_offset).ok_or("source_offset: row extent overflows")?;
                if row.first_index().offset != first_index_source_offset { return Err("first_index_source_offset differs from row layout".into()); }
                if row.indices().map(|index| index.offset) != index_source_offsets { return Err("index_source_offsets differ from row layout".into()); }
                RmCreationDisplayDataEncoding::Index(row)
            }
            (RmCreationDisplayDataEncodingWire::Linked { indices, raw_indices, index_source_offsets, flag, target_index, raw_target_index, target_index_source_offset, mode, discriminator }, Some(first_index), Some(raw_first_index), Some(first_index_source_offset), target_object_id) => {
                let [a, b, c] = row_indices(indices, raw_indices);
                let indices = [a?.into(), b?.into(), c?.into()];
                let first = atom(first_index, &raw_first_index, "first_index/raw_first_index")?;
                let target = atom(target_index, &raw_target_index, "target_index/raw_target_index")?.into();
                let row = LinkedRow::<(), u64>::new(first, discriminator, target, indices, flag, mode, wire.source_offset).ok_or("source_offset: row extent overflows")?;
                if row.first_index().offset != first_index_source_offset { return Err("first_index_source_offset differs from row layout".into()); }
                if row.target_index().offset != target_index_source_offset { return Err("target_index_source_offset differs from row layout".into()); }
                if row.indices().map(|index| index.offset) != index_source_offsets { return Err("index_source_offsets differ from row layout".into()); }
                RmCreationDisplayDataEncoding::Linked { row, target_object_id }
            }
            (RmCreationDisplayDataEncodingWire::Target { indices, raw_indices, index_source_offsets, target_index, raw_target_index, target_index_source_offset, mode }, None, None, None, target_object_id) => {
                let [a, b, c] = row_indices(indices, raw_indices);
                let indices = [a?.into(), b?.into(), c?.into()];
                let target = atom(target_index, &raw_target_index, "target_index/raw_target_index")?.into();
                let row = TargetRow::<(), u64>::new(target, indices, mode, wire.source_offset).ok_or("source_offset: row extent overflows")?;
                if row.target_index().offset != target_index_source_offset { return Err("target_index_source_offset differs from row layout".into()); }
                if row.indices().map(|index| index.offset) != index_source_offsets { return Err("index_source_offsets differ from row layout".into()); }
                RmCreationDisplayDataEncoding::Target { row, target_object_id }
            }
            _ => return Err("first_index/raw_first_index/first_index_source_offset/target_object_id: incompatible row family".into()),
        };
        Ok(Self {
            id: wire.id,
            ordinal: wire.ordinal,
            class_definition: wire.class_definition,
            encoding,
            source_entry: wire.source_entry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_display_rejects_fields_inconsistent_with_its_row() {
        let wire = serde_json::json!({
            "id": "creation", "ordinal": 0,
            "first_index": 1, "raw_first_index": [128, 1],
            "first_index_source_offset": 8,
            "class_name": "UGS::RM_creation_display_data", "class_definition": "class",
            "encoding": { "kind": "index", "flag": 3,
                "indices": [2, 3, 4, 5], "raw_indices": [[2], [3], [4], [5]],
                "index_source_offsets": [13, 14, 15, 16] },
            "source_entry": "entry", "source_offset": 5
        });
        let row: RmCreationDisplayDataRelation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(row).unwrap(), wire);
        for (field, value) in [
            ("class_name", serde_json::json!("other")),
            ("target_object_id", serde_json::json!("object")),
            ("first_index_source_offset", serde_json::json!(9)),
            ("source_offset", serde_json::json!(u64::MAX)),
        ] {
            let mut invalid = wire.clone();
            invalid[field] = value;
            assert!(
                serde_json::from_value::<RmCreationDisplayDataRelation>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains(field)
            );
        }
        for index in 0..4 {
            let mut invalid = wire.clone();
            invalid["encoding"]["index_source_offsets"][index] = 99.into();
            assert!(
                serde_json::from_value::<RmCreationDisplayDataRelation>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("index_source_offsets")
            );
        }
    }
    #[test]
    fn creation_display_relations_keep_all_three_wire_forms() {
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
                r#"{{"id":"relation","ordinal":0{first},"class_name":"UGS::RM_creation_display_data","class_definition":"definition","encoding":{encoding},"source_entry":"entry","source_offset":5}}"#
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
