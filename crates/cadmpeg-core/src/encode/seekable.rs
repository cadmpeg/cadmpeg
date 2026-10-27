// SPDX-License-Identifier: Apache-2.0
//! Final seekable output retains the caller account until serialization ends.

use std::io::{self, Cursor, Seek, SeekFrom, Write};

use super::{EncodeBuffer, EncodeSession};
use crate::decode::{u64_from_index, ResourceLimit};
use crate::io::WriteSeek;

impl EncodeSession {
    /// Serializes final output on this account through a private seekable sink.
    ///
    /// The callback completes and drops its serializer before returning. A
    /// resource refusal takes precedence over its operation result, including
    /// an internal `WriteZero` caused by a refused write. An operation error
    /// returns no output. Success hands over the final bytes without a copy.
    pub fn write_seekable<T, E>(
        &self,
        operation: &'static str,
        write: impl for<'output> FnOnce(&'output mut dyn WriteSeek) -> Result<T, E>,
    ) -> Result<Result<(Vec<u8>, T), E>, ResourceLimit> {
        self.finish()?;
        let mut sink = OutputSink {
            buffer: self.buffer(),
            position: 0,
            operation,
        };
        let result = write(&mut sink);
        self.finish()?;
        Ok(result.map(|value| (sink.buffer.into_vec(), value)))
    }
}

struct OutputSink<'a> {
    buffer: EncodeBuffer<'a>,
    position: u64,
    operation: &'static str,
}

impl OutputSink<'_> {
    fn admit_write(&mut self, length: usize) -> Result<(usize, usize), ResourceLimit> {
        self.buffer.session.finish()?;
        let start = match usize::try_from(self.position) {
            Ok(start) => start,
            Err(_) => {
                let growth = self.position - u64_from_index(self.buffer.bytes.capacity());
                return Err(self
                    .buffer
                    .session
                    .account
                    .allocation_failed(growth, self.operation));
            }
        };
        let end = match start.checked_add(length) {
            Some(end) => end,
            None => {
                if start > self.buffer.len() {
                    self.buffer
                        .reserve_append(start - self.buffer.len(), self.operation)?;
                }
                let available = self.buffer.bytes.capacity() - start;
                let growth = u64_from_index(length - available);
                return Err(self
                    .buffer
                    .session
                    .account
                    .allocation_failed(growth, self.operation));
            }
        };
        if end > self.buffer.len() {
            self.buffer
                .reserve_append(end - self.buffer.len(), self.operation)?;
        }
        Ok((start, end))
    }
}

impl Write for OutputSink<'_> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if input.is_empty() {
            return Ok(0);
        }
        let Ok((start, end)) = self.admit_write(input.len()) else {
            return Ok(0);
        };
        const ZEROES: [u8; 4096] = [0; 4096];
        while self.buffer.len() < start {
            let length = (start - self.buffer.len()).min(ZEROES.len());
            self.buffer.bytes.extend_from_slice(&ZEROES[..length]);
        }
        let overwrite_end = end.min(self.buffer.len());
        let overwritten = overwrite_end - start;
        self.buffer.bytes[start..overwrite_end].copy_from_slice(&input[..overwritten]);
        self.buffer.bytes.extend_from_slice(&input[overwritten..]);
        self.position = u64_from_index(end);
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for OutputSink<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let mut cursor = Cursor::new(self.buffer.as_bytes());
        cursor.set_position(self.position);
        let position = cursor.seek(position)?;
        self.position = position;
        Ok(position)
    }
}

#[cfg(test)]
mod tests;
