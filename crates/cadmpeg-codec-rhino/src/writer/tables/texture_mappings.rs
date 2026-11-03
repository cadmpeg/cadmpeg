// SPDX-License-Identifier: Apache-2.0
//! Texture-mapping table records re-emitted from held typed fields.
//!
//! A mapping record is its typed header (identity, type, projection,
//! transforms, name, space, cap) around the retained primitive class-wrapper
//! chunk, which the decoder keeps verbatim because the custom primitive is
//! an untyped polymorphic object (`docs/formats/rhino_3dm.md` §7.2.19). A
//! mapping whose primitive bytes are unavailable has no short encoding the
//! writer can frame, so it stays omitted with a per-record loss.

use super::{
    anonymous, class_wrapper, crc_chunk_with_direct, records, utf16, uuid, CadIr, LossNote, Map,
    Value,
};
use crate::loss::RhinoLossCode;
use crate::writer::WriteError;
use cadmpeg_ir::native::bytes::NativeBytes;

const TEXTURE_MAPPING_RECORD: u32 = 0x2000_807a;

pub(in crate::writer) struct TextureMappingExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    pub(in crate::writer) written: std::collections::BTreeSet<String>,
    pub(in crate::writer) losses: Vec<LossNote>,
}

fn u32_field(fields: &Map<String, Value>, name: &str) -> Result<u32, WriteError> {
    u32::try_from(
        fields
            .get(name)
            .and_then(Value::as_i64)
            .ok_or_else(|| WriteError::Omit(format!("Rhino texture mapping {name} needs an integer")))?,
    )
    .map_err(|_| WriteError::Omit(format!("Rhino texture mapping {name} exceeds u32")))
}

fn transform(fields: &Map<String, Value>, name: &str) -> Result<[f64; 16], WriteError> {
    let rows = fields
        .get(name)
        .and_then(Value::as_array)
        .filter(|rows| rows.len() == 4)
        .ok_or_else(|| WriteError::Omit(format!("Rhino texture mapping {name} needs four rows")))?;
    let mut out = [0.0; 16];
    for (row, target) in rows.iter().zip(out.chunks_exact_mut(4)) {
        let cells = row
            .as_array()
            .filter(|cells| cells.len() == 4)
            .ok_or_else(|| {
                WriteError::Omit(format!("Rhino texture mapping {name} needs four columns"))
            })?;
        for (cell, value) in cells.iter().zip(target.iter_mut()) {
            *value = cell.as_f64().filter(|value| value.is_finite()).ok_or_else(|| {
                WriteError::Omit(format!("Rhino texture mapping {name} needs finite numbers"))
            })?;
        }
    }
    Ok(out)
}

fn primitive_bytes(fields: &Map<String, Value>) -> Result<Vec<u8>, WriteError> {
    let retained: Option<NativeBytes<Vec<u8>>> = fields
        .get("primitive_bytes")
        .cloned()
        .map(serde_json::from_value::<NativeBytes<Vec<u8>>>)
        .transpose()
        .map_err(|_| {
            WriteError::Omit("Rhino texture mapping primitive bytes are not hexadecimal".into())
        })?;
    let bytes = retained
        .ok_or_else(|| {
            WriteError::Omit("Rhino texture mapping has no retained primitive bytes".into())
        })?
        .into_inner();
    if bytes.is_empty() {
        return Err(WriteError::Omit(
            "Rhino texture mapping primitive bytes are empty".into(),
        ));
    }
    Ok(bytes)
}

