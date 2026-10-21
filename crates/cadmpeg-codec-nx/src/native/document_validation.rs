// SPDX-License-Identifier: Apache-2.0
//! Typed document links, independent of source framing and byte positions.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace, NativeRecord};
use serde_json::{Map, Value};

mod construction;
mod surfaces;
mod values;

pub(crate) fn validate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    namespace: &NativeNamespace,
    ir: &cadmpeg_ir::CadIr,
) -> Result<(), NativeConvertError> {
    let products = ir
        .model
        .product_definitions
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    let attributes = ir
        .model
        .attributes
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    let appearances = ir
        .model
        .appearances
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut arenas = BTreeMap::new();
    for (name, records) in namespace.arenas() {
        let mut index = BTreeMap::new();
        if name != "unknowns"
            && !super::catalogue::CATALOGUE
                .iter()
                .any(|entry| entry.arena == name)
        {
            return Err(invalid(name, "unknown NX native arena"));
        }
        for record in records {
            let fields = record.copy_fields(ctx)?;
            validate_values(record, &Value::Object(fields.clone()))?;
            for (field, resolves) in [
                ("neutral_product", &products),
                ("neutral_attribute", &attributes),
                ("neutral_appearance", &appearances),
            ] {
                if let Some(value) = fields.get(field) {
                    if !value.as_str().is_some_and(|id| resolves.contains(id)) {
                        return Err(invalid(
                            record.id(),
                            "neutral content link does not resolve",
                        ));
                    }
                }
            }
            if index.insert(record.id(), record).is_some() {
                return Err(invalid(record.id(), "duplicate native record identity"));
            }
        }
        arenas.insert(name.as_str(), index);
    }
    let arena = |name: &str| arenas.get(name);
    let mut lane_form = None;
    if let Some(occurrences) = arena("fast_load_component_occurrences") {
        for record in occurrences.values() {
            let fields = record.fields()?;
            typed_field::<u32>(record, &fields, "ordinal")?;
            typed_field::<String>(record, &fields, "source_entry")?;
            let form = number(record, &fields, "occurrence_lane_form")?;
            if form > 1 || lane_form.is_some_and(|previous| previous != form) {
                return Err(invalid(
                    record.id(),
                    "occurrence_lane_form disagrees across the roster",
                ));
            }
            lane_form = Some(form);
            if !matches!(number(record, &fields, "marker")?, 49 | 57) {
                return Err(invalid(record.id(), "unknown occurrence marker"));
            }
        }
        for record in occurrences.values() {
            let fields = record.fields()?;
            let index = number(record, &fields, "prototype_index")?;
            if !(1..=254).contains(&index)
                || text(record, &fields, "prototype")?
                    != format!("nx:fast-load:prototype#{}", index - 1)
            {
                return Err(invalid(
                    record.id(),
                    "prototype and prototype index disagree",
                ));
            }
            for (field, target) in [
                ("prototype", "fast_load_component_prototypes"),
                ("component_uuid", "fast_load_component_uuids"),
            ] {
                require_link(record, &fields, field, arena(target))?;
            }
        }
    }
    if let Some(documents) = arena("display_jt_documents") {
        for document in documents.values() {
            let fields = document.fields()?;
            super::display_jt::admission::admit_document_values(document.id(), &fields)?;
            let entries = fields
                .get("toc_entries")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid(document.id(), "TOC entries are absent"))?;
            let mut ids = BTreeSet::new();
            for entry in entries {
                let id = entry
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid(document.id(), "TOC entry identity is absent"))?;
                if !ids.insert(id) {
                    return Err(invalid(document.id(), "duplicate TOC entry identity"));
                }
            }
        }
    }
    if let Some(segments) = arena("display_jt_segments") {
        for record in segments.values() {
            let fields = record.fields()?;
            typed_field::<cadmpeg_ir::hash::digest::Sha256Digest>(
                record,
                &fields,
                "payload_sha256",
            )?;
            if let Some(compression) = fields.get("compression").filter(|value| !value.is_null()) {
                super::display_jt::admission::admit_compression_values(record.id(), compression)?;
            }
            require_link(record, &fields, "document", arena("display_jt_documents"))?;
            let document = arena("display_jt_documents")
                .and_then(|documents| documents.get(text(record, &fields, "document").ok()?))
                .ok_or_else(|| invalid(record.id(), "document does not resolve"))?;
            let document_fields = document.fields()?;
            let toc_id = text(record, &fields, "toc_entry")?;
            let entry = document_fields
                .get("toc_entries")
                .and_then(Value::as_array)
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(toc_id))
                })
                .and_then(Value::as_object)
                .ok_or_else(|| invalid(record.id(), "toc_entry does not resolve in document"))?;
            let attributes: cadmpeg_ir::native::bytes::NativeBytes<[u8; 4]> =
                serde_json::from_value(entry.get("attributes").cloned().unwrap_or(Value::Null))
                    .map_err(|error| {
                        invalid(record.id(), &format!("toc_entry.attributes: {error}"))
                    })?;
            if number(record, &fields, "segment_type")?
                != u64::from(cadmpeg_core::bytes::assemble_u32_be(*attributes))
            {
                return Err(invalid(
                    record.id(),
                    "segment_type disagrees with toc_entry.attributes",
                ));
            }
            for key in ["segment_id", "segment_byte_len"] {
                if fields.get(key) != entry.get(key) {
                    return Err(invalid(
                        record.id(),
                        &format!("{key} disagrees with toc_entry"),
                    ));
                }
            }
            typed_field::<cadmpeg_ir::native::bytes::NativeBytes<[u8; 16]>>(
                record,
                &fields,
                "segment_id",
            )?;
            typed_field::<cadmpeg_ir::native::bytes::NativeBytes<[u8; 16]>>(
                document,
                entry,
                "segment_id",
            )?;
            typed_field::<u32>(record, &fields, "segment_type")?;
            typed_field::<u32>(record, &fields, "segment_byte_len")?;
        }
    }
    for name in [
        "display_jt_shape_lod_elements",
        "display_jt_compressed_elements",
        "display_jt_compressed_element_sequences",
    ] {
        if let Some(elements) = arena(name) {
            for record in elements.values() {
                let fields = record.fields()?;
                if name != "display_jt_compressed_element_sequences" {
                    typed_field::<u32>(record, &fields, "ordinal")?;
                    typed_field::<u32>(record, &fields, "object_id")?;
                    typed_field::<cadmpeg_ir::native::bytes::NativeBytes<[u8; 16]>>(
                        record,
                        &fields,
                        "object_type_id",
                    )?;
                    typed_field::<cadmpeg_ir::hash::digest::Sha256Digest>(
                        record,
                        &fields,
                        "body_sha256",
                    )?;
                    let length: u32 = typed_field(record, &fields, "body_byte_len")?;
                    let base: u8 = typed_field(record, &fields, "object_base_type")?;
                    if name == "display_jt_shape_lod_elements" && base != 4 {
                        return Err(invalid(record.id(), "object_base_type must be 4"));
                    }
                    if name == "display_jt_compressed_elements" && length.checked_add(21).is_none()
                    {
                        return Err(invalid(
                            record.id(),
                            "body_byte_len must fit the complete element frame",
                        ));
                    }
                }
                require_link(record, &fields, "segment", arena("display_jt_segments"))?;
                let segment = arena("display_jt_segments")
                    .and_then(|segments| segments.get(text(record, &fields, "segment").ok()?))
                    .ok_or_else(|| invalid(record.id(), "segment does not resolve"))?;
                let owner = segment.fields()?;
                if name != "display_jt_shape_lod_elements"
                    && owner.get("compression").is_none_or(Value::is_null)
                {
                    return Err(invalid(record.id(), "segment has no compression envelope"));
                }
                if name == "display_jt_shape_lod_elements" {
                    if number(segment, &owner, "segment_type")? != 7 {
                        return Err(invalid(
                            record.id(),
                            "shape LOD owner is not a type-7 segment",
                        ));
                    }
                } else if number(record, &fields, "segment_type")?
                    != number(segment, &owner, "segment_type")?
                {
                    return Err(invalid(record.id(), "segment_type disagrees with segment"));
                }
                if name == "display_jt_compressed_element_sequences" {
                    let Some(ids) = fields.get("elements").and_then(Value::as_array) else {
                        return Err(invalid(record.id(), "sequence elements are absent"));
                    };
                    let mut framed_length = 20_u64;
                    typed_field::<u32>(record, &fields, "framed_byte_len")?;
                    typed_field::<cadmpeg_ir::hash::digest::Sha256Digest>(
                        record,
                        &fields,
                        "tail_sha256",
                    )?;
                    for (ordinal, id) in ids.iter().enumerate() {
                        let element = id
                            .as_str()
                            .and_then(|id| arena("display_jt_compressed_elements")?.get(id))
                            .ok_or_else(|| {
                                invalid(record.id(), "elements contains an unresolved identity")
                            })?;
                        let element_fields = element.fields()?;
                        if element_fields.get("segment") != fields.get("segment")
                            || number(element, &element_fields, "ordinal")?
                                != cadmpeg_core::decode::u64_from_index(ordinal)
                        {
                            return Err(invalid(
                                record.id(),
                                "sequence element owner or ordinal disagrees",
                            ));
                        }
                        let length = number(element, &element_fields, "body_byte_len")?;
                        framed_length = framed_length
                            .checked_add(25)
                            .and_then(|sum| sum.checked_add(length))
                            .ok_or_else(|| {
                                invalid(record.id(), "framed_byte_len overflows body_byte_len sum")
                            })?;
                    }
                    if number(record, &fields, "framed_byte_len")? != framed_length {
                        return Err(invalid(record.id(), "framed_byte_len disagrees with element body_byte_len values and end marker"));
                    }
                }
            }
        }
    }
    construction::validate(ctx, namespace)?;
    surfaces::validate(ctx, namespace)?;
    Ok(())
}

