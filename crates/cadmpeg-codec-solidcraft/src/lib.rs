// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Inspect and decode `SolidCraft` `.sc` and `.solidcraft` designs.
//!
//! A design is a plain JSON document or a ZIP container that holds the
//! document beside content-addressed payload blobs and disposable caches.
//! [`SolidcraftCodec`] implements [`cadmpeg_ir::codec::Codec`]: it detects a
//! design by its content, inspects every container member with its role and
//! checks each blob against the digest in its name, and classifies the
//! encoding and the document format. Decode reads the document header,
//! decodes the final state of the model cache into neutral B-rep, and retains
//! every member as a named native record.
//!
//! <!-- generated: capability solidcraft -->
//! Support: L5 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidcraft-sc-and-solidcraft)).
//! <!-- /generated: capability solidcraft -->
//!
//! # Quick start
//!
//! ```no_run
//! use std::fs::File;
//!
//! use cadmpeg_codec_solidcraft::SolidcraftCodec;
//! use cadmpeg_core::decode::InspectOptions;
//! use cadmpeg_ir::codec::Codec;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("bracket.sc")?;
//! let summary = SolidcraftCodec.inspect(&mut input, &InspectOptions::default())?;
//! for entry in &summary.entries {
//!     println!("{} {}", entry.role, entry.name);
//! }
//! # Ok(())
//! # }
//! ```

mod cache;
mod canvas;
mod decode;
mod detect;
mod dialect;
mod document;
mod history;
mod inspect;
mod look;
mod loss;
mod migrate;
mod notes;
mod package;
mod points;
mod source;
mod writer;

#[doc(hidden)]
pub mod fuzz;

use cadmpeg_core::decode::{DecodeContext, DecodeDepth, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::{Catalog, ResolvedWrite},
    DepthNeed, EncodeInput, EncoderBackend, ExportBody,
};
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::ContainerSummary;

/// Codec for `SolidCraft` designs.
#[derive(Debug, Default, Clone, Copy)]
pub struct SolidcraftCodec;

impl CodecBackend for SolidcraftCodec {
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

impl EncoderBackend for SolidcraftCodec {
    const FORMAT: FormatId = <Self as CodecBackend>::FORMAT;
    /// Reads the document digest and the retained source image, which a
    /// shallower decode does not record.
    const NEEDS: DepthNeed = DepthNeed::AtLeast(DecodeDepth::Full);
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(dialect::TARGETS, Some(0));

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        writer::plan(input, &target)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