pub(in crate::writer) fn mappings(ir: &CadIr) -> Result<TextureMappingExport, WriteError> {    let mut output = TextureMappingExport {
        records: Vec::new(),
        written: std::collections::BTreeSet::new(),
        losses: Vec::new(),
    };
    for record in records(ir, "texture_mappings") {
        let fields = record.fields()?;
        let encoded = (|| {
            let minor = if fields
                .get("texture_space")
                .and_then(Value::as_i64)
                .is_some_and(|space| space != 0)
                || fields.get("capped").and_then(Value::as_bool).unwrap_or(false)
            {
                1
            } else {
                0
            };
            let mut body = Vec::new();
            body.extend(uuid(fields.get("source_uuid"), record.id()));
            body.extend(u32_field(&fields, "mapping_type")?.to_le_bytes());
            body.extend(u32_field(&fields, "projection")?.to_le_bytes());
            for value in transform(&fields, "primitive_transform")? {
                body.extend(value.to_le_bytes());
            }
            for value in transform(&fields, "uvw_transform")? {
                body.extend(value.to_le_bytes());
            }
            body.extend(utf16(
                fields.get("name").and_then(Value::as_str).ok_or_else(|| {
                    WriteError::Omit("Rhino texture mapping name needs a string".into())
                })?,
            )?);
            body.extend(primitive_bytes(&fields)?);
            if minor >= 1 {
                body.extend(u32_field(&fields, "texture_space")?.to_le_bytes());
                body.push(u8::from(
                    fields.get("capped").and_then(Value::as_bool).unwrap_or(false),
                ));
            }
            let anonymous = anonymous(1, minor, &body).map_err(WriteError::Fail)?;
            crc_chunk_with_direct(
                TEXTURE_MAPPING_RECORD,
                &class_wrapper(
                    crate::wire::Uuid::to_wire(crate::presentation::TEXTURE_MAPPING),
                    &anonymous,
                )
                .map_err(WriteError::Fail)?,
                &[],
            )
            .map_err(WriteError::Fail)
        })();
        match encoded {
            Ok(record_bytes) => {
                output.records.push(record_bytes);
                output.written.insert(record.id().to_owned());
            }
            Err(WriteError::Omit(reason)) => output.losses.push(
                RhinoLossCode::WriterMaterialOmitted
                    .note(format!("{}: {reason}", record.id())),
            ),
            Err(error) => return Err(error),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::mappings;
    use crate::loss::RhinoLossCode;
    use cadmpeg_ir::{CadIr, NativeRecord};
    use serde_json::json;

    fn mapping_record(id: &str, fields: serde_json::Value) -> CadIr {
        let mut ir = CadIr::empty();
        ir.native.namespace_mut("rhino").arenas_mut().insert(
            "texture_mappings".into(),
            vec![NativeRecord::new(
                id.try_into().expect("texture mapping identity"),
                serde_json::from_value(fields).expect("texture mapping fields"),
            )
            .expect("texture mapping record")],
        );
        ir.finalize();
        ir
    }

    fn held_fields() -> serde_json::Value {
        json!({
            "source_uuid": "485b60b1-196d-4730-923e-83fe02065baf",
            "name": "test mapping",
            "mapping_type": 3,
            "projection": 1,
            "primitive_transform": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
            "uvw_transform": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
            "primitive_class_uuid": "00000000-0000-0000-0000-000000000000",
            "primitive_bytes": "deadbeefcafe",
            "texture_space": 0,
            "capped": true,
        })
    }

    #[test]
    fn held_mapping_reemits_typed_fields_around_verbatim_primitive_bytes() {
        let id = "rhino:presentation:texture_mapping#test";
        let ir = mapping_record(id, held_fields());
        let exported = mappings(&ir).expect("held mapping encodes");
        assert!(exported.losses.is_empty(), "{:?}", exported.losses);
        assert_eq!(exported.records.len(), 1);
        assert!(exported.written.contains(id));
        let record = &exported.records[0];
        let primitive = [0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe];
        assert!(
            record.windows(primitive.len()).any(|window| window == primitive),
            "the retained primitive chunk is embedded verbatim"
        );
        let name = "test mapping".encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect::<Vec<_>>();
        assert!(
            record.windows(name.len()).any(|window| window == name.as_slice()),
            "the mapping name is emitted as UTF-16"
        );
    }

    #[test]
    fn mapping_without_primitive_bytes_names_its_own_omission() {
        let id = "rhino:presentation:texture_mapping#unprimitived";
        let mut fields = held_fields();
        fields.as_object_mut().expect("fields object").remove("primitive_bytes");
        let ir = mapping_record(id, fields);
        let exported = mappings(&ir).expect("omission is a loss, not a failure");
        assert!(exported.records.is_empty());
        assert!(!exported.written.contains(id));
        assert_eq!(exported.losses.len(), 1);
        assert_eq!(
            exported.losses[0].code,
            RhinoLossCode::WriterMaterialOmitted.note("").code
        );
        assert!(exported.losses[0].message.contains(id));
    }
}
