// SPDX-License-Identifier: Apache-2.0
//! Reads CATIA V5 `.CATPart` files into [`cadmpeg_ir::CadIr`].
//!
//! [`CatiaCodec`] implements the shared [`Codec`] interface. It detects the
//! `V5_CFV2` file signature, inspects catalogued logical streams, identifies the
//! storage variant, and decodes the record families supported for that variant.
//!
//! # Decode a part
//!
//! ```
//! use std::fs::File;
//!
//! use cadmpeg_codec_catia::CatiaCodec;
//! use cadmpeg_ir::{Codec, DecodeOptions};
//!
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.CATPart")?;
//! let decoded = CatiaCodec.decode(&mut input, &DecodeOptions::default())?;
//! println!("{} surfaces", decoded.ir.model.surfaces.len());
//! # Ok(())
//! # }
//! ```
//!
//! Read `decoded.report.losses` before consuming model relationships. A partial
//! decode preserves the native payload in an unknown record and reports the
//! model layers that remain unresolved.
//!
//! # Format model
//!
//! Most `CATPart` files contain an outer `V5_CFV2` header and a nested container.
//! Its `CATIA_V5 CB0001` directory maps named logical streams to physical extent
//! lists. [`container`] reconstructs these streams before [`decode`] selects a
//! decoder using [`variant::Variant`].
//!
//! Standard nested parts can produce analytic surfaces, curves, vertices,
//! bodies, faces, loops, coedges, and edges when the stored trim and endpoint
//! relations resolve to one graph. Other recognized layouts expose supported
//! analytic or NURBS carriers and selected bindings. The codec does not write
//! `CATPart` files or decode assemblies, design history, tessellation,
//! appearances, materials, persistent tags, or general document metadata.
//!
//! The low-level [`geometry`], [`topology`], [`b5`], [`e5`], and
//! [`zero_entity`] modules expose record decoders for applications that need
//! format-level access.

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
