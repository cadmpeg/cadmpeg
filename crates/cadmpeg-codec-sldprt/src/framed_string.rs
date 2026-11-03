// SPDX-License-Identifier: Apache-2.0
//! Framed counted-`CString` reader.
//!
//! The framed string form is the marker `ff fe ff`, a short, `u16` or `u32`
//! count frame, and the counted UTF-16LE code units. A count byte below `ff`
//! is the unit count; `ff` selects a little-endian `u16` count, and `ffff`
//! selects a little-endian `u32` count. The reader bounds-checks `count * 2`
//! against the unread bytes before touching them, decodes UTF-16 directly
//! from the borrowed code units, and rejects unpaired surrogates.

use cadmpeg_core::decode::{bounded_len, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

/// One decoded framed string with the offset of its framing marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FramedString {
    /// The decoded text.
    pub(crate) value: String,
    /// The offset of the framing marker.
    pub(crate) offset: usize,
}

/// Marker opening a framed string.
const MARKER: [u8; 3] = [0xff, 0xfe, 0xff];
/// Count byte selecting the `u16` count frame.
const WIDE_COUNT: u8 = 0xff;
/// Count selecting the `u32` count frame.
const FAR_COUNT: u16 = 0xffff;

/// Read one framed string at the view position.
///
/// Returns `None` when the marker is absent, leaving the view at the entry
/// position. A present marker with a truncated frame or invalid code units is
/// a failed read: the view rolls back to the marker offset and the error
/// carries the read location.
pub(crate) fn read_framed_string(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
) -> Result<Option<FramedString>, CodecError> {
    let start = view.position();
    let Some(marker) = view.take(MARKER.len()) else {
        return Ok(None);
    };
    if marker != MARKER {
        if view.seek(start).is_none() {
            return Err(CodecError::malformed(
                "SLDPRT framed string rollback misses its entry position",
            ));
        }
        return Ok(None);
    }
    let read = read_after_marker(ctx, view, start);
    if read.is_err() && view.seek(start).is_none() {
        return Err(CodecError::malformed(
            "SLDPRT framed string rollback misses its entry position",
        ));
    }
    read.map(Some)
}

/// Read the count frame and code units after an accepted marker.
fn read_after_marker(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    start: usize,
) -> Result<FramedString, CodecError> {
    let first = view.req_u8()?;
    let count = if first == WIDE_COUNT {
        let second = view.req_u16_le()?;
        if second == FAR_COUNT {
            u64::from(view.req_u32_le()?)
        } else {
            u64::from(second)
        }
    } else {
        u64::from(first)
    };
    let units = bounded_len(count, 2, view.remaining()).ok_or_else(|| {
        CodecError::malformed("SLDPRT framed string count exceeds its unread bytes")
    })?;
    let byte_len = units.checked_mul(2).ok_or_else(|| {
        CodecError::malformed("SLDPRT framed string length overflows its byte window")
    })?;
    let raw = view.req_take(byte_len)?;
    let value = ctx.utf16le_text(raw, units, false, "decode SLDPRT framed string")?;
    ctx.retain_bytes(u64_from_index(value.len()), "retain SLDPRT framed string")?;
    Ok(FramedString {
        value,
        offset: start,
    })
}

