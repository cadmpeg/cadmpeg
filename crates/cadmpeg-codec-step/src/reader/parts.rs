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
    let analyzed = super::decode_exchange(input, exchange, diagnostics, ctx, packaging)?;
    let mut decoded = analyzed.decoded;
    let (model, mut native, fidelity, mut losses) = parts.into_parts();
    // STEP parse callbacks return their records without staging neutral
    // content; only the native stubs, fidelity and losses leave the parts.
    // The part output holds only this codec's namespace.
    debug_assert!(model.entity_count() == 0);
    debug_assert!(native
        .loss_counts()
        .iter()
        .all(|count| count.format == "step"));
    let staged = std::mem::take(native.namespace_mut("step").arenas_mut());
    for (arena, mut records) in staged {
        let existing = decoded
            .ir
            .native
            .namespace_mut("step")
            .arenas_mut()
            .entry(arena)
            .or_default();
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
    decoded.source_fidelity.append(fidelity)?;
    decoded.body.losses.append(&mut losses);
    // The reader names each given-up part's drops after the stages run; the
    // part losses arrive here, so the amendment lands on the merged report.
    super::drops::amend_record_unreadable(
        &mut decoded.body.losses,
        &analyzed.damage_ledger,
        &analyzed.damage_starts,
    );
    Ok(decoded)
}
