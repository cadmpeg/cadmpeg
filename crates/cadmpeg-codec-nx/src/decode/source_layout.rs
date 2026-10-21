// SPDX-License-Identifier: Apache-2.0
//! Source encodings and layout belong to fidelity, not typed native content.

use cadmpeg_core::CodecError;
use cadmpeg_ir::{document::CadIr, native::NativeRecord, SourceFidelity};
use serde_json::{json, Value};

// NativeBytes fields of the NX scan records. A matching field is extracted
// only when its value is a hexadecimal byte encoding; typed objects and
// mathematical arrays continue through the normal traversal.
const BYTE_FIELDS: &[&str] = &[
    "body",
    "flags",
    "index_lane_raw_indices",
    "layout_prefix",
    "markers",
    "prefix",
    "payload",
    "raw",
    "raw_anchor_index",
    "raw_background_components",
    "raw_body_image_object_index",
    "raw_body_object_index",
    "raw_byte_len",
    "raw_color_index",
    "raw_components",
    "raw_continuation_index",
    "raw_descriptor_indices",
    "raw_first",
    "raw_first_index",
    "raw_first_indices",
    "raw_first_object_indices",
    "raw_group_node",
    "raw_indices",
    "raw_local_ordinal",
    "raw_member_index",
    "raw_member_indices",
    "raw_object_id",
    "raw_object_index",
    "raw_object_indices",
    "raw_operand_object_index",
    "raw_ordinal",
    "raw_position",
    "raw_scalar",
    "raw_scalars",
    "raw_second",
    "raw_second_indices",
    "raw_second_object_indices",
    "raw_selectors",
    "raw_slot_indices",
    "raw_status_code",
    "raw_target_index",
    "raw_target_object_index",
    "raw_terminal_object_index",
    "raw_tool_object_indices",
    "raw_trailing_indices",
    "raw_trailing_object_indices",
    "raw_type_code",
    "raw_type_indices",
    "raw_value",
    "raw_values",
    "registry_suffix",
    "state_lane",
    "suffix",
    "table_trailing_bytes",
    "tail",
];

