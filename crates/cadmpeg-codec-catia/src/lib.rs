// SPDX-License-Identifier: Apache-2.0
//! # cadmpeg-codec-catia
//!
//! Decoder for Dassault Systèmes CATIA V5 `.CATPart` files.
//!
//! ## What is implemented
//!
//! A `.CATPart` is a `V5_CFV2` container: an outer file header with a big-endian
//! directory offset/length pair, and (for most parts) a nested `V5_CFV2`
//! sub-container whose `CATIA_V5 CB0001` directory catalogues named logical
//! streams as extent lists. The geometry is stored in one of **five distinct
//! families**, each with its own decode path. This codec:
//!
//! - [`CatiaCodec::detect`] recognizes the unique `V5_CFV2\0` outer magic;
//! - [`CatiaCodec::inspect`] parses the outer header, reconstructs the inner
//!   stream directory, enumerates its named streams, and identifies the storage
//!   variant (spec §1) — this works for every variant;
//! - [`CatiaCodec::decode`] decodes standard-nested `05 08 01` vertex points,
//!   tag-bridged planes, and inline analytic carriers from `SurfacicReps`; it
//!   also decodes directly framed analytic carriers in zero-entity `a9 03`
//!   streams, E5 circle/cone/torus carriers, and `a8`/`a5` freeform NURBS
//!   carrier pools where present.
//!
//! ## What is decoded, and what is reported as loss
//!
//! Standard-nested geometry decodes vertices, analytic carriers, compatible
//! circle/line carriers, and faces when their stored senses are available.
//! Zero-entity, E5, and object-stream families transfer their directly framed
//! carriers. Remaining carrier-to-topology bindings are reported as loss rather
//! than fabricated.

pub mod b5;
pub mod container;
pub mod decode;
pub mod e5;
pub mod geometry;
pub mod topology;
pub mod variant;
pub mod zero_entity;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, ReadSeek,
};

/// The CATIA V5 `.CATPart` codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct CatiaCodec;

impl Codec for CatiaCodec {
    fn id(&self) -> &'static str {
        "catia"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if container::looks_like_catia(prefix) {
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

#[cfg(test)]
mod tests;
