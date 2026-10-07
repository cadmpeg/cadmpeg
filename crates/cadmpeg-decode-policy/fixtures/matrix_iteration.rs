// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub fn fixed_matrix_flatten(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    let matrix = [[1_u8, 2, 3], [4, 5, 6]];
    let rows = ctx.admit_iter(&matrix, "matrix rows")?;
    for value in rows.flatten() {
        let _value = value;
    }
    Ok(())
}

pub fn borrowed_btree_source(
    ctx: &DecodeContext<'_>,
    values: &BTreeMap<String, u8>,
) -> Result<(), CodecError> {
    for (key, value) in ctx.admit_iter(values, "borrowed tree")? {
        let _entry = (key, value);
    }
    Ok(())
}

pub fn owned_btree_source(
    ctx: &DecodeContext<'_>,
    values: BTreeMap<String, u8>,
) -> Result<(), CodecError> {
    for (key, value) in ctx.admit_iter(values, "owned tree")? {
        let _entry = (key, value);
    }
    Ok(())
}

pub fn borrowed_json_map_source(
    ctx: &DecodeContext<'_>,
    values: &Map<String, Value>,
) -> Result<(), CodecError> {
    for (key, value) in ctx.admit_iter(values, "borrowed JSON map")? {
        let _entry = (key, value);
    }
    Ok(())
}

pub fn owned_json_map_source(
    ctx: &DecodeContext<'_>,
    values: Map<String, Value>,
) -> Result<(), CodecError> {
    for (key, value) in ctx.admit_iter(values, "owned JSON map")? {
        let _entry = (key, value);
    }
    Ok(())
}

pub fn owned_json_map_early_break(
    ctx: &DecodeContext<'_>,
    values: Map<String, Value>,
) -> Result<(), CodecError> {
    for (key, value) in ctx.admit_iter(values, "owned nested JSON map")? {
        let _key_length = key.len();
        if value.is_array() || value.is_object() {
            break;
        }
    }
    Ok(())
}

pub fn tolerant_product_links(
    ctx: &DecodeContext<'_>,
    properties: &Map<String, Value>,
) -> Result<(), CodecError> {
    for (name, value) in ctx.admit_iter(properties, "Product properties")? {
        if !ctx.equal(name.as_str(), "links", "Product property name")? {
            continue;
        }
        let Value::Array(values) = value else {
            continue;
        };
        for target in ctx.admit_iter(values.as_slice(), "Product link values")? {
            let Value::String(target) = target else {
                continue;
            };
            if target.is_empty() {
                continue;
            }
        }
    }
    Ok(())
}
