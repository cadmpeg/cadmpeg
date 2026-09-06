// SPDX-License-Identifier: Apache-2.0
//! Existing row-wire checks pending relocation to their new production owners.
use serde::Serialize;
use crate::native::om::column_row::{DataBlockIndexRow, DataBlockLinkedIndexRow, DataBlockTargetIndexRow};
use crate::native::om::creation_display::RmCreationDisplayDataRelation;
use crate::native::om::display_color::RmDisplayColorAssignmentEncoding;

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
            let json = format!(r#"{{"id":"relation","ordinal":0{first},"class_name":"UGS::RM_creation_display_data","class_definition":"definition","encoding":{encoding},"source_entry":"entry","source_offset":5}}"#);
            let relation: RmCreationDisplayDataRelation = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&relation).unwrap(), json);
            let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
            wire["encoding"]["raw_indices"][0] = serde_json::json!([255]);
            let error = serde_json::from_value::<RmCreationDisplayDataRelation>(wire).unwrap_err();
            assert!(error.to_string().contains("raw_indices"), "{error}");
        }
    }

}
