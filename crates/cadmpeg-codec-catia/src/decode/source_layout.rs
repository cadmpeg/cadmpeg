// SPDX-License-Identifier: Apache-2.0
//! Source encodings and layout belong to fidelity, not typed native content.

use cadmpeg_core::CodecError;
use cadmpeg_ir::{document::CadIr, native::NativeRecord, SourceFidelity};
use serde_json::{json, Value};

// NativeBytes fields of the CATIA scan records. A matching field is extracted
// only when its value is a hexadecimal byte encoding; typed objects and
// mathematical arrays continue through the normal traversal.
const BYTE_FIELDS: &[&str] = &[
    "bytes",
    "controls",
    "data",
    "definition_prefix",
    "definition_suffix",
    "f1",
    "header",
    "inline_body",
    "payload",
    "prefix",
    "prefix_atom_widths",
    "program",
    "record_suffix",
    "senses",
    "storage_prefix",
    "tag",
    "tail",
    "terminal",
    "terminal_controls",
    "value_payload",
];

pub(super) fn move_layout(ir: &mut CadIr, fidelity: &mut SourceFidelity) -> Result<(), CodecError> {
    for (arena, records) in ir.native.namespace_mut("catia").arenas_mut() {
        for record in records {
            let identity =
                cadmpeg_ir::ids::Identity::try_from(record.id()).map_err(CodecError::malformed)?;
            let fields = record.fields()?;
            let mut value = Value::Object(fields);
            extract(
                &mut value,
                &format!("native.catia.{arena}.{}", record.id()),
                fidelity,
            )?;
            let Value::Object(fields) = value else {
                return Err(CodecError::malformed(
                    "CATIA native record is not an object",
                ));
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
            if let Some(Value::Object(scalar)) = object.get_mut("Binary64") {
                if let Some(bits) = scalar.get("bits").and_then(Value::as_u64) {
                    fidelity
                        .metadata
                        .insert(format!("{path}.Binary64.bits"), bits.to_string());
                    let number = f64::from_bits(bits);
                    scalar.remove("bits");
                    if let Some(value) = serde_json::Number::from_f64(number) {
                        scalar.insert("value".into(), Value::Number(value));
                    } else {
                        scalar.insert(
                            "non_finite".into(),
                            Value::String(
                                if number.is_nan() {
                                    "nan"
                                } else if number.is_sign_positive() {
                                    "positive_infinity"
                                } else {
                                    "negative_infinity"
                                }
                                .into(),
                            ),
                        );
                    }
                }
            }
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
                    if let Some(bytes) = child.as_str().and_then(decode_hex) {
                        let hash = cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes);
                        let record_id = format!(
                            "catia:source:evidence#{}",
                            cadmpeg_ir::hash::sha256_hex(child_path.as_bytes())
                        );
                        fidelity.insert_retained_record(
                            cadmpeg_ir::ids::UnknownId::mint(record_id.clone())
                                .map_err(CodecError::malformed)?,
                            cadmpeg_ir::source_fidelity::RetainedSourceRecord::from_bytes(
                                child_path,
                                0,
                                cadmpeg_ir::source_fidelity::RetainedBytes::Inline {
                                    data: bytes.clone(),
                                },
                            )?,
                        )?;
                        *child = json!({"record_id": record_id, "byte_len": bytes.len(), "sha256": hash});
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
    fn source_bytes_and_positions_leave_typed_records() {
        let mut value = json!({"payload":"0001ff", "payload_offset":17, "radius":3.0, "program":{"kind":"sum"}});
        let mut fidelity = SourceFidelity::default();
        extract(&mut value, "native.catia.record", &mut fidelity).unwrap();
        assert_eq!(value["payload"]["byte_len"], 3);
        assert!(value.get("payload_offset").is_none());
        assert_eq!(value["radius"], 3.0);
        assert_eq!(value["program"]["kind"], "sum");
        assert_eq!(
            fidelity.metadata["native.catia.record.payload_offset"],
            "17"
        );
        assert!(fidelity.unrepresented().is_empty());
    }
}
