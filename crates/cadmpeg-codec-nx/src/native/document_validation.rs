// SPDX-License-Identifier: Apache-2.0
//! Typed document links, independent of source framing and byte positions.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace, NativeRecord};
use serde_json::{Map, Value};

pub(crate) fn validate(
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
            let fields = record.fields()?;
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
            let form = number(record, &fields, "occurrence_lane_form")?;
            if form > 1 || lane_form.is_some_and(|previous| previous != form) {
                return Err(invalid(record.id(), "occurrence lane forms disagree"));
            }
            lane_form = Some(form);
            if !matches!(number(record, &fields, "marker")?, 49 | 57) {
                return Err(invalid(record.id(), "unknown occurrence marker"));
            }
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
                .ok_or_else(|| invalid(record.id(), "TOC entry does not resolve in document"))?;
            if fields.get("segment_type").and_then(Value::as_u64)
                != entry.get("attributes").and_then(Value::as_u64)
            {
                return Err(invalid(
                    record.id(),
                    "segment type disagrees with TOC attributes",
                ));
            }
            for key in ["segment_id", "segment_byte_len"] {
                if fields.get(key) != entry.get(key) {
                    return Err(invalid(record.id(), "segment disagrees with its TOC entry"));
                }
            }
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
                require_link(record, &fields, "segment", arena("display_jt_segments"))?;
                let segment = arena("display_jt_segments")
                    .and_then(|segments| segments.get(text(record, &fields, "segment").ok()?))
                    .ok_or_else(|| invalid(record.id(), "segment does not resolve"))?;
                let owner = segment.fields()?;
                if name == "display_jt_shape_lod_elements" {
                    if number(segment, &owner, "segment_type")? != 7 {
                        return Err(invalid(
                            record.id(),
                            "shape LOD owner is not a type-7 segment",
                        ));
                    }
                } else if number(record, &fields, "segment_type")?
                    != number(segment, &owner, "segment_type")?
                    || owner.get("compression").is_none_or(Value::is_null)
                {
                    return Err(invalid(
                        record.id(),
                        "compressed element owner type or envelope disagrees",
                    ));
                }
                if name == "display_jt_compressed_element_sequences" {
                    let Some(ids) = fields.get("elements").and_then(Value::as_array) else {
                        return Err(invalid(record.id(), "sequence elements are absent"));
                    };
                    for (ordinal, id) in ids.iter().enumerate() {
                        let element = id
                            .as_str()
                            .and_then(|id| arena("display_jt_compressed_elements")?.get(id))
                            .ok_or_else(|| {
                                invalid(record.id(), "sequence element does not resolve")
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
                    }
                }
            }
        }
    }
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
                    if !value.as_array().is_some_and(|bits| {
                        bits.len() == 3
                            && bits
                                .iter()
                                .all(|bit| bit.as_u64().is_some_and(|bit| bit <= 32))
                    }) {
                        return Err(invalid(
                            record.id(),
                            "coordinate quantization requires three bit counts in 0..=32",
                        ));
                    }
                } else if name == "legal_owner_flags" {
                    if !value.as_array().is_some_and(|flags| {
                        matches!(flags.len(), 14 | 16) && flags.iter().all(Value::is_boolean)
                    }) {
                        return Err(invalid(
                            record.id(),
                            "legal owners require fourteen or sixteen Boolean flags",
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
