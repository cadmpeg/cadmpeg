// SPDX-License-Identifier: Apache-2.0
//! Per-record omissions for native arenas the writer cannot frame.
//!
//! Setting records keep only a digest, a length and a typecode, and the
//! encoder is synthesis-only, so their bytes are unavailable. They stay
//! `Dropped`; each record names its own loss here in addition to the
//! framework's grouped omission. Texture mappings re-emit from their
//! retained primitive bytes (`tables::texture_mappings`).

use super::{records, CadIr, Value};
use crate::loss::RhinoLossCode;
use cadmpeg_ir::report::loss::LossNote;

pub(in crate::writer) fn losses(ir: &CadIr) -> Vec<LossNote> {
    let mut out = Vec::new();
    for record in records(ir, "setting_records") {
        let fields = record.field("typecode");
        let typecode = fields.as_ref().and_then(Value::as_str).unwrap_or("unknown");
        out.push(RhinoLossCode::WriterAttributesOmitted.note(format!(
            "setting record {} ({typecode}): the held record carries only a digest and length, and the synthesis-only encoder frames no source bytes",
            record.id()
        )));
    }
    out
}
