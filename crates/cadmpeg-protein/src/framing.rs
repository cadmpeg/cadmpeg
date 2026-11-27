// SPDX-License-Identifier: Apache-2.0
//! Paged logical-record framing.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::layout::{continuation_page, instance_stream_header, record_start_page, terminal_page};
use crate::{CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN, TERMINAL_MARKER};

/// One exact logical record recovered from the `InstanceProperties` page
/// framing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordFrame {
    /// Byte offset in the dechunked logical stream.
    logical_offset: usize,
    /// Complete record bytes, including the opening marker.
    bytes: Vec<u8>,
}

impl RecordFrame {
    /// Returns the record offset in the dechunked logical stream.
    pub const fn logical_offset(&self) -> usize {
        self.logical_offset
    }

    /// Returns the complete record bytes, including the opening marker.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Split a paged `InstanceProperties` stream into logical records.
///
/// The stream is a [`STREAM_HEADER_LEN`]-byte header followed by fixed
/// [`PAGE_SIZE`] pages. A page opening with [`TERMINAL_MARKER`] is the last
/// page of its record at any used byte count. Any other page whose bytes 4..8
/// hold [`RECORD_MARKER`] opens a record and [`CONTINUATION_MARKER`] extends
/// it. The last page closes the record and carries the used byte count as a `u16` at
/// offset 4. Every record is returned with the opening marker restored so
/// record offsets match the on-page layout.
pub fn record_frames(bytes: &[u8]) -> Result<Vec<RecordFrame>, CodecError> {
    if bytes.len() < STREAM_HEADER_LEN + PAGE_SIZE {
        return Err(CodecError::Malformed(
            "Protein page stream is shorter than its header and one page".into(),
        ));
    }
    if View::u32_le_at(bytes, instance_stream_header::DECLARED_SIZE)
        .map(cadmpeg_core::decode::index_from_u32)
        != Some(PAGE_SIZE)
    {
        return Err(CodecError::Malformed(
            "Protein declared page size is invalid".into(),
        ));
    }
    if !(bytes.len() - STREAM_HEADER_LEN).is_multiple_of(PAGE_SIZE) {
        return Err(CodecError::Malformed(
            "Protein page stream has a partial trailing page".into(),
        ));
    }
    let mut records = Vec::new();
    let mut current: Option<RecordFrame> = None;
    let mut logical_offset = 0usize;
    for page in bytes[STREAM_HEADER_LEN..].chunks_exact(PAGE_SIZE) {
        let (payload, terminal) = if page.get(terminal_page::MARKER..terminal_page::USED)
            == Some(TERMINAL_MARKER)
        {
            let used =
                usize::from(View::u16_le_at(page, terminal_page::USED).ok_or_else(|| {
                    CodecError::Malformed("Protein terminal used-byte count is truncated".into())
                })?);
            let payload = page
                .get(terminal_page::BODY..terminal_page::BODY + used)
                .ok_or_else(|| {
                    CodecError::Malformed("Protein terminal payload is truncated".into())
                })?;
            if View::u16_le_at(page, terminal_page::RECORD_START) == Some(1) {
                close_open_record(&mut current, &mut logical_offset, &mut records)?;
            }
            (payload, true)
        } else if page.get(record_start_page::MARKER..record_start_page::BODY)
            == Some(RECORD_MARKER)
        {
            close_open_record(&mut current, &mut logical_offset, &mut records)?;
            (&page[record_start_page::BODY..], false)
        } else if page.get(continuation_page::MARKER..continuation_page::BODY)
            == Some(CONTINUATION_MARKER)
        {
            if current.is_none() {
                return Err(CodecError::Malformed(
                    "Protein continuation page has no open record".into(),
                ));
            }
            (&page[continuation_page::BODY..], false)
        } else {
            return Err(CodecError::Malformed(
                "Protein page marker is unknown".into(),
            ));
        };
        if current.is_none() {
            current = Some(RecordFrame {
                logical_offset,
                bytes: RECORD_MARKER.to_vec(),
            });
        }
        let frame = current
            .as_mut()
            .ok_or_else(|| CodecError::Malformed("Protein page has no record owner".into()))?;
        frame.bytes.extend_from_slice(payload);
        if terminal {
            let frame = current.take().ok_or_else(|| {
                CodecError::Malformed("Protein terminal page has no record owner".into())
            })?;
            logical_offset = logical_offset
                .checked_add(frame.bytes.len())
                .ok_or_else(|| {
                    CodecError::Malformed("Protein terminal logical offset overflow".into())
                })?;
            records.push(frame);
        }
    }
    if let Some(record) = current {
        records.push(record);
    }
    Ok(records)
}

/// End the open record, if any, at the page that starts the next record.
fn close_open_record(
    current: &mut Option<RecordFrame>,
    logical_offset: &mut usize,
    records: &mut Vec<RecordFrame>,
) -> Result<(), CodecError> {
    if let Some(record) = current.take() {
        *logical_offset = logical_offset
            .checked_add(record.bytes.len())
            .ok_or_else(|| {
                CodecError::Malformed("Protein record-start logical offset overflow".into())
            })?;
        records.push(record);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn terminal_page_frames_one_record() {
        let mut bytes = vec![0_u8; crate::STREAM_HEADER_LEN + crate::PAGE_SIZE];
        let offset = crate::layout::instance_stream_header::DECLARED_SIZE;
        bytes[offset..offset + 4].copy_from_slice(
            &u32::try_from(crate::PAGE_SIZE)
                .expect("page size")
                .to_le_bytes(),
        );
        let page = &mut bytes[crate::STREAM_HEADER_LEN..];
        page[..4].copy_from_slice(crate::TERMINAL_MARKER);
        page[4..6].copy_from_slice(&5_u16.to_le_bytes());
        let body = crate::layout::terminal_page::BODY;
        page[body..body + 5].copy_from_slice(b"frame");
        let frames = super::record_frames(&bytes).expect("valid terminal page");
        assert_eq!(frames[0].bytes(), [crate::RECORD_MARKER, b"frame"].concat());
    }

    /// A stream of `pages`, each a next-page index, a used byte count, a
    /// record-start flag, and a fill byte for its used body bytes.
    fn paged(pages: &[(i32, u16, u16, u8)]) -> Vec<u8> {
        let mut bytes = vec![0_u8; crate::STREAM_HEADER_LEN];
        let offset = crate::layout::instance_stream_header::DECLARED_SIZE;
        bytes[offset..offset + 4].copy_from_slice(
            &u32::try_from(crate::PAGE_SIZE)
                .expect("page size")
                .to_le_bytes(),
        );
        for (next, used, start, fill) in pages {
            let mut page = vec![0_u8; crate::PAGE_SIZE];
            page[..4].copy_from_slice(&next.to_le_bytes());
            page[4..6].copy_from_slice(&used.to_le_bytes());
            page[6..8].copy_from_slice(&start.to_le_bytes());
            let body = crate::layout::terminal_page::BODY;
            page[body..body + usize::from(*used)].fill(*fill);
            bytes.extend_from_slice(&page);
        }
        bytes
    }

    #[test]
    fn a_full_last_page_closes_its_record() {
        let record = |fills: &[(u8, usize)]| {
            let mut bytes = crate::RECORD_MARKER.to_vec();
            for (fill, count) in fills {
                bytes.extend(std::iter::repeat_n(*fill, *count));
            }
            bytes
        };

        // A two-page record whose last page is full, then a one-page record.
        let frames = super::record_frames(&paged(&[
            (1, 128, 1, b'a'),
            (-1, 128, 0, b'b'),
            (-1, 5, 1, b'c'),
        ]))
        .expect("valid pages");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].bytes(), record(&[(b'a', 128), (b'b', 128)]));
        assert_eq!(frames[1].bytes(), record(&[(b'c', 5)]));
        assert_eq!(frames[1].logical_offset(), frames[0].bytes().len());

        // A one-page record that fills its page, then a one-page record.
        let frames = super::record_frames(&paged(&[(-1, 128, 1, b'a'), (-1, 7, 1, b'b')]))
            .expect("valid pages");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].bytes(), record(&[(b'a', 128)]));
        assert_eq!(frames[1].bytes(), record(&[(b'b', 7)]));

        // A full one-page record after a record-start page that names a next
        // page: the page that starts a record ends the record before it.
        let frames = super::record_frames(&paged(&[(1, 128, 1, b'a'), (-1, 128, 1, b'b')]))
            .expect("valid pages");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].bytes(), record(&[(b'a', 128)]));
        assert_eq!(frames[1].bytes(), record(&[(b'b', 128)]));

        // A last page states at most the 128 bytes of its body.
        let mut oversized = paged(&[(-1, 128, 1, b'a')]);
        let used_at = crate::STREAM_HEADER_LEN + crate::layout::terminal_page::USED;
        oversized[used_at..used_at + 2].copy_from_slice(&129_u16.to_le_bytes());
        assert!(super::record_frames(&oversized).is_err());

        // A continuation page after a full last page has no open record.
        assert!(super::record_frames(&paged(&[
            (1, 128, 1, b'a'),
            (-1, 128, 0, b'b'),
            (3, 128, 0, b'c'),
        ]))
        .is_err());
    }

    #[test]
    fn valid_terminal_page_frames_an_empty_record() {
        let mut bytes = vec![0_u8; crate::STREAM_HEADER_LEN + crate::PAGE_SIZE];
        let offset = crate::layout::instance_stream_header::DECLARED_SIZE;
        bytes[offset..offset + 4].copy_from_slice(
            &u32::try_from(crate::PAGE_SIZE)
                .expect("page size")
                .to_le_bytes(),
        );
        bytes[crate::STREAM_HEADER_LEN..crate::STREAM_HEADER_LEN + crate::TERMINAL_MARKER.len()]
            .copy_from_slice(crate::TERMINAL_MARKER);
        let frames = super::record_frames(&bytes).expect("valid terminal page");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].bytes(), crate::RECORD_MARKER);
    }
}
