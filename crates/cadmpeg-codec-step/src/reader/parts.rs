// SPDX-License-Identifier: Apache-2.0
//! Source-addressed parser parts composed with the existing semantic reader.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;

use super::Packaging;

pub(crate) fn decode(
    source: View<'_>,
    ctx: &DecodeContext<'_>,
    packaging: Packaging,
) -> Result<Decoded, CodecError> {
    let (exchange, diagnostics, parts) = crate::parse::parts::parse(source, ctx)?;
    let mut decoded =
        super::decode_exchange(source.window(), exchange, &diagnostics, ctx, packaging)?;
    // The parser stage builds transient RawRecords, not a second native AST or
    // neutral projection. Its only document records are the shared helper stubs.
    let (model, mut native, fidelity, mut losses) = parts.into_parts(ctx)?.into_parts();
    if model.entity_count() != 0 {
        return Err(CodecError::malformed(
            "STEP parser stage unexpectedly built neutral records",
        ));
    }
    let namespace = native.namespace_mut("step");
    let destination = decoded.ir.native.namespace_mut("step").arenas_mut();
    for (name, mut records) in std::mem::take(namespace.arenas_mut()) {
        let destination = destination.entry(name).or_default();
        let additional = u64_from_index(
            records
                .len()
                .checked_mul(size_of::<cadmpeg_ir::native::NativeRecord>())
                .ok_or_else(|| CodecError::malformed("STEP part arena size overflows"))?,
        );
        ctx.retain_bytes(additional, "STEP shared part arena composition")?;
        destination.try_reserve(records.len()).map_err(|_| {
            ResourceLimit::allocation_failed(
                ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                additional,
                "STEP shared part arena composition",
            )
        })?;
        destination.append(&mut records);
    }
    decoded.source_fidelity.append(fidelity)?;
    decoded.body.losses.append(&mut losses);
    Ok(decoded)
}
