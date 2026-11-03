// SPDX-License-Identifier: Apache-2.0
//! Per-record omissions for native arenas the writer cannot frame.
//!
//! Texture mappings keep only their primitive class UUID; the custom
//! primitive's bytes stay in source fidelity and the `MappingCRCCache` is
//! recomputable cache state (`docs/formats/rhino_3dm.md` §7.2.19), so no
//! record can be re-emitted from held fields. Setting records keep only a
//! digest, a length and a typecode, and the encoder is synthesis-only, so
//! their bytes are unavailable too. Both stay `Dropped`; each record names
//! its own loss here in addition to the framework's grouped omission.

use super::{records, CadIr, Value};
use crate::loss::RhinoLossCode;
use cadmpeg_ir::report::loss::LossNote;

pub(in crate::writer) fn losses(ir: &CadIr) -> Vec<LossNote> {
    let mut out = Vec::new();
    for record in records(ir, "texture_mappings") {
        out.push(RhinoLossCode::WriterMaterialOmitted.note(format!(
            "texture mapping {}: the custom primitive bytes are not retained, only the primitive class UUID is admitted",
            record.id()
        )));
    }
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