pub(crate) fn move_layout(ir: &mut CadIr, fidelity: &mut SourceFidelity) -> Result<(), CodecError> {
    for (arena, records) in ir.native.namespace_mut("nx").arenas_mut() {
        for record in records {
            let identity =
                cadmpeg_ir::ids::Identity::try_from(record.id()).map_err(CodecError::malformed)?;
            let mut value = Value::Object(record.fields()?);
            extract(
                &mut value,
                &format!("native.nx.{arena}.{}", record.id()),
                fidelity,
            )?;
            let Value::Object(fields) = value else {
                return Err(CodecError::malformed("NX native record is not an object"));
            };
            *record = NativeRecord::new(identity, fields)?;
        }
    }
    if let Some(source) = &mut ir.source {
        let keys = source
            .attributes
            .keys()
            .filter(|key| is_layout_key(key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(value) = source.attributes.remove(&key) {
                fidelity.metadata.insert(format!("source.{key}"), value);
            }
        }
    }
    Ok(())
}

fn extract(value: &mut Value, path: &str, fidelity: &mut SourceFidelity) -> Result<(), CodecError> {
    match value {
        Value::Object(object) => {
            let keys = object.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let child_path = format!("{path}.{key}");
                if is_layout_key(&key) {
                    if let Some(value) = object.remove(&key) {
                        fidelity.metadata.insert(child_path, value.to_string());
                    }
                    continue;
                }
                let Some(child) = object.get_mut(&key) else {
                    continue;
                };
                if BYTE_FIELDS.contains(&key.as_str()) {
                    if extract_bytes(child, &child_path, fidelity)? {
                        continue;
                    }
                }
                extract(child, &child_path, fidelity)?;
            }
        }
        Value::Array(values) => {
            for (ordinal, value) in values.iter_mut().enumerate() {
                extract(value, &format!("{path}[{ordinal}]"), fidelity)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn extract_bytes(
    value: &mut Value,
    path: &str,
    fidelity: &mut SourceFidelity,
) -> Result<bool, CodecError> {
    if let Some(bytes) = value.as_str().and_then(decode_hex) {
        let hash = cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes);
        let record_id = format!(
            "nx:source:evidence#{}",
            cadmpeg_ir::hash::sha256_hex(path.as_bytes())
        );
        fidelity.insert_retained_record(
            cadmpeg_ir::ids::UnknownId::mint(record_id.clone()).map_err(CodecError::malformed)?,
            cadmpeg_ir::source_fidelity::RetainedSourceRecord::from_bytes(
                path.to_owned(),
                0,
                cadmpeg_ir::source_fidelity::RetainedBytes::Inline {
                    data: bytes.clone(),
                },
            )?,
        )?;
        *value = json!({"record_id":record_id,"byte_len":bytes.len(),"sha256":hash});
        return Ok(true);
    }
    if let Value::Array(values) = value {
        let mut extracted = false;
        for (ordinal, child) in values.iter_mut().enumerate() {
            extracted |= extract_bytes(child, &format!("{path}[{ordinal}]"), fidelity)?;
        }
        return Ok(extracted);
    }
    Ok(false)
}

fn is_layout_key(key: &str) -> bool {
    key == "offset"
        || key.ends_with("_offset")
        || key.ends_with("_offsets")
        || key == "pos"
        || key.ends_with("_pos")
        || key == "byte_start"
        || key == "byte_end"
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appendix_b_keep_fields_preserve_exact_native_serialization() {
        let mut value = json!({"attributes":"00000007", "component_quantization_bits":"080a0c", "legal_owner_flags":"000100"});
        let mut fidelity = SourceFidelity::default();
        extract(&mut value, "native.nx.record", &mut fidelity).unwrap();
        assert_eq!(value["attributes"], json!("00000007"));
        assert_eq!(value["component_quantization_bits"], json!("080a0c"));
        assert_eq!(value["legal_owner_flags"], json!("000100"));
        assert!(fidelity.retained_records().is_empty());
    }

    #[test]
    fn appendix_b_keep_byte_fields_are_not_source_payloads() {
        // Each of these NX field names has only keep classifications in the
        // immutable Appendix B inventory. Their typed owners admit the form;
        // the layout pass must preserve even a short or empty byte value.
        for field in [
            "discriminator",
            "first_reference_prefix",
            "lsg_segment_id",
            "marker",
            "object_type_id",
            "opener",
            "ordinals",
            "raw_count",
            "row_schema_indices",
            "schema_fingerprint",
            "second_reference_prefix",
            "segment_id",
            "selector",
            "split_tracking_data",
            "state",
            "trailer",
            "trailing_value",
        ] {
            for bytes in ["", "0001ff"] {
                let mut value = json!({"owner": {field: bytes}});
                let original = value.clone();
                let mut fidelity = SourceFidelity::default();
                extract(&mut value, "native.nx.record", &mut fidelity).unwrap();
                assert_eq!(value, original, "{field}");
                assert!(fidelity.retained_records().is_empty(), "{field}");
                assert!(fidelity.metadata.is_empty(), "{field}");
            }
        }
    }

    #[test]
    fn source_bytes_and_positions_leave_typed_records() {
        let mut value = json!({"payload":"0001ff", "payload_offset":17, "radius":3.0, "program":{"kind":"sum"}});
        let mut fidelity = SourceFidelity::default();
        extract(&mut value, "native.nx.record", &mut fidelity).unwrap();
        assert_eq!(value["payload"]["byte_len"], 3);
        assert!(value.get("payload_offset").is_none());
        assert_eq!(value["radius"], 3.0);
        assert_eq!(value["program"]["kind"], "sum");
        assert_eq!(fidelity.metadata["native.nx.record.payload_offset"], "17");
        assert!(fidelity.unrepresented().is_empty());
    }
}
