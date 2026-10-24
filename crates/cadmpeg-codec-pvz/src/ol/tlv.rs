// SPDX-License-Identifier: Apache-2.0
//! TLV framing of an expanded OL stream (§4.4) and the stream index record
//! every stream opens with (§4.5).

use cadmpeg_core::decode::{index_from_u32, View};
use cadmpeg_core::CodecError;

use crate::layout::{tlv_header, token};
use crate::read::{at, record};

/// Tag of the stream index record (§4.5): the identity of the edge
/// tessellation stream.
const STREAM_INDEX_TAG: u32 = token::STREAM_EDGE_TESSELLATION;
/// Payload bytes of the stream index record.
const STREAM_INDEX_LEN: usize = 4;
/// Mask of the stream identity byte the stream index record repeats.
const STREAM_INDEX_MASK: u32 = 0xff;

/// One TLV record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Record<'a> {
    pub(crate) tag: u32,
    pub(crate) payload: View<'a>,
}

impl Record<'_> {
    /// Whether this is the tag-0, length-0 scope terminator.
    pub(crate) fn is_terminator(&self) -> bool {
        self.tag == 0 && self.payload.window().is_empty()
    }
}

/// Reads the next record, or `None` at the end of the stream.
pub(super) fn next<'a>(stream: &mut View<'a>) -> Result<Option<Record<'a>>, CodecError> {
    if stream.is_empty() {
        return Ok(None);
    }
    let header = record(stream, tlv_header::LEN)?;
    let tag = at(header, tlv_header::TAG)?.req_u32_le()?;
    let length = at(header, tlv_header::LENGTH)?.req_u32_le()?;
    let payload = stream.req_take_child(index_from_u32(length))?;
    Ok(Some(Record { tag, payload }))
}

/// Reads the stream index record of a stream with identity `stream_id` and
/// returns its value, or refuses a stream that does not open with one.
pub(super) fn stream_index(stream: &mut View<'_>, stream_id: u32) -> Result<u32, CodecError> {
    let first = next(stream)?.filter(|first| {
        first.tag == STREAM_INDEX_TAG && first.payload.window().len() == STREAM_INDEX_LEN
    });
    let Some(first) = first else {
        return Err(CodecError::malformed(format_args!(
            "OL stream {stream_id:#010x} does not open with its stream index record"
        )));
    };
    let mut payload = first.payload;
    let value = payload.req_u32_le()?;
    let expected = stream_id & STREAM_INDEX_MASK;
    if value != expected {
        return Err(CodecError::malformed(format_args!(
            "OL stream {stream_id:#010x} states stream index {value}, not {expected}"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::View;

    use super::{next, stream_index, STREAM_INDEX_TAG};
    use crate::test_support::ol::tlv;

    #[test]
    fn records_fill_the_stream_in_order() {
        let mut bytes = tlv(STREAM_INDEX_TAG, &1_u32.to_le_bytes());
        bytes.extend(tlv(0, &[]));
        bytes.extend(tlv(0x1b, &[1, 2, 3, 4]));
        let mut stream = View::over_retained(&bytes);
        assert_eq!(stream_index(&mut stream, 0x0100_0001).unwrap(), 1);
        let terminator = next(&mut stream).unwrap().unwrap();
        assert!(terminator.is_terminator());
        let list = next(&mut stream).unwrap().unwrap();
        assert_eq!((list.tag, list.payload.window()), (0x1b, &[1, 2, 3, 4][..]));
        assert!(!list.is_terminator());
        assert!(next(&mut stream).unwrap().is_none());
    }

    #[test]
    fn a_record_past_the_stream_end_is_refused() {
        let mut bytes = tlv(0x1b, &[1, 2, 3, 4]);
        bytes.truncate(10);
        assert!(next(&mut View::over_retained(&bytes)).is_err());
        assert!(next(&mut View::over_retained(&[1, 0, 0])).is_err());
    }

    #[test]
    fn the_stream_index_record_names_its_stream() {
        let stream = tlv(STREAM_INDEX_TAG, &2_u32.to_le_bytes());
        assert!(stream_index(&mut View::over_retained(&stream), 0x0100_0001).is_err());
        assert_eq!(
            stream_index(&mut View::over_retained(&stream), 0x0100_0002).unwrap(),
            2
        );
        let exact = tlv(STREAM_INDEX_TAG, &3_u32.to_le_bytes());
        assert_eq!(
            stream_index(&mut View::over_retained(&exact), 0x0100_0003).unwrap(),
            3
        );
        assert!(stream_index(&mut View::over_retained(&tlv(5, &[0; 4])), 0x0100_0000).is_err());
        assert!(stream_index(&mut View::over_retained(&[]), 0x0100_0000).is_err());
    }
}
