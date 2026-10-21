// SPDX-License-Identifier: Apache-2.0
//! Source layout is sidecar metadata; native records hold typed values only.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{document::CadIr, native::NativeRecord, SourceFidelity};
use serde_json::Value;

pub(super) fn move_layout(
    _ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    fidelity: &mut SourceFidelity,
) -> Result<(), CodecError> {
    for (arena, records) in ir.native.namespace_mut("creo").arenas_mut() {
        for record in records {
            let mut fields = record.fields()?;
            fields.insert("id".into(), Value::String(record.id().to_owned()));
            let path = format!("native.creo.{arena}.{}", record.id());
            let mut value = Value::Object(fields);
            extract(&mut value, &path, fidelity)?;
            let Value::Object(fields) = value else {
                return Err(CodecError::malformed("Creo native record is not an object"));
            };
            *record = NativeRecord::new(
                cadmpeg_ir::ids::Identity::try_from(record.id()).map_err(CodecError::malformed)?,
                fields,
            )?;
        }
    }
    if let Some(source) = &mut ir.source {
        let layout = source
            .attributes
            .keys()
            .filter(|key| {
                key.as_str().contains("offset")
                    || key.as_str().contains("sha256")
                    || key.as_str().contains("pointer")
                    || key.as_str() == "configuration_state"
            })
            .cloned()
            .collect::<Vec<_>>();
        for key in layout {
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
            if let Some(classes) = object.remove("source_untyped") {
                let classes = serde_json::from_value::<cadmpeg_ir::ArenaClasses>(classes)
                    .map_err(|error| CodecError::malformed(error.to_string()))?;
                let byte_len = object
                    .get("byte_len")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        CodecError::malformed("Creo source evidence has no byte length")
                    })?;
                let sha256 = object
                    .get("sha256")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CodecError::malformed("Creo source evidence has no digest"))?
                    .to_owned();
                let digest = cadmpeg_ir::hash::sha256_hex(path.as_bytes());
                let id = cadmpeg_ir::ids::UnknownId::mint(format!("creo:source:evidence#{digest}"))
                    .map_err(CodecError::malformed)?;
                // This source owner is the exact record image itself. Its start
                // is zero; its containing file/body layout is kept in metadata.
                fidelity.insert_retained_record(
                    id,
                    cadmpeg_ir::source_fidelity::RetainedSourceRecord::from_bytes(
                        path.to_owned(),
                        0,
                        cadmpeg_ir::source_fidelity::RetainedBytes::Digest {
                            byte_len,
                            sha256: cadmpeg_ir::hash::digest::Sha256Digest::try_from(sha256)
                                .map_err(|error| CodecError::malformed(error.to_string()))?,
                        },
                    )?
                    .with_untyped(classes),
                )?;
            }
            let offsets = object
                .keys()
                .filter(|key| is_layout_key(key))
                .cloned()
                .collect::<Vec<_>>();
            for key in offsets {
                if let Some(value) = object.remove(&key) {
                    fidelity
                        .metadata
                        .insert(format!("{path}.{key}"), value.to_string());
                }
            }
            for (key, child) in object {
                extract(child, &format!("{path}.{key}"), fidelity)?;
            }
        }
        Value::Array(values) => {
            for (ordinal, child) in values.iter_mut().enumerate() {
                extract(child, &format!("{path}[{ordinal}]"), fidelity)?;
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
        || key == "byte_start"
        || key == "byte_end"
}
