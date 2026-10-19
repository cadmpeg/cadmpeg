// SPDX-License-Identifier: Apache-2.0
//! Source-only evidence that cannot count as typed document content.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::CadIr;
use serde_json::Value;

pub(crate) fn untyped_records(
    ir: &CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<String>, CodecError> {
    let Some(namespace) = ir.native.namespace("iges") else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for (arena, records) in namespace.arenas() {
        for record in records {
            let fields = record.copy_fields(ctx)?;
            let mut pending: Vec<_> = fields.values().collect();
            let mut untyped = arena == "macro_definitions";
            while let Some(value) = pending.pop() {
                match value {
                    Value::Object(fields) => {
                        if fields.get("diagnostic").and_then(Value::as_str)
                            == Some("native text is not UTF-8")
                            || fields.get("kind").and_then(Value::as_str) == Some("unreadable")
                        {
                            untyped = true;
                        }
                        pending.extend(fields.values());
                    }
                    Value::Array(items) => pending.extend(items),
                    _ => {}
                }
            }
            if untyped {
                result.push(record.id().to_owned());
            }
        }
    }
    Ok(result)
}
