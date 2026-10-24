// SPDX-License-Identifier: Apache-2.0
//! Content detection for `.pvz` packages.
//!
//! A ZIP prefix whose first local member is a structure member that declares
//! a known generation, in the binary or the XML form, is [`Confidence::High`]. A ZIP that names a structure member
//! elsewhere is [`Confidence::Medium`], and any other ZIP is
//! [`Confidence::Low`], as for every ZIP-based format.

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{index_from_u32, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Confidence;

use crate::package::{is_structure_name, STRUCTURE_EXTENSION};

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
/// Expanded bytes the probe inflates from a leading structure member.
const DETECTION_STRUCTURE_BYTES: usize = 64 * 1024;

/// How strongly a byte prefix names a `.pvz` package.
pub(crate) fn confidence(ctx: &DecodeContext<'_>, prefix: &[u8]) -> Result<Confidence, CodecError> {
    if !prefix.starts_with(LOCAL_HEADER_MAGIC) {
        if ArchiveSnapshot::has_footer(prefix) {
            match ArchiveSnapshot::contains_matching_name(
                ctx,
                View::over_retained(prefix),
                is_structure_name,
            ) {
                Ok(true) => return Ok(Confidence::Medium),
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Ok(false) | Err(_) => {}
            }
        }
        return Ok(Confidence::No);
    }
    if leading_member_is_signed_structure(ctx, prefix)? {
        Ok(Confidence::High)
    } else if contains(prefix, STRUCTURE_EXTENSION.as_bytes()) {
        Ok(Confidence::Medium)
    } else {
        Ok(Confidence::Low)
    }
}

/// Whether the first local member is a structure member whose expanded
/// bytes declare a known generation.
fn leading_member_is_signed_structure(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
) -> Result<bool, CodecError> {
    let (Some(method), Some(compressed), Some(name_len), Some(extra_len)) = (
        View::u16_le_at(prefix, LOCAL_METHOD),
        View::u32_le_at(prefix, LOCAL_COMPRESSED_SIZE),
        View::u16_le_at(prefix, LOCAL_NAME_LEN),
        View::u16_le_at(prefix, LOCAL_EXTRA_LEN),
    ) else {
        return Ok(false);
    };
    let Some(name_end) = LOCAL_HEADER_LEN.checked_add(usize::from(name_len)) else {
        return Ok(false);
    };
    let Some(data_start) = name_end.checked_add(usize::from(extra_len)) else {
        return Ok(false);
    };
    let Some(name) = prefix
        .get(LOCAL_HEADER_LEN..name_end)
        .and_then(|name| std::str::from_utf8(name).ok())
    else {
        return Ok(false);
    };
    if !is_structure_name(name) {
        return Ok(false);
    }
    let Some(rest) = prefix.get(data_start..) else {
        return Ok(false);
    };
    let declared = index_from_u32(compressed);
    let payload = match declared {
        0 => rest,
        _ => rest.get(..declared).unwrap_or(rest),
    };
    let declared = match method {
        METHOD_STORED => crate::pvs::declared(View::over_retained(payload)),
        METHOD_DEFLATE => {
            let Some(expanded) = ctx.inflate_probe(
                View::over_retained(payload),
                DETECTION_STRUCTURE_BYTES,
                false,
            )?
            else {
                return Ok(false);
            };
            crate::pvs::declared(View::over_retained(&expanded))
        }
        _ => None,
    };
    Ok(declared.is_some())
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::codec::{Codec, Confidence};
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};

    use crate::test_support::{deflate_stored_block, package, OL_09, PVS_0304};
    use crate::PvzCodec;

    fn detect(bytes: &[u8]) -> Confidence {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
        let confidence = PvzCodec.detect(&ctx, root).unwrap();
        ctx.finish_session().unwrap();
        confidence
    }

    #[test]
    fn a_leading_signed_structure_member_is_high() {
        let stored = package(&[("model.pvs", PVS_0304), ("model.ol", OL_09)]);
        assert_eq!(detect(&stored), Confidence::High);

        let deflated = archive(&[
            RawEntry::compressed(
                "model.pvs",
                cadmpeg_test_support::zip_fixture::DEFLATE,
                &deflate_stored_block(PVS_0304),
                PVS_0304,
            ),
            RawEntry::stored("model.ol", OL_09),
        ]);
        assert_eq!(detect(&deflated), Confidence::High);
    }

    #[test]
    fn a_leading_xml_structure_member_with_a_known_version_is_high() {
        let xml: &[u8] = br#"<PV_FILE type="PVS" version="0302"><section_structure/></PV_FILE>"#;
        let bytes = package(&[("model.pvs", xml), ("model.ol", OL_09)]);
        assert_eq!(detect(&bytes), Confidence::High);

        let unknown: &[u8] = br#"<PV_FILE type="PVS" version="0399"></PV_FILE>"#;
        let bytes = package(&[("model.pvs", unknown), ("model.ol", OL_09)]);
        assert_eq!(detect(&bytes), Confidence::Medium);
    }

    #[test]
    fn a_structure_member_after_the_first_member_is_medium() {
        let bytes = package(&[("model.ol", OL_09), ("model.pvs", PVS_0304)]);
        assert_eq!(detect(&bytes), Confidence::Medium);
    }

    #[test]
    fn a_leading_structure_member_without_a_known_signature_is_medium() {
        let bytes = package(&[("model.pvs", b"not a signature"), ("model.ol", OL_09)]);
        assert_eq!(detect(&bytes), Confidence::Medium);
    }

    #[test]
    fn a_zip_without_a_structure_member_is_low() {
        let bytes = package(&[("model.ol", OL_09), ("readme.txt", b"text")]);
        assert_eq!(detect(&bytes), Confidence::Low);
    }

    #[test]
    fn bytes_that_are_not_a_zip_are_no() {
        assert_eq!(detect(PVS_0304), Confidence::No);
        assert_eq!(detect(b""), Confidence::No);
    }
}
