// SPDX-License-Identifier: Apache-2.0
//! Encode source-less F3D archives and apply supported edits to retained source
//! archives.

pub(crate) mod generate;
pub(crate) mod patch;
pub(crate) mod primitives;
pub(crate) mod saved;
pub(crate) mod target;

/// Every text field this writer fits to its width (cadir-v11.md §3.4).
#[cfg(test)]
pub(crate) const CAPACITY_FIELDS: &[cadmpeg_ir::codec::write::capacity::TextCapacity] = &[
    generate::configuration_root::TABLE_ID,
    generate::configuration_root::ROW_ID,
];

#[cfg(test)]
mod tests;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, View};
use cadmpeg_ir::codec::{CodecBackend, DecodeFailure, DecodeResult};
use cadmpeg_ir::report::strictness::Strictness;

/// Decode a retained source archive, opened as input of the session of
/// `ctx`, for a writer that reads only its IR and losses, so the decode
/// records no content digests.
pub(crate) fn decode_retained_source(
    ctx: &DecodeContext<'_>,
    image: View<'_>,
) -> Result<DecodeResult, DecodeFailure> {
    let arena = DecodeArena::new();
    ctx.with_arena(&arena, |ctx| {
        let decoded = crate::decode::decode(ctx, image).map_err(DecodeFailure::from)?;
        DecodeResult::from_decoded_without_digests(
            ctx,
            decoded,
            <crate::F3dCodec as CodecBackend>::FORMAT,
            &Strictness::default(),
        )
    })
}
