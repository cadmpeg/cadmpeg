// SPDX-License-Identifier: Apache-2.0
//! Scoped DEFLATE output for detection.

use super::{u64_from_index, DecodeContext, ScopedReservation, View};
use crate::CodecError;
use flate2::{Decompress, FlushDecompress, Status};

impl DecodeContext<'_> {
    /// Inflates one raw or zlib member up to `cap`. An oversized or malformed
    /// member supplies no evidence. Resource refusals remain typed.
    pub fn inflate_probe(
        &self,
        source: View<'_>,
        cap: usize,
        zlib: bool,
    ) -> Result<Option<(Vec<u8>, ScopedReservation<'_>)>, CodecError> {
        self.charge_work(
            u64_from_index(source.window().len()),
            "probe compressed input",
        )?;
        // miniz_oxide uses a 32 KiB dictionary and fixed Huffman tables. This
        // 256 KiB bound includes the state and table storage for one decoder.
        let _workspace = self.reserve_scoped(256 * 1024, "DEFLATE probe workspace")?;
        let mut decoder = Decompress::new(zlib);
        let mut storage = self.reserve_scoped(0, "DEFLATE probe output")?;
        let mut output = Vec::new();
        let mut chunk = [0_u8; 1024];
        let mut offset = 0;
        loop {
            self.charge_work(u64_from_index(chunk.len()), "DEFLATE probe step")?;
            let before_in = decoder.total_in();
            let before_out = decoder.total_out();
            let Ok(status) = decoder.decompress(
                &source.window()[offset..],
                &mut chunk,
                FlushDecompress::None,
            ) else {
                return Ok(None);
            };
            let consumed = usize::try_from(decoder.total_in() - before_in)
                .map_err(|_| CodecError::Malformed("DEFLATE probe input overflow".into()))?;
            let produced = usize::try_from(decoder.total_out() - before_out)
                .map_err(|_| CodecError::Malformed("DEFLATE probe output overflow".into()))?;
            offset += consumed;
            if cap
                .checked_sub(output.len())
                .is_none_or(|remaining| produced > remaining)
            {
                return Ok(None);
            }
            self.charge_work(u64_from_index(produced), "DEFLATE probe copy")?;
            self.reserve_scoped_vec(&mut storage, &mut output, produced, "DEFLATE probe output")?;
            output.extend_from_slice(&chunk[..produced]);
            if status == Status::StreamEnd {
                return Ok(Some((output, storage)));
            }
            if consumed == 0 && produced == 0 {
                return Ok(Some((output, storage)));
            }
        }
    }
}

#[cfg(test)]
mod tests {}
