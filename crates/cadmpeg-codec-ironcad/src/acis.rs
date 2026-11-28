// SPDX-License-Identifier: Apache-2.0
//! ACIS kernel records of `Brep Data`.

use cadmpeg_asm::brep::{decode_with_header, AsmBrep, DecodePurpose};
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_asm::kernel_header::{BinaryHeader, RefWidth};
use cadmpeg_asm::sab;
use cadmpeg_asm::stream_error::StreamFailure;
use cadmpeg_asm::{acis_header, acis_interned};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_ir::unknown::part::Cause;

/// Identity format of the B-rep entities that ACIS records decode.
pub(crate) const ID_FORMAT: IdFormat = cadmpeg_asm::asm_format!("ironcad");

/// Why an ACIS record does not decode.
pub(crate) struct AcisFailure {
    pub(crate) cause: Cause,
    pub(crate) error: CodecError,
}

impl AcisFailure {
    fn unreadable(error: CodecError) -> Self {
        Self {
            cause: Cause::Unreadable,
            error,
        }
    }
}

/// One decoded ACIS record.
pub(crate) struct DecodedAcis {
    pub(crate) brep: AsmBrep,
    /// Records whose extended entity prefix was reduced to the shared prefix.
    pub(crate) reduced_records: usize,
    /// Offset of the bytes that the framed records leave unread.
    pub(crate) unread: Option<usize>,
}

/// Frames and decodes one ACIS binary save stream. A stream that does not
/// frame is unreadable; a framed stream the B-rep decode refuses is
/// undecoded.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    bytes: View<'_>,
    header: &BinaryHeader,
    stream: &str,
) -> Result<DecodedAcis, AcisFailure> {
    let bytes = bytes.window();
    let start = acis_header::record_stream_start_with_header(bytes, header).ok_or_else(|| {
        AcisFailure::unreadable(CodecError::malformed("ACIS record has no record stream"))
    })?;
    let entity_count = header.metadata.entity_count;
    let framing_error = |failure: StreamFailure| {
        failure.into_codec_error(ctx, |error| {
            CodecError::malformed(format_args!("ACIS SAB framing failed: {error}"))
        })
    };
    let (records, reduced_records, unread) = if acis_interned::applies(bytes, start) {
        let framed = acis_interned::frame(ctx, bytes, start, bytes.len(), entity_count)
            .map_err(framing_error)
            .map_err(AcisFailure::unreadable)?;
        (framed.records, framed.reduced_records, framed.unread)
    } else {
        let records = match acis_header::solved_record_limit_with_header(bytes, header) {
            Some(limit) => sab::frame(ctx, bytes, start, limit, RefWidth::Four, entity_count),
            None => {
                sab::frame_history(ctx, bytes, start, bytes.len(), RefWidth::Four, entity_count)
            }
        }
        .map_err(framing_error)
        .map_err(AcisFailure::unreadable)?;
        (records, 0, None)
    };
    let brep = decode_with_header(
        ctx,
        &records,
        bytes,
        Some(&header.metadata),
        stream,
        ID_FORMAT,
        DecodePurpose::Model,
    )
    .map_err(|error| AcisFailure {
        cause: Cause::Undecoded,
        error,
    })?;
    Ok(DecodedAcis {
        brep,
        reduced_records,
        unread,
    })
}

/// Moves every identity of `brep` under `namespace`, so that the graphs of two
/// records stay disjoint.
pub(crate) fn qualify(
    ctx: &DecodeContext<'_>,
    brep: AsmBrep,
    namespace: &str,
) -> Result<AsmBrep, CodecError> {
    let prefix = format!("{ID_FORMAT}:");
    let mut map = IdentityMap::new(|source: &str| {
        let suffix = source.strip_prefix(prefix.as_str()).unwrap_or(source);
        Ok(format!("{ID_FORMAT}:brep/{namespace}/{suffix}"))
    });
    let rewritten = brep.rewrite_identities(ctx, &mut map);
    map.finish()?;
    rewritten
}
