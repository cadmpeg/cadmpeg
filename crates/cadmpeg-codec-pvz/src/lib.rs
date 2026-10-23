// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Inspect and decode PTC Creo View (`ProductView`) `.pvz` packages.
//!
//! A `.pvz` package is a ZIP archive. It holds one `.pvs` structure member
//! and `.ol` geometry members. [`PvzCodec`] implements
//! [`cadmpeg_ir::codec::Codec`]: it detects the package by its structure
//! member, inspects every member with its signature, and classifies the
//! package dialect from the structure member's signature. Decode reads the
//! structure member into product definitions and an occurrence tree, and
//! each geometry member a definition names into a body: the neutral B-rep of
//! its exact-geometry stream, with its tessellations and appearances. The
//! crate is read-only.
//!
//! <!-- generated: capability pvz -->
//! Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#creo-view-pvz)).
//! <!-- /generated: capability pvz -->
//!
//! # Quick start
//!
//! ```no_run
//! use std::fs::File;
//!
//! use cadmpeg_codec_pvz::PvzCodec;
//! use cadmpeg_core::decode::InspectOptions;
//! use cadmpeg_ir::codec::Codec;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("assembly.pvz")?;
//! let summary = PvzCodec.inspect(&mut input, &InspectOptions::default())?;
//! for entry in &summary.entries {
//!     println!("{} {}", entry.role, entry.name);
//! }
//! # Ok(())
//! # }
//! ```

mod brep;
mod decode;
mod detect;
mod dialect;
mod gzip;
mod ids;
mod inspect;
/// Byte-offset constants generated from `docs/layouts/creo_view_pvz.toml`.
mod layout;
mod loss;
mod model;
mod ol;
mod package;
mod pvs;
mod read;

#[doc(hidden)]
pub mod fuzz;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::ContainerSummary;

/// Codec for PTC Creo View `.pvz` packages.
#[derive(Debug, Default, Clone, Copy)]
pub struct PvzCodec;

impl CodecBackend for PvzCodec {
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
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