#[cfg(test)]
mod tests {
    use super::{read_framed_string, FAR_COUNT, MARKER, WIDE_COUNT};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    fn decode(
        bytes: &[u8],
    ) -> (
        Result<Option<super::FramedString>, cadmpeg_core::CodecError>,
        usize,
    ) {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, mut view) =
            DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("root context");
        let result = read_framed_string(&ctx, &mut view);
        (result, view.position())
    }

    fn encode_units(units: &[u16]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    fn framed_short(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = MARKER.to_vec();
        bytes.push(u8::try_from(units.len()).expect("short fixture fits its frame"));
        bytes.extend(encode_units(&units));
        bytes
    }

    fn framed_wide(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = MARKER.to_vec();
        bytes.push(WIDE_COUNT);
        bytes.extend_from_slice(
            &u16::try_from(units.len())
                .expect("wide fixture fits its frame")
                .to_le_bytes(),
        );
        bytes.extend(encode_units(&units));
        bytes
    }

    fn framed_far(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = MARKER.to_vec();
        bytes.push(WIDE_COUNT);
        bytes.extend_from_slice(&FAR_COUNT.to_le_bytes());
        bytes.extend_from_slice(
            &u32::try_from(units.len())
                .expect("far fixture fits its frame")
                .to_le_bytes(),
        );
        bytes.extend(encode_units(&units));
        bytes
    }

    #[test]
    fn short_form_decodes_the_borrowed_code_units() {
        for text in ["", "A", "Hi!", "A\u{1F600}B"] {
            let bytes = framed_short(text);
            let (result, end) = decode(&bytes);
            let framed = result.expect("valid short frame").expect("marker present");
            assert_eq!(framed.value, text);
            assert_eq!(framed.offset, 0);
            assert_eq!(end, bytes.len());
        }
    }

    #[test]
    fn wide_form_accepts_boundary_and_large_counts() {
        for units in [0xff, 300] {
            let text: String = std::iter::repeat_n('x', units).collect();
            let bytes = framed_wide(&text);
            let (result, end) = decode(&bytes);
            let framed = result.expect("valid wide frame").expect("marker present");
            assert_eq!(framed.value, text);
            assert_eq!(framed.offset, 0);
            assert_eq!(end, bytes.len());
        }
    }

    #[test]
    fn far_form_accepts_the_u32_frame() {
        let bytes = framed_far("abc");
        let (result, end) = decode(&bytes);
        let framed = result.expect("valid far frame").expect("marker present");
        assert_eq!(framed.value, "abc");
        assert_eq!(framed.offset, 0);
        assert_eq!(end, bytes.len());
    }

    #[test]
    fn absent_marker_returns_none_without_moving() {
        for bytes in [vec![], vec![0xff, 0xfe], vec![0x00, 0x01, 0x02, 0x03]] {
            let (result, position) = decode(&bytes);
            assert!(result.expect("absent marker is not a failure").is_none());
            assert_eq!(position, 0);
        }
    }

    #[test]
    fn offset_names_the_marker_position() {
        let mut bytes = vec![0x11, 0x22, 0x33, 0x44, 0x55];
        bytes.extend(framed_short("key"));
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, mut view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root context");
        view.skip(5).expect("prefix length");
        let framed = read_framed_string(&ctx, &mut view)
            .expect("valid prefixed frame")
            .expect("marker present");
        assert_eq!(framed.value, "key");
        assert_eq!(framed.offset, 5);
        assert_eq!(view.position(), bytes.len());
    }

    #[test]
    fn truncated_frames_roll_back_to_the_marker() {
        let valid = framed_short("AB");
        let mut cases = vec![
            MARKER.to_vec(),
            vec![0xff, 0xfe, 0xff, WIDE_COUNT],
            vec![0xff, 0xfe, 0xff, WIDE_COUNT, 0x02],
            vec![0xff, 0xfe, 0xff, 0x05, 0x41, 0x00],
            vec![0xff, 0xfe, 0xff, WIDE_COUNT, 0xff, 0xff],
            vec![
                0xff, 0xfe, 0xff, WIDE_COUNT, 0xff, 0xff, 0x03, 0x00, 0x00, 0x00, 0x41, 0x00,
            ],
            {
                let mut oversized = MARKER.to_vec();
                oversized.push(WIDE_COUNT);
                oversized.extend_from_slice(&FAR_COUNT.to_le_bytes());
                oversized.extend_from_slice(&u32::MAX.to_le_bytes());
                oversized
            },
        ];
        cases.push(valid[..valid.len() - 1].to_vec());
        for bytes in cases {
            let (result, position) = decode(&bytes);
            assert!(result.is_err(), "truncated frame is a failed read");
            assert_eq!(position, 0);
        }
        let mut prefixed = vec![0x11, 0x22];
        prefixed.extend([0xff, 0xfe, 0xff, 0x04, 0x41, 0x00]);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, mut view) =
            DecodeContext::from_root_bytes(&prefixed, &arena, &policy).expect("root context");
        view.skip(2).expect("prefix length");
        assert!(read_framed_string(&ctx, &mut view).is_err());
        assert_eq!(view.position(), 2);
    }

    #[test]
    fn invalid_surrogates_are_rejected() {
        for units in [
            vec![0xd800],
            vec![0xdc00],
            vec![0xd800, 0x0041],
            vec![0x0041, 0xdc00],
        ] {
            let mut bytes = MARKER.to_vec();
            bytes.push(u8::try_from(units.len()).expect("surrogate fixture fits its frame"));
            bytes.extend(encode_units(&units));
            let (result, position) = decode(&bytes);
            assert!(
                matches!(result, Err(cadmpeg_core::CodecError::Malformed(_))),
                "unpaired surrogates are rejected"
            );
            assert_eq!(position, 0);
        }
    }
}
