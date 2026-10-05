// SPDX-License-Identifier: Apache-2.0
//! Charged stream-scope tests on native record identifiers.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
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

/// Whether native record `id` belongs to `stream`: whether `id` is `stream`,
/// a `:`, and a suffix without `:`. The prefix comparison and the suffix
/// search together read each byte of `id` at most once.
pub(in crate::design::decode) fn in_stream(
    ctx: &DecodeContext<'_>,
    id: &str,
    stream: &str,
) -> Result<bool, CodecError> {
    let Some((scope, rest)) = id.as_bytes().split_at_checked(stream.len()) else {
        return Ok(false);
    };
    let Some((&b':', suffix)) = rest.split_first() else {
        return Ok(false);
    };
    Ok(
        ctx.equal_bytes(scope, stream.as_bytes(), "match F3D record stream scope")?
            && !ctx.any_by(
                suffix,
                |byte| Ok(*byte == b':'),
                "find F3D record stream scope",
            )?,
    )
}

/// Whether native record `id` has the stream scope `stream`, where `None` is
/// the scope of an ID without `:`. Each byte of `id` is read at most once.
pub(in crate::design::decode) fn has_stream(
    ctx: &DecodeContext<'_>,
    id: &str,
    stream: Option<&str>,
) -> Result<bool, CodecError> {
    match stream {
        Some(stream) => in_stream(ctx, id, stream),
        None => Ok(!ctx.any_by(
            id.as_bytes(),
            |byte| Ok(*byte == b':'),
            "find F3D record stream scope",
        )?),
    }
}

/// Record offsets keyed by stream scope, in scope and offset order, with the
/// scoped reservation that holds them.
pub(in crate::design::decode) type StreamOffsets<'records, 'ctx> =
    (Vec<(&'records str, u64)>, ScopedReservation<'ctx>);

#[cfg(test)]
mod tests {
    #[test]
    fn in_stream_matches_the_scope_before_the_last_colon() {
        let ctx = cadmpeg_test_support::service_decode_context();
        for (id, stream, expected) in [
            ("a:b", "a", true),
            ("a:b:c", "a:b", true),
            ("a:b:c", "a", false),
            ("ab:c", "a", false),
            (":x", "", true),
            ("a:", "a", true),
            ("a", "a", false),
            ("b:c", "a", false),
        ] {
            assert_eq!(
                super::in_stream(&ctx, id, stream).unwrap(),
                expected,
                "{id} in {stream}"
            );
            assert_eq!(
                super::record_stream(&ctx, id).unwrap() == Some(stream),
                expected
            );
        }
    }

    #[test]
    fn has_stream_matches_scopes_including_ids_without_one() {
        let ctx = cadmpeg_test_support::service_decode_context();
        for (id, other) in [("a:b", "c:d"), ("a:b", "a:c"), ("ab", "cd"), ("ab", "a:b")] {
            let scope = super::record_stream(&ctx, other).unwrap();
            assert_eq!(
                super::has_stream(&ctx, id, scope).unwrap(),
                super::record_stream(&ctx, id).unwrap() == scope,
                "{id} beside {other}"
            );
        }
    }
}
