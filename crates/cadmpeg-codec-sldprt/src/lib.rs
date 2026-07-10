// SPDX-License-Identifier: Apache-2.0
//! # cadmpeg-codec-sldprt
//!
//! Decoder for `SolidWorks` `.sldprt` files.
//!
//! ## What is implemented
//!
//! A `.sldprt` is a custom block-framed container: an 8-byte outer header
//! followed by CRC-validated raw-DEFLATE blocks, a fixed-cell section-index
//! grid, and a tail section directory (all sharing one marker). The solid B-rep
//! lives in paired embedded Parasolid `partition` and `deltas` streams. This codec:
//!
//! - [`SldprtCodec::detect`] recognizes the block marker after the outer header;
//! - [`SldprtCodec::inspect`] enumerates every block (with its decompressed-
//!   payload family and, for Parasolid blocks, the stream schema), the tail
//!   directory entries, and the cache-cell grid;
//! - [`SldprtCodec::decode`] locates the active Parasolid stream, walks its
//!   typed topology chain (`face → bridge → surface`, `loop → coedge → edge-use
//!   → curve`, `coedge → vertex-use → point`), decodes the compact analytic
//!   surface/curve carriers it references, and builds the IR B-rep graph.
//!
//! ## What is decoded, and what is reported as loss
//!
//! Analytic and NURBS carriers are decoded, with lengths
//! converted metre→millimetre (×1000). Faces whose support surface is a carrier
//! this codec does not type (offset, swept, blended, intersection, and
//! spline-on-surface) keep their
//! topology and are emitted with a [`SurfaceGeometry::Unknown`] surface linking
//! to the preserved record bytes. Every such omission is counted in the
//! [`cadmpeg_ir::report::DecodeReport`]. When no Parasolid body stream can be
//! located or framed, decode falls back to container-metadata only and says so.
//!
//! [`SurfaceGeometry::Unknown`]: cadmpeg_ir::geometry::SurfaceGeometry::Unknown

mod appearance;
pub mod brep;
pub mod container;
pub mod decode;
mod history;
mod metadata;
pub mod parasolid;
mod resolved_features;
mod tessellation;
mod writer;
mod writer_patch;
mod writer_transform;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, Encoder, ReadSeek,
};
use cadmpeg_ir::document::CadIr;
use sha2::{Digest, Sha256};
use std::io::Write;

/// The `SolidWorks` `.sldprt` codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct SldprtCodec;

impl SldprtCodec {
    /// Write unchanged IR byte-for-byte, or regenerate supported modified geometry.
    pub fn write_preserved(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<(), CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("semantic_sha256"));
        if expected.is_none_or(|expected| decode::semantic_hash(ir) != *expected) {
            return writer::write_semantic(ir, writer);
        }
        let record = ir
            .unknowns
            .iter()
            .find(|record| record.id.0 == "sldprt:source-image")
            .ok_or_else(|| {
                CodecError::NotImplemented("IR has no retained SLDPRT source image".into())
            })?;
        let data = record.data.as_ref().ok_or_else(|| {
            CodecError::Malformed("retained SLDPRT source image has no bytes".into())
        })?;
        let hash = Sha256::digest(data)
            .iter()
            .fold(String::new(), |mut acc, byte| {
                use std::fmt::Write as _;
                let _ = write!(acc, "{byte:02x}");
                acc
            });
        if data.len() as u64 != record.byte_len || hash != record.sha256 {
            return Err(CodecError::Malformed(
                "retained SLDPRT source image failed integrity validation".into(),
            ));
        }
        writer.write_all(data)?;
        Ok(())
    }
}

impl Codec for SldprtCodec {
    fn id(&self) -> &'static str {
        "sldprt"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if container::looks_like_sldprt(prefix) {
            Confidence::High
        } else {
            Confidence::No
        }
    }

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(reader)?;
        Ok(container::summarize(&scan))
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(reader, options)
    }
}

impl Encoder for SldprtCodec {
    fn id(&self) -> &'static str {
        "sldprt"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<(), CodecError> {
        self.write_preserved(ir, writer)
    }
}

#[cfg(test)]
mod tests;
