// SPDX-License-Identifier: Apache-2.0
//! Inspect and structurally decode PTC Creo Parametric and Pro/ENGINEER `.prt`
//! files stored in the PSB container.
//!
//! Support level: [L1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! on the cadmpeg support ladder.
//!
//! # Quick start
//!
//! [`CreoCodec`] implements [`cadmpeg_ir::codec::Codec`]. Use
//! [`CreoCodec::inspect`] to enumerate sections and read container diagnostics:
//!
//! ```no_run
//! use std::fs::File;
//!
//! use cadmpeg_codec_creo::CreoCodec;
//! use cadmpeg_ir::codec::Codec;
//! use cadmpeg_ir::InspectOptions;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.prt")?;
//! let summary = CreoCodec.inspect(&mut input, &InspectOptions::default())?;
//! println!("{} sections", summary.entries.len());
//! # Ok(())
//! # }
//! ```
//!
//! Use [`CreoCodec::decode`] for a [`cadmpeg_ir::document::CadIr`] document and
//! its [`cadmpeg_ir::report::DecodeReport`].
//!
//! # Format model
//!
//! A PSB file begins with the `#UGC:2` ASCII signature, an ASCII header and
//! table of contents, then named binary sections. Detection uses this signature
//! because Siemens NX also uses the `.prt` extension.
//!
//! [`container`] identifies ND and DEPDB layouts, classifies sections, reads
//! surface and curve namespace counts, and discovers typed namespace rows.
//! [`psb`] and [`scalar`] expose the context-independent primitive decoders.
//! [`surface`], [`curve`], and [`topology`] expose the typed structural model.
//!
//! # Decode scope
//!
//! Decode transfers standard model-space datum planes from `ActDatums` as
//! derived, unbounded plane surfaces. It preserves PSB geometry sections as
//! [`cadmpeg_ir::unknown::UnknownRecord`] values.
//!
//! Surface prototype parameters describe family templates rather than placed
//! instances. Per-instance coordinates, curve geometry, face bindings, and
//! feature evaluation are incomplete, so the codec does not emit a body B-rep.
//! The decode report identifies these losses and reports whether any datum
//! planes were transferred.

pub mod container;
pub mod curve;
pub mod datum;
pub mod decode;
pub mod feature;
pub mod psb;
pub mod scalar;
pub mod surface;
pub mod topology;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult,
};
use cadmpeg_ir::decode::{DecodeContext, View};

/// Codec for Creo Parametric and Pro/ENGINEER PSB `.prt` files.
#[derive(Debug, Default, Clone, Copy)]
pub struct CreoCodec;

impl Codec for CreoCodec {
    fn id(&self) -> &'static str {
        "creo"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        // The `#UGC:2` ASCII magic is unique to the Creo/Pro-E PSB container and
        // distinguishes it from a Siemens NX `.prt` sharing the extension.
        if container::looks_like_creo(prefix) {
            Confidence::High
        } else {
            Confidence::No
        }
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let mut reader = std::io::Cursor::new(root.window());
        let scan = container::scan(&mut reader)?;
        Ok(container::summarize(&scan))
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        let options = DecodeOptions {
            container_only: ctx.container_only(),
            policy: *ctx.policy(),
        };
        let mut reader = std::io::Cursor::new(root.window());
        decode::decode(&mut reader, &options)
    }
}

#[cfg(test)]
mod tests;
