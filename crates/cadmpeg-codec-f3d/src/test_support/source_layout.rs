// SPDX-License-Identifier: Apache-2.0
//! Decode sidecars built from stated record layouts.

use cadmpeg_ir::source_fidelity::SourceFidelity;
use serde_json::{json, Map, Value};

/// A sidecar whose `record_layout` holds, for each `(id, offset, byte_len,
/// pointers)`, one capture in `stream` with that extent and those pointers.
pub(crate) fn sidecar(
    stream: &str,
    layouts: &[(&str, u64, u64, &[(&str, u64)])],
) -> SourceFidelity {
    let record_layout = layouts
        .iter()
        .map(|(id, offset, byte_len, pointers)| {
            (
                (*id).to_owned(),
                json!({
                    "stream": stream,
                    "offset": offset,
                    "byte_len": byte_len,
                    "reads": {},
                    "pointers": pointers
                        .iter()
                        .map(|(name, value)| ((*name).to_owned(), json!(value)))
                        .collect::<Map<String, Value>>(),
                    "payloads": {},
                }),
            )
        })
        .collect::<Map<String, Value>>();
    serde_json::from_value(json!({ "record_layout": record_layout })).expect("sidecar fixture")
}
