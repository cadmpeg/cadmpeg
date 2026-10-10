// SPDX-License-Identifier: Apache-2.0
//! Copies native value storage directly through the caller's context.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde_json::{Map, Value};

pub(super) fn fields(
    ctx: &DecodeContext<'_>,
    source: &Map<String, Value>,
) -> Result<Map<String, Value>, CodecError> {
    let _depth = ctx.enter_nested("copy native field object")?;
    ctx.charge_work(1, "copy native field object")?;
    let mut copied = Map::new();
    for (key, value) in source {
        let key = ctx.copy_retained_text(key, "copy native field name")?;
        let value = copy_value(ctx, value)?;
        insert(ctx, &mut copied, key, value)?;
    }
    Ok(copied)
}

pub(super) fn insert(
    ctx: &DecodeContext<'_>,
    fields: &mut Map<String, Value>,
    key: String,
    value: Value,
) -> Result<(), CodecError> {
    // Map keeps its B-tree private. One entry search visits its search path.
    let len = fields.len();
    let (_, comparisons) = super::map_search_bound(len);
    let work = u64_from_index(key.len())
        .checked_mul(comparisons)
        .ok_or_else(|| {
            ctx.refuse_codec_limit("insert copied native field", u64::MAX - 1, u64::MAX)
        })?;
    ctx.charge_work(work, "insert copied native field")?;
    match fields.entry(key) {
        serde_json::map::Entry::Vacant(entry) => {
            ctx.admit_btree_node_storage::<String, Value>(len, "insert copied native field")?;
            ctx.charge_collection_items(1, "insert copied native field")?;
            entry.insert(value);
        }
        serde_json::map::Entry::Occupied(mut entry) => {
            entry.insert(value);
        }
    }
    Ok(())
}

fn copy_value(ctx: &DecodeContext<'_>, source: &Value) -> Result<Value, CodecError> {
    ctx.charge_work(1, "copy native field value")?;
    Ok(match source {
        Value::Null => Value::Null,
        Value::Bool(value) => Value::Bool(*value),
        Value::Number(value) => Value::Number(value.clone()),
        Value::String(value) => {
            Value::String(ctx.copy_retained_text(value, "copy native field text")?)
        }
        Value::Array(values) => {
            let _depth = ctx.enter_nested("copy native field sequence")?;
            Value::Array(ctx.try_collect_retained_with(
                values,
                "copy native field sequence",
                |value| copy_value(ctx, value),
            )?)
        }
        Value::Object(values) => Value::Object(fields(ctx, values)?),
    })
}