/// Validate the document form, which contains semantic values and source
/// evidence rather than the source scanner's byte-addressed reload format.
fn validate_values(record: &NativeRecord, value: &Value) -> Result<(), NativeConvertError> {
    match value {
        Value::Object(fields) => {
            if fields.contains_key("record_id") && fields.contains_key("sha256") {
                let valid = fields
                    .get("record_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| cadmpeg_ir::ids::Identity::try_from(id).is_ok())
                    && fields.get("byte_len").and_then(Value::as_u64).is_some()
                    && fields
                        .get("sha256")
                        .and_then(Value::as_str)
                        .is_some_and(|hash| {
                            cadmpeg_ir::hash::digest::Sha256Digest::try_from(hash.to_owned())
                                .is_ok()
                        });
                if !valid {
                    return Err(invalid(record.id(), "invalid source record evidence"));
                }
            }
            for (name, value) in fields {
                if name == "component_quantization_bits" {
                    let bits = serde_json::from_value::<
                        cadmpeg_ir::native::bytes::NativeBytes<[u8; 3]>,
                    >(value.clone());
                    if !bits.is_ok_and(|bits| bits.iter().all(|bit| *bit <= 32)) {
                        return Err(invalid(
                            record.id(),
                            "coordinate quantization requires three bit counts in 0..=32",
                        ));
                    }
                } else if name == "legal_owner_flags" {
                    let flags = serde_json::from_value::<
                        cadmpeg_ir::native::bytes::NativeBytes<[u8; 16]>,
                    >(value.clone());
                    if !flags.is_ok_and(|flags| {
                        crate::parasolid::LegalOwnerFlags::try_from(flags.as_slice()).is_ok()
                    }) {
                        return Err(invalid(
                            record.id(),
                            "legal_owner_flags require sixteen binary byte values",
                        ));
                    }
                }
                validate_values(record, value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_values(record, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn text<'a>(
    record: &NativeRecord,
    fields: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, NativeConvertError> {
    fields
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(record.id(), &format!("{key} is not text")))
}

fn typed_field<T: serde::de::DeserializeOwned>(
    record: &NativeRecord,
    fields: &Map<String, Value>,
    key: &str,
) -> Result<T, NativeConvertError> {
    serde_json::from_value(fields.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|error| invalid(record.id(), &format!("{key}: {error}")))
}
fn number(
    record: &NativeRecord,
    fields: &Map<String, Value>,
    key: &str,
) -> Result<u64, NativeConvertError> {
    fields
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(record.id(), &format!("{key} is not an unsigned integer")))
}
fn require_link(
    record: &NativeRecord,
    fields: &Map<String, Value>,
    key: &str,
    targets: Option<&BTreeMap<&str, &NativeRecord>>,
) -> Result<(), NativeConvertError> {
    let id = text(record, fields, key)?;
    if targets.is_some_and(|targets| targets.contains_key(id)) {
        Ok(())
    } else {
        Err(invalid(record.id(), &format!("{key} does not resolve")))
    }
}
fn invalid(id: &str, message: &str) -> NativeConvertError {
    NativeConvertError::InvalidCollection(format!("NX {id}: {message}"))
}
