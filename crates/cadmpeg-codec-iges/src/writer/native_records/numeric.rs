// SPDX-License-Identifier: Apache-2.0
//! Preserve native numeric token kinds when their current values agree.

use super::{CadIr, Entity, NativeRecord};
use cadmpeg_ir::scalar::FiniteReal;
use serde_json::Value;

fn fields(body: &[u8]) -> Option<Vec<&[u8]>> {
    let mut result = Vec::new();
    let mut start = 0;
    while start < body.len() {
        let mut end = start;
        while end < body.len() && body[end].is_ascii_digit() {
            end += 1;
        }
        if end > start && matches!(body.get(end), Some(b'H' | b'h')) {
            let count = std::str::from_utf8(&body[start..end])
                .ok()?
                .parse::<usize>()
                .ok()?;
            end = end.checked_add(1)?.checked_add(count)?;
            if end >= body.len() {
                return None;
            }
        } else {
            end = start;
            while end < body.len() && !matches!(body[end], b',' | b';') {
                end += 1;
            }
        }
        let delimiter = *body.get(end)?;
        if !matches!(delimiter, b',' | b';') {
            return None;
        }
        result.push(&body[start..end]);
        start = end + 1;
        if delimiter == b';' {
            return (start == body.len()).then_some(result);
        }
    }
    None
}

pub(super) fn retain(ir: &CadIr, source: &NativeRecord, entity: &mut Entity) {
    let Some(source_id) = source
        .field("source_entity")
        .and_then(|value| value.as_str().map(str::to_owned))
    else {
        return;
    };
    let Some(native) = ir
        .native
        .namespace("iges")
        .and_then(|ns| ns.arenas().get("entities"))
        .and_then(|records| records.iter().find(|record| record.id() == source_id))
    else {
        return;
    };
    if native.field("entity_type").and_then(|value| value.as_u64())
        != Some(u64::from(entity.type_code))
    {
        return;
    }
    let Some(parameters) = native.field("parameters") else {
        return;
    };
    let Some(parameters) = parameters.as_array() else {
        return;
    };
    let Some(tokens) = fields(&entity.parameter_body) else {
        return;
    };
    if parameters.len() != tokens.len() + 1 {
        return;
    }
    let mut output = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if index != 0 {
            output.push(b',');
        }
        let native = &parameters[index + 1]["value"];
        let value = match native.get("kind").and_then(Value::as_str) {
            Some("integer") => native
                .get("value")
                .and_then(Value::as_i64)
                .and_then(cadmpeg_core::convert::f64_from_i64),
            Some("real") => native.get("value").and_then(Value::as_f64),
            _ => None,
        };
        let current = std::str::from_utf8(token)
            .ok()
            .and_then(|text| text.replace(['D', 'd'], "E").parse::<f64>().ok());
        let replacement = if value
            .zip(current)
            .is_some_and(|(left, right)| left == right)
        {
            match native.get("kind").and_then(Value::as_str) {
                Some("integer") => native
                    .get("value")
                    .and_then(Value::as_i64)
                    .map(|value| value.to_string()),
                Some("real") => value.and_then(FiniteReal::new).map(super::number),
                _ => None,
            }
        } else {
            None
        };
        match replacement {
            Some(value) => output.extend_from_slice(value.as_bytes()),
            None => output.extend_from_slice(token),
        }
    }
    output.push(b';');
    entity.parameter_body = output;
}
