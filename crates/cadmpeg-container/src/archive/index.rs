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
