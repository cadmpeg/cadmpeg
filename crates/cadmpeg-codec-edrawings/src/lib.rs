// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Inspect and decode eDrawings `.eprt` and `.easm` packages.
//!
//! A package is a ZIP archive. Its `eModel` member holds a HOOPS Stream
//! Format model: a comment line, a few wrapper records, and one zlib stream.
//! [`EdrawingsCodec`] implements [`cadmpeg_ir::codec::Codec`]: it detects a
//! package by its `eModel` member, inspects every member with its role, and
//! classifies the package dialect from the stream version in the comment
//! line. Decode opens the package, expands the model stream under the decode
//! caps, and walks its records. The stream holds display meshes only, so each
//! body becomes a faceted B-rep (planar faces) and decode reports the loss
//! `edrawings/geometry.reconstructed-from-mesh`. A part becomes one part
//! definition; an assembly becomes part definitions, group definitions, and
//! an occurrence tree with absolute placements in millimetres. The crate is
//! read-only.
//!
//! <!-- generated: capability edrawings -->
//! Support: L2 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidworks-edrawings-eprt-and-easm)).
//! <!-- /generated: capability edrawings -->
//!
//! # Quick start
//!
//! ```no_run
//! use std::fs::File;
//!
//! use cadmpeg_codec_edrawings::EdrawingsCodec;
//! use cadmpeg_core::decode::InspectOptions;
//! use cadmpeg_ir::codec::Codec;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.eprt")?;
//! let summary = EdrawingsCodec.inspect(&mut input, &InspectOptions::default())?;
//! for entry in &summary.entries {
//!     println!("{} {}", entry.role, entry.name);
//! }
//! # Ok(())
//! # }
//! ```

mod decode;
mod detect;
mod dialect;
mod hsf;
mod ids;
mod inspect;
mod loss;
mod mesh_brep;
mod package;
mod stream;

#[doc(hidden)]
pub mod fuzz;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::ContainerSummary;

/// Codec for eDrawings `.eprt` and `.easm` packages.
#[derive(Debug, Default, Clone, Copy)]
pub struct EdrawingsCodec;

impl CodecBackend for EdrawingsCodec {
    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);

    fn detect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        prefix: View<'_>,
    ) -> Result<Confidence, CodecError> {
        detect::confidence(ctx, prefix.window())
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        inspect::inspect(ctx, root)
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        decode::decode(ctx, root)
    }
}

#[cfg(test)]
mod geometry_tests;
#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
