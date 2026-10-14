// SPDX-License-Identifier: Apache-2.0
//! Bounded DEFLATE output for detection.

use super::{DecodeContext, View};
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
    ) -> Result<Option<Vec<u8>>, CodecError> {
        if let Some(limit) = self.resource_refusal() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let mut decoder = Decompress::new(zlib);
        let mut output = Vec::new();
        let mut chunk = [0_u8; 1024];
        let mut offset = 0;
        loop {
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
            output.extend_from_slice(&chunk[..produced]);
            if status == Status::StreamEnd || (consumed == 0 && produced == 0) {
                return Ok(Some(output));
            }
        }
    }
}
