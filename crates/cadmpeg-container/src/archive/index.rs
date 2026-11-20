// SPDX-License-Identifier: Apache-2.0
//! An admitted namespace keeps opaque comment bytes outside dependency selection.

use crate::layout::end_record;
use std::io::{Cursor, Read, Seek, SeekFrom};

pub(super) struct NamespaceReader<'bytes> {
    bytes: Cursor<&'bytes [u8]>,
    comment_length: u64,
}

impl<'bytes> NamespaceReader<'bytes> {
    pub(super) fn new(bytes: &'bytes [u8], end: usize) -> Self {
        Self {
            bytes: Cursor::new(&bytes[..end + end_record::LEN]),
            comment_length: cadmpeg_core::decode::u64_from_index(end + end_record::COMMENT_LENGTH),
        }
    }
}

impl Read for NamespaceReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let start = self.bytes.position();
        let length = self.bytes.read(output)?;
        for field in [self.comment_length, self.comment_length + 1] {
            if let Some(index) = field
                .checked_sub(start)
                .and_then(|n| usize::try_from(n).ok())
            {
                if let Some(byte) = output[..length].get_mut(index) {
                    *byte = 0;
                }
            }
        }
        Ok(length)
    }
}

impl Seek for NamespaceReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.bytes.seek(position)
    }
}

/// Uses the dependency's central-name decoder before its namespace map merges keys.
pub(super) fn declaration_name(
    bytes: &[u8],
    central: u64,
    entries_end: u64,
) -> Result<String, cadmpeg_core::CodecError> {
    use crate::layout::central_header;
    use cadmpeg_core::decode::View;
    use cadmpeg_core::CodecError;

    let start = usize::try_from(central)
        .map_err(|_| CodecError::malformed("ZIP central offset exceeds memory"))?;
    let mut length = central_header::LEN;
    for field in [
        central_header::NAME_LENGTH,
        central_header::EXTRA_LENGTH,
        central_header::COMMENT_LENGTH,
    ] {
        length += usize::from(
            View::u16_le_at(bytes, start + field)
                .ok_or_else(|| CodecError::malformed("truncated ZIP central length"))?,
        );
    }
    let end = start
        .checked_add(length)
        .filter(|&end| cadmpeg_core::decode::u64_from_index(end) <= entries_end)
        .ok_or_else(|| CodecError::malformed("ZIP central record exceeds admitted directory"))?;
    let record = bytes
        .get(start..end)
        .ok_or_else(|| CodecError::malformed("truncated ZIP central record"))?;
    let mut footer = [0; end_record::LEN];
    footer[..4].copy_from_slice(b"PK\x05\x06");
    for field in [end_record::DISK_ENTRIES, end_record::ENTRIES] {
        footer[field..field + 2].copy_from_slice(&1_u16.to_le_bytes());
    }
    let size = u32::try_from(length)
        .map_err(|_| CodecError::malformed("ZIP central record size exceeds u32"))?;
    footer[end_record::DIRECTORY_SIZE..end_record::DIRECTORY_SIZE + 4]
        .copy_from_slice(&size.to_le_bytes());
    let reader = DeclarationReader {
        record,
        footer,
        position: 0,
    };
    let config = zip::read::Config {
        archive_offset: zip::read::ArchiveOffset::Known(0),
    };
    let archive = zip::ZipArchive::with_config(config, reader)
        .map_err(|error| CodecError::malformed(format_args!("not a readable ZIP: {error}")))?;
    archive
        .name_for_index(0)
        .ok_or_else(|| CodecError::malformed("ZIP indexed name is absent"))?
        .map(std::borrow::Cow::into_owned)
        .map_err(|error| CodecError::malformed(format_args!("invalid ZIP entry name: {error}")))
}

struct DeclarationReader<'a> {
    record: &'a [u8],
    footer: [u8; end_record::LEN],
    position: u64,
}

impl Read for DeclarationReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let Ok(start) = usize::try_from(self.position) else {
            return Ok(0);
        };
        let source = if start < self.record.len() {
            &self.record[start..]
        } else {
            self.footer
                .get(start - self.record.len()..)
                .unwrap_or_default()
        };
        let length = output.len().min(source.len());
        output[..length].copy_from_slice(&source[..length]);
        self.position += cadmpeg_core::decode::u64_from_index(length);
        Ok(length)
    }
}

impl Seek for DeclarationReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let next = match position {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => {
                cadmpeg_core::decode::u64_from_index(self.record.len() + self.footer.len())
                    .checked_add_signed(offset)
            }
        };
        self.position = next.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid ZIP declaration seek",
            )
        })?;
        Ok(self.position)
    }
}
