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

use cadmpeg_core::decode::{DecodeArena, DecodeContext};
use cadmpeg_ir::codec::{CodecBackend, DecodeFailure, DecodeOptions, DecodeResult};

/// Decode a retained source archive for a writer that reads only its IR and
/// losses, so the decode records no content digests.
pub(crate) fn decode_retained_source(
    image: &[u8],
    policy: cadmpeg_core::decode::DecodePolicy,
) -> Result<DecodeResult, DecodeFailure> {
    let options = DecodeOptions {
        policy,
        ..DecodeOptions::default()
    };
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::read_root(
        &mut std::io::Cursor::new(image),
        &arena,
        &options.policy,
        options.depth,
    )?;
    let result = crate::decode::decode(&ctx, root)
        .map_err(DecodeFailure::from)
        .and_then(|decoded| {
            DecodeResult::from_decoded_without_digests(
                &ctx,
                decoded,
                <crate::F3dCodec as CodecBackend>::FORMAT,
                &options.strictness,
            )
        });
    ctx.finish_session()?;
    result
}
