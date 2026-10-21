// SPDX-License-Identifier: Apache-2.0
//! Admission of semantic scalar, count and reference columns.

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(super) fn fail(record: &Value, message: &str) -> NativeConvertError {
    super::invalid(record["id"].as_str().unwrap_or("NX record"), message)
}

pub(super) fn field<T: DeserializeOwned>(
    record: &Value,
    key: &str,
) -> Result<T, NativeConvertError> {
    serde_json::from_value(record[key].clone())
        .map_err(|error| fail(record, &format!("{key}: {error}")))
}

pub(super) fn uint(record: &Value, key: &str, maximum: u64) -> Result<u64, NativeConvertError> {
    record[key]
        .as_u64()
        .filter(|value| *value <= maximum)
        .ok_or_else(|| {
            fail(
                record,
                &format!("{key} is outside its unsigned scalar range"),
            )
        })
}

pub(super) fn nonzero_byte(record: &Value, key: &str) -> Result<(), NativeConvertError> {
    if uint(record, key, u64::from(u8::MAX))? == 0 {
        return Err(fail(record, &format!("{key} must be nonzero")));
    }
    Ok(())
}

pub(super) fn array<'a>(record: &'a Value, key: &str) -> Result<&'a [Value], NativeConvertError> {
    record[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| fail(record, &format!("{key} is not an array")))
}

pub(super) fn sized_array<'a>(
    record: &'a Value,
    key: &str,
    count: usize,
) -> Result<&'a [Value], NativeConvertError> {
    let values = array(record, key)?;
    if values.len() != count {
        return Err(fail(record, &format!("{key} must contain {count} values")));
    }
    Ok(values)
}

pub(super) fn index(record: &Value) -> Result<u64, NativeConvertError> {
    uint(record, "object_index", u64::from(u16::MAX))
}

pub(super) fn finite(record: &Value, value: &Value, key: &str) -> Result<(), NativeConvertError> {
    if !value.as_f64().is_some_and(f64::is_finite) {
        return Err(fail(record, &format!("{key} must be finite")));
    }
    Ok(())
}

pub(super) fn unique<'a>(
    namespace: &'a NativeNamespace,
    arena: &str,
    id: &str,
) -> Result<&'a cadmpeg_ir::NativeRecord, NativeConvertError> {
    let mut matches = namespace
        .arenas()
        .get(arena)
        .into_iter()
        .flatten()
        .filter(|record| record.id() == id);
    let record = matches
        .next()
        .ok_or_else(|| super::invalid(id, &format!("{arena} reference does not resolve")))?;
    if matches.next().is_some() {
        return Err(super::invalid(
            id,
            &format!("duplicate identity in {arena}"),
        ));
    }
    Ok(record)
}

pub(super) fn link(
    namespace: &NativeNamespace,
    record: &Value,
    key: &str,
    arena: &str,
) -> Result<(), NativeConvertError> {
    let id = record[key]
        .as_str()
        .ok_or_else(|| fail(record, &format!("{key} is not an identity")))?;
    unique(namespace, arena, id)?;
    Ok(())
}

pub(super) fn optional_link(
    namespace: &NativeNamespace,
    record: &Value,
    key: &str,
    arena: &str,
) -> Result<(), NativeConvertError> {
    if record.get(key).is_some() {
        link(namespace, record, key, arena)?;
    }
    Ok(())
}

pub(super) fn data_block(
    namespace: &NativeNamespace,
    reference: &Value,
) -> Result<(), NativeConvertError> {
    let object_index = index(reference)?;
    if let Some(id) = reference.get("data_block") {
        let id = id
            .as_str()
            .ok_or_else(|| fail(reference, "data_block must name a target"))?;
        let target = unique(namespace, "data_blocks", id)?;
        let fields = target.fields()?;
        if object_index == 0
            || fields.get("block_ordinal").and_then(Value::as_u64) != Some(object_index)
        {
            return Err(fail(
                reference,
                "data_block must match its nonzero object_index",
            ));
        }
    }
    Ok(())
}

pub(super) fn placement(record: &Value) -> Result<(), NativeConvertError> {
    let matrix: [f64; 12] = field(record, "matrix")?;
    if !matrix.iter().all(|value| value.is_finite())
        || !super::super::assembly_placement::rigid_rotation(&matrix)
    {
        return Err(fail(record, "matrix must contain a finite rigid rotation"));
    }
    let label: String = field(record, "label")?;
    if !label.is_ascii() || label.as_bytes().contains(&0) || label.len() > 253 {
        return Err(fail(record, "label must fit the placement text frame"));
    }
    field::<crate::canonical_uuid::CanonicalUuid<String>>(record, "uuid")?;
    let source: String = field(record, "source_entry")?;
    if source != "/Root/UG_PART/UG_PART" {
        return Err(fail(
            record,
            "assembly placement source_entry is inconsistent",
        ));
    }
    Ok(())
}

pub(super) fn storage(record: &Value) -> Result<(), NativeConvertError> {
    let header: [u32; 9] = field(record, "header_words")?;
    let boundaries: Vec<u32> = field(record, "boundaries")?;
    let table: Vec<u32> = field(record, "table_words")?;
    let spans = array(record, "physical_spans")?;
    let rows = array(record, "terminal_rows")?;
    let count = u64::from(header[0]);
    let length = |length| cadmpeg_core::decode::u64_from_index(length);
    if record["source_entry"].as_str() != Some("/Root/FastLoad/RMFastLoad")
        || count == 0
        || [2, 5, 6, 7].into_iter().any(|index| header[index] != 0)
        || count.checked_add(1) != Some(length(boundaries.len()))
        || length(table.len()) != count
        || length(spans.len()) != count
        || length(rows.len()) != u64::from(header[4])
        || boundaries.first() != Some(&0)
        || boundaries.get(1).is_none_or(|value| *value == 0)
        || boundaries.last().is_none_or(|value| *value > header[8])
    {
        return Err(fail(
            record,
            "RMFastLoad storage must have complete consistent physical arrays and terminal rows",
        ));
    }
    for (span, pair) in spans.iter().zip(boundaries.windows(2)) {
        if pair[1].checked_sub(pair[0]).map(u64::from) != span["byte_len"].as_u64() {
            return Err(fail(
                record,
                "RMFastLoad physical_spans disagree with boundaries",
            ));
        }
        field::<cadmpeg_ir::hash::digest::Sha256Digest>(span, "sha256")?;
    }
    for row in rows {
        field::<[crate::om::rmfastload::TerminalAtom; 3]>(row, "tokens")?;
    }
    Ok(())
}
