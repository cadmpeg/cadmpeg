// SPDX-License-Identifier: Apache-2.0
//! Reads Rhino `.3dm` files into [`cadmpeg_ir::document::CadIr`].
//!
//! The codec provides bounded 3DM container inspection and container-only
//! decoding for the full-decode archive bands.

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, ReadSeek,
};

pub(crate) mod chunks;
pub(crate) mod container;

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

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        container::inspect(reader)
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        container::decode(reader, options.container_only)
    }
}

#[cfg(test)]
mod tests;
