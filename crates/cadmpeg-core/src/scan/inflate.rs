// SPDX-License-Identifier: Apache-2.0
//! Bounded inflate steps with admission before output production.

use super::{ScanBudget, StopReason};
use crate::decode::{u64_from_index, View};
use flate2::{Decompress, FlushDecompress, Status};

const CHUNK: usize = 4096;
const INFLATE_STATE_BYTES: u64 = 128 * 1024;

/// Compression framing of a feature-list member.
#[derive(Debug, Clone, Copy)]
pub enum InflateFraming {
    /// Raw DEFLATE, as used by ZIP members.
    Raw,
    /// Zlib header and checksum.
    Zlib,
    /// Zstandard frames, including skippable frames.
    Zstd,
}

impl ScanBudget<'_> {
    /// Inflates an acquired member in bounded steps and returns consumed input.
    pub fn inflate(
        &self,
        source: View<'_>,
        framing: InflateFraming,
    ) -> Result<(Vec<u8>, usize), StopReason> {
        if matches!(framing, InflateFraming::Zstd) {
            let bytes = crate::decode::zstd::scan::decode(source, self)?;
            return Ok((bytes, source.window().len()));
        }
        self.retain(INFLATE_STATE_BYTES)?;
        let mut decoder = Decompress::new(matches!(framing, InflateFraming::Zlib));
        let mut input = source;
        let mut output = Vec::new();
        loop {
            self.check()?;
            let capacity = usize::try_from(self.expansion_available().min(u64_from_index(CHUNK)))
                .map_err(|_| StopReason::ExpandedBytes)?;
            self.expand(u64_from_index(capacity))?;
            self.retain(u64_from_index(capacity))?;
            let mut chunk = [0; CHUNK];
            let before_in = decoder.total_in();
            let before_out = decoder.total_out();
            let available = input.remaining().min(CHUNK);
            let bytes = input
                .unread()
                .get(..available)
                .ok_or_else(|| StopReason::Truncated("compressed member".into()))?;
            let status = decoder
                .decompress(bytes, &mut chunk[..capacity], FlushDecompress::None)
                .map_err(|error| StopReason::Malformed(error.to_string()))?;
            let consumed = usize::try_from(decoder.total_in() - before_in)
                .map_err(|_| StopReason::SourceBytes)?;
            let produced = usize::try_from(decoder.total_out() - before_out)
                .map_err(|_| StopReason::ExpandedBytes)?;
            self.release_expansion(u64_from_index(capacity - produced));
            input.req_take(consumed)?;
            output
                .try_reserve_exact(produced)
                .map_err(|_| StopReason::RetainedBytes)?;
            output.extend_from_slice(&chunk[..produced]);
            if status == Status::StreamEnd {
                return Ok((
                    output,
                    usize::try_from(decoder.total_in()).map_err(|_| StopReason::SourceBytes)?,
                ));
            }
            if capacity == 0 {
                return Err(self.refuse(StopReason::ExpandedBytes));
            }
            if consumed == 0 && produced == 0 {
                return Err(StopReason::Truncated("compressed member".into()));
            }
        }
    }
}
