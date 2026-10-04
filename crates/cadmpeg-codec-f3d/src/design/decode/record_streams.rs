// SPDX-License-Identifier: Apache-2.0
//! Charged stream-scope tests on native record identifiers.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// The stream scope of a native record ID: the text before its last `:`.
pub(in crate::design::decode) fn record_stream<'id>(
    ctx: &DecodeContext<'_>,
    id: &'id str,
) -> Result<Option<&'id str>, CodecError> {
    Ok(crate::design::decode::text::rsplit_once_ascii(
        ctx,
        id,
        b':',
        "find F3D record stream scope",
    )?
    .map(|(stream, _)| stream))
}

/// Whether native record `id` belongs to `stream`. Scopes of different length
/// differ without a byte comparison.
pub(in crate::design::decode) fn in_stream(
    ctx: &DecodeContext<'_>,
    id: &str,
    stream: &str,
) -> Result<bool, CodecError> {
    let Some(id_stream) = record_stream(ctx, id)? else {
        return Ok(false);
    };
    ctx.equal_bytes(
        id_stream.as_bytes(),
        stream.as_bytes(),
        "match F3D record stream scope",
    )
}
