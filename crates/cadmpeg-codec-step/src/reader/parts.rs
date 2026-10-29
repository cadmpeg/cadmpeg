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
) -> Result<Decoded, CodecError> {
    let (exchange, diagnostics, parts) = crate::parse::parts::parse(source, ctx)?;
    let mut decoded =
        super::decode_exchange(source.window(), exchange, &diagnostics, ctx, packaging)?;
    let (_, _, fidelity, mut losses) = parts.into_parts();
    decoded.source_fidelity.append(fidelity)?;
    decoded.body.losses.append(&mut losses);
    Ok(decoded)
}
