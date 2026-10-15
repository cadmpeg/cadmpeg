// SPDX-License-Identifier: Apache-2.0
//! Copies native values under the caller's depth account.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde_json::{Map, Value};

pub(super) fn fields(
    ctx: &DecodeContext<'_>,
    source: &Map<String, Value>,
) -> Result<Map<String, Value>, CodecError> {
    let _depth = ctx.enter_nested("copy native field object")?;
    let mut copied = Map::new();
    for (key, value) in source {
        copied.insert(key.to_owned(), copy_value(ctx, value)?);
    }
    Ok(copied)
}

fn copy_value(ctx: &DecodeContext<'_>, source: &Value) -> Result<Value, CodecError> {
    Ok(match source {
        Value::Array(values) => {
            let _depth = ctx.enter_nested("copy native field sequence")?;
            Value::Array(
                values
                    .iter()
                    .map(|value| copy_value(ctx, value))
                    .collect::<Result<_, _>>()?,
            )
        }
        Value::Object(values) => Value::Object(fields(ctx, values)?),
        value => value.clone(),
    })
}
