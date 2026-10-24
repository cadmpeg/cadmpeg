// SPDX-License-Identifier: Apache-2.0
//! One GZIP member ([RFC 1952](https://www.rfc-editor.org/rfc/rfc1952)): the
//! encoding of every PVS stream payload (§3.3) and every OL stream payload
//! (§4.2).
//!
//! The member header and trailer are read here. The DEFLATE body is expanded
//! by the shared raw-DEFLATE expander into a new decode space under the
//! session's decompression caps.

use cadmpeg_container::compression::inflate_deflate;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;

/// The two identification bytes that open a GZIP member.
const MAGIC: [u8; 2] = [0x1f, 0x8b];
/// The one compression method RFC 1952 defines: DEFLATE.
const METHOD_DEFLATE: u8 = 8;
/// Header flag: a two-byte header CRC follows the optional fields.
const FLAG_HEADER_CRC: u8 = 0x02;
/// Header flag: a length-prefixed extra field follows the fixed header.
const FLAG_EXTRA: u8 = 0x04;
/// Header flag: a NUL-terminated original file name follows.
const FLAG_NAME: u8 = 0x08;
/// Header flag: a NUL-terminated comment follows.
const FLAG_COMMENT: u8 = 0x10;
/// Header flag bits RFC 1952 reserves; a conforming member clears them.
const FLAG_RESERVED: u8 = 0xe0;
/// Bytes of `MTIME`, `XFL`, and `OS` after the flag byte.
const FIXED_TAIL_LEN: usize = 6;
/// Bytes of the trailer: `CRC32` and `ISIZE`.
const TRAILER_LEN: usize = 8;
/// `ISIZE` holds the expanded length modulo 2^32.
const ISIZE_MODULUS: u64 = 1 << 32;

/// Expands the one GZIP member that fills `member` exactly.
pub(crate) fn expand<'a>(
    ctx: &DecodeContext<'a>,
    member: View<'_>,
) -> Result<View<'a>, CodecError> {
    let mut header = member;
    if header.req_take(MAGIC.len())? != MAGIC {
        return Err(CodecError::malformed(format_args!(
            "GZIP member at offset {} does not open with 1f 8b",
            member.start()
        )));
    }
    let method = header.req_u8()?;
    if method != METHOD_DEFLATE {
        return Err(CodecError::malformed(format_args!(
            "GZIP member at offset {} names compression method {method}, not DEFLATE",
            member.start()
        )));
    }
    let flags = header.req_u8()?;
    if flags & FLAG_RESERVED != 0 {
        return Err(CodecError::malformed(format_args!(
            "GZIP member at offset {} sets reserved flag bits {flags:#04x}",
            member.start()
        )));
    }
    header.req_take(FIXED_TAIL_LEN)?;
    if flags & FLAG_EXTRA != 0 {
        let extra = header.req_u16_le()?;
        header.req_take(usize::from(extra))?;
    }
    for flag in [FLAG_NAME, FLAG_COMMENT] {
        if flags & flag != 0 {
            while header.req_u8()? != 0 {}
        }
    }
    if flags & FLAG_HEADER_CRC != 0 {
        header.req_u16_le()?;
    }
    let body_end = member
        .end()
        .checked_sub(TRAILER_LEN)
        .filter(|end| *end >= header.position())
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "GZIP member at offset {} has no room for its trailer",
                member.start()
            ))
        })?;
    let (Some(body), Some(mut trailer)) = (
        member.child(header.position(), body_end),
        member.child(body_end, member.end()),
    ) else {
        return Err(CodecError::malformed(format_args!(
            "GZIP member at offset {} has an unbounded body",
            member.start()
        )));
    };
    let crc = trailer.req_u32_le()?;
    let size = trailer.req_u32_le()?;
    let expanded = inflate_deflate(ctx, body, ExpandSpec::Unknown)?;
    let window = expanded.window();
    if u64_from_index(window.len()) % ISIZE_MODULUS != u64::from(size) {
        return Err(CodecError::malformed(format_args!(
            "GZIP member at offset {} expands to {} bytes; its trailer states {size} modulo 2^32",
            member.start(),
            window.len()
        )));
    }
    if crc32fast::hash(window) != crc {
        return Err(CodecError::malformed(format_args!(
            "GZIP member at offset {} fails its CRC-32",
            member.start()
        )));
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::View;
    use cadmpeg_core::CodecError;

    use super::expand;
    use crate::test_support::gzip;

    fn expanded(bytes: &[u8]) -> Result<Vec<u8>, CodecError> {
        let ctx = cadmpeg_test_support::service_decode_context();
        expand(&ctx, View::over_retained(bytes)).map(|view| view.window().to_vec())
    }

    #[test]
    fn a_member_expands_to_its_plain_bytes() {
        assert_eq!(expanded(&gzip(b"structure")).unwrap(), b"structure");
        assert_eq!(expanded(&gzip(b"")).unwrap(), b"");
    }

    #[test]
    fn optional_header_fields_are_skipped() {
        let plain = gzip(b"payload");
        let mut member = plain[..3].to_vec();
        member.push(0x02 | 0x04 | 0x08 | 0x10);
        member.extend_from_slice(&plain[4..10]);
        member.extend_from_slice(&[3, 0, b'x', b'y', b'z']);
        member.extend_from_slice(b"name\0comment\0");
        member.extend_from_slice(&[0xaa, 0xbb]);
        member.extend_from_slice(&plain[10..]);
        assert_eq!(expanded(&member).unwrap(), b"payload");
    }

    #[test]
    fn a_damaged_trailer_or_header_is_malformed() {
        let member = gzip(b"payload");
        let last = member.len() - 1;

        let mut wrong_crc = member.clone();
        wrong_crc[last - 4] ^= 1;
        let mut wrong_size = member.clone();
        wrong_size[last] ^= 1;
        let mut wrong_magic = member.clone();
        wrong_magic[1] = 0;
        let mut wrong_method = member.clone();
        wrong_method[2] = 7;
        let mut reserved = member.clone();
        reserved[3] = 0x80;
        let mut trailing = member.clone();
        trailing.push(0);

        for bad in [
            wrong_crc,
            wrong_size,
            wrong_magic,
            wrong_method,
            reserved,
            trailing,
            member[..12].to_vec(),
        ] {
            assert!(
                matches!(
                    expanded(&bad),
                    Err(CodecError::Malformed(_) | CodecError::Truncated { .. })
                ),
                "{bad:02x?}"
            );
        }
    }
}
