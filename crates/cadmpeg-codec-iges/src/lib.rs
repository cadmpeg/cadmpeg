// SPDX-License-Identifier: Apache-2.0
//! Read-only IGES 5.3 Fixed ASCII codec.

mod card;
mod global;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, ReadSeek,
};

/// Codec for IGES files.
#[derive(Debug, Default, Clone, Copy)]
pub struct IgesCodec;

impl Codec for IgesCodec {
    fn id(&self) -> &'static str {
        "iges"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        card::detect_fixed_ascii(prefix)
    }

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        let scan = card::scan(reader)?;
        let global = global::parse(&scan)?;
        let mut summary = card::summarize(&scan);
        summary.notes.extend(global.summary_notes());
        Ok(summary)
    }

    fn decode(
        &self,
        _reader: &mut dyn ReadSeek,
        _options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        Err(CodecError::NotImplemented("IGES decode".into()))
    }
}

#[cfg(test)]
mod tests;
