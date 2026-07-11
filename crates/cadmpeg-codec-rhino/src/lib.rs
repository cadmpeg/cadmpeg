// SPDX-License-Identifier: Apache-2.0
//! Reads Rhino `.3dm` files into [`cadmpeg_ir::document::CadIr`].
//!
//! The codec currently provides format detection only. Inspection and decoding
//! are reserved for later implementation phases.

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, ReadSeek,
};

pub(crate) mod chunks;

const MAGIC: &[u8] = chunks::MAGIC;

/// Decoder and inspector for Rhino `.3dm` files.
#[derive(Debug, Default, Clone, Copy)]
pub struct RhinoCodec;

impl Codec for RhinoCodec {
    fn id(&self) -> &'static str {
        "rhino"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if prefix.windows(MAGIC.len()).any(|window| window == MAGIC) {
            Confidence::High
        } else {
            Confidence::No
        }
    }

    fn inspect(&self, _reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        Err(CodecError::NotImplemented(
            "Rhino 3DM inspection is not implemented".to_string(),
        ))
    }

    fn decode(
        &self,
        _reader: &mut dyn ReadSeek,
        _options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        Err(CodecError::NotImplemented(
            "Rhino 3DM decoding is not implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests;
