// SPDX-License-Identifier: Apache-2.0
//! Source-addressed parser parts composed with the existing semantic reader.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;

use super::Packaging;

pub(crate) fn decode(
    source: View<'_>,
    ctx: &DecodeContext<'_>,
    packaging: Packaging,
    stream: Option<cadmpeg_ir::StreamName>,
) -> Result<Decoded, CodecError> {
    let (exchange, diagnostics, parts) = crate::parse::parts::parse(source, ctx, stream)?;
    decode_parsed(
        source.window(),
        exchange,
        &diagnostics,
        parts,
        ctx,
        packaging,
    )
}

pub(crate) fn decode_parsed(
    input: &[u8],
    exchange: crate::parse::Exchange,
    diagnostics: &[crate::parse::ParseDiagnostic],
    parts: cadmpeg_ir::source_fidelity::parts::PartOutput,
    ctx: &DecodeContext<'_>,
    packaging: Packaging,
) -> Result<Decoded, CodecError> {
    let mut decoded = super::decode_exchange(input, exchange, diagnostics, ctx, packaging)?;
    let (model, native, fidelity, mut losses) = parts.into_parts();
    // STEP parse callbacks return their records without staging neutral
    // content; only the native stubs, fidelity and losses leave the parts.
    debug_assert!(model.entity_count() == 0);
    for (format, mut namespace) in native.0 {
        let destination = decoded.ir.native.namespace_mut(format);
        for (arena, mut records) in std::mem::take(namespace.arenas_mut()) {
            let existing = destination.arenas_mut().entry(arena).or_default();
            if records
                .iter()
                .any(|record| existing.iter().any(|known| known.id() == record.id()))
            {
                return Err(CodecError::malformed(
                    "given-up part stub duplicates a native record",
                ));
            }
            existing.append(&mut records);
        }
    }
    decoded.source_fidelity.append(fidelity)?;
    decoded.body.losses.append(&mut losses);
    Ok(decoded)
}
