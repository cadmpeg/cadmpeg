// SPDX-License-Identifier: Apache-2.0
//! Content detection for eDrawings packages.
//!
//! A ZIP prefix whose first local member is `eModel`, and whose bytes open
//! with the HSF comment `;; HSF V`, is [`Confidence::High`]. A ZIP that names
//! an `eModel` member any other way is [`Confidence::Medium`]. Any other
//! input is [`Confidence::No`]: a ZIP without an `eModel` member is not an
//! eDrawings package, so the codec stays out of the tie among generic ZIPs.
//! The file extension never takes part.

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{index_from_u32, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Confidence;

use crate::package::{is_model_name, MODEL_MEMBER};
use crate::stream::COMMENT_PREFIX;

/// ZIP local file header signature.
const LOCAL_HEADER_MAGIC: &[u8; 4] = b"PK\x03\x04";
/// Length of the fixed part of a ZIP local file header.
const LOCAL_HEADER_LEN: usize = 30;
/// Local-header offset of the compression method.
const LOCAL_METHOD: usize = 8;
/// Local-header offset of the compressed size.
const LOCAL_COMPRESSED_SIZE: usize = 18;
/// Local-header offset of the name length.
const LOCAL_NAME_LEN: usize = 26;
/// Local-header offset of the extra-field length.
const LOCAL_EXTRA_LEN: usize = 28;
/// ZIP compression method of a stored member.
const METHOD_STORED: u16 = 0;
/// ZIP compression method of a raw-DEFLATE member.
const METHOD_DEFLATE: u16 = 8;
/// Compressed bytes of a DEFLATE member that the probe expands. The comment
/// line sits at the front of the expanded bytes, so a short head decides.
const DETECTION_COMPRESSED_BYTES: usize = 1024;
/// Expanded bytes the probe accepts from that head.
const DETECTION_EXPANDED_BYTES: usize = 64 * 1024;

/// How strongly a byte prefix names an eDrawings package.
pub(crate) fn confidence(ctx: &DecodeContext<'_>, prefix: &[u8]) -> Result<Confidence, CodecError> {
    if !prefix.starts_with(LOCAL_HEADER_MAGIC) {
        if ArchiveSnapshot::has_footer(prefix) {
            match ArchiveSnapshot::contains_matching_name(
                ctx,
                View::over_retained(prefix),
                is_model_name,
            ) {
                Ok(true) => return Ok(Confidence::Medium),
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Ok(false) | Err(_) => {}
            }
        }
        return Ok(Confidence::No);
    }
    match leading_member(ctx, prefix)? {
        Leading::SignedModel => Ok(Confidence::High),
        Leading::Model => Ok(Confidence::Medium),
        Leading::Other if contains(prefix, MODEL_MEMBER.as_bytes()) => Ok(Confidence::Medium),
        Leading::Other => Ok(Confidence::No),
    }
}

/// What the first local member of a package is.
enum Leading {
    /// The `eModel` member, and it opens with the HSF comment.
    SignedModel,
    /// The `eModel` member, and its opening bytes are unconfirmed.
    Model,
    /// Another member, or no readable local header.
    Other,
}

fn leading_member(ctx: &DecodeContext<'_>, prefix: &[u8]) -> Result<Leading, CodecError> {
    let (Some(method), Some(compressed), Some(name_len), Some(extra_len)) = (
        View::u16_le_at(prefix, LOCAL_METHOD),
        View::u32_le_at(prefix, LOCAL_COMPRESSED_SIZE),
        View::u16_le_at(prefix, LOCAL_NAME_LEN),
        View::u16_le_at(prefix, LOCAL_EXTRA_LEN),
    ) else {
        return Ok(Leading::Other);
    };
    let Some(name_end) = LOCAL_HEADER_LEN.checked_add(usize::from(name_len)) else {
        return Ok(Leading::Other);
    };
    let Some(data_start) = name_end.checked_add(usize::from(extra_len)) else {
        return Ok(Leading::Other);
    };
    let Some(name) = prefix
        .get(LOCAL_HEADER_LEN..name_end)
        .and_then(|name| std::str::from_utf8(name).ok())
    else {
        return Ok(Leading::Other);
    };
    if !is_model_name(name) {
        return Ok(Leading::Other);
    }
    let Some(rest) = prefix.get(data_start..) else {
        return Ok(Leading::Model);
    };
    let declared = index_from_u32(compressed);
    let payload = match declared {
        0 => rest,
        _ => rest.get(..declared).unwrap_or(rest),
    };
    let signed = match method {
        METHOD_STORED => payload.starts_with(COMMENT_PREFIX),
        METHOD_DEFLATE => {
            let head = payload.get(..DETECTION_COMPRESSED_BYTES).unwrap_or(payload);
            ctx.inflate_probe(View::over_retained(head), DETECTION_EXPANDED_BYTES, false)?
                .is_some_and(|expanded| expanded.starts_with(COMMENT_PREFIX))
        }
        _ => false,
    };
    Ok(if signed {
        Leading::SignedModel
    } else {
        Leading::Model
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::codec::{Codec, Confidence};

    use crate::test_support::{
        assembly_package, deflated_part_package, emodel, part_package, zip_of,
    };
    use crate::EdrawingsCodec;

    fn detect(bytes: &[u8]) -> Confidence {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
        let confidence = EdrawingsCodec.detect(&ctx, root).unwrap();
        ctx.finish_session().unwrap();
        confidence
    }

    #[test]
    fn a_leading_signed_model_member_is_high() {
        assert_eq!(detect(&part_package(b"body")), Confidence::High);
        assert_eq!(detect(&assembly_package(b"body")), Confidence::High);
        assert_eq!(detect(&deflated_part_package(b"body")), Confidence::High);
    }

    #[test]
    fn a_leading_model_member_without_the_comment_is_medium() {
        let bytes = zip_of(&[("eModel", b"not an HSF stream"), ("scene.xml", b"<a/>")]);
        assert_eq!(detect(&bytes), Confidence::Medium);
    }

    #[test]
    fn a_model_member_after_the_first_member_is_medium() {
        let bytes = zip_of(&[("scene.xml", b"<a/>"), ("eModel", &emodel(b"body"))]);
        assert_eq!(detect(&bytes), Confidence::Medium);
    }

    #[test]
    fn a_zip_without_a_model_member_is_no() {
        let bytes = zip_of(&[("model.ol", b"geometry"), ("readme.txt", b"text")]);
        assert_eq!(detect(&bytes), Confidence::No);
    }

    #[test]
    fn bytes_that_are_not_a_zip_are_no() {
        assert_eq!(detect(&emodel(b"body")), Confidence::No);
        assert_eq!(detect(b""), Confidence::No);
        assert_eq!(detect(b"PK\x03\x04"), Confidence::No);
    }

    #[test]
    fn a_truncated_package_is_still_decided_by_its_leading_member() {
        let bytes = part_package(b"body");
        assert_eq!(detect(&bytes[..60]), Confidence::High);
        assert_eq!(detect(&bytes[..20]), Confidence::No);
    }
}
