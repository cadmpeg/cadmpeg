// SPDX-License-Identifier: Apache-2.0
//! Inspect and structurally decode PTC Creo Parametric and Pro/ENGINEER `.prt`
//! files stored in the PSB container.
//!
//! [`CreoCodec`] is the normal public decode API. A hidden `fuzz` module
//! exposes `()`-returning parser wrappers. It implements [`cadmpeg_ir::codec::Codec`]:
//! it detects the `#UGC:2` PSB signature, inspects named sections, and decodes
//! the geometry, topology, sketches, and design records supported for that
//! layout.
//!
//! Support level: [L1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! on the cadmpeg support ladder.
//!
//! # Quick start
//!
//! Use [`cadmpeg_ir::Codec::inspect`] to enumerate sections and read container
//! diagnostics:
//!
//! ```no_run
//! use std::fs::File;
//!
//! use cadmpeg_codec_creo::CreoCodec;
//! use cadmpeg_ir::codec::Codec;
//! use cadmpeg_core::decode::InspectOptions;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.prt")?;
//! let summary = CreoCodec.inspect(&mut input, &InspectOptions::default())?;
//! println!("{} sections", summary.entries.len());
//! # Ok(())
//! # }
//! ```
//!
//! Use [`cadmpeg_ir::Codec::decode`] for a [`cadmpeg_ir::document::CadIr`] document and
//! its [`cadmpeg_ir::report::DecodeReport`].
//!
//! # Format model
//!
//! A PSB file begins with the `#UGC:2` ASCII signature and an ASCII header.
//! Legacy persistence uses a `P_OBJECT` body with optional named sections;
//! later persistence uses a table of contents and named binary sections.
//! Detection uses the signature because Siemens NX also uses `.prt`.
//!
//! # Decode scope
//!
//! Decode transfers complete model-space planes, selected cylinders, placed
//! cones, tori, and spheres when positional or feature construction establishes
//! model space, interpolation and NURBS-related carriers with complete control
//! bodies, reference lines, circles, and ellipses, connected topology with
//! analytic intersections and pcurves, `SolidPrimdata` triangle strips, a root
//! product identity occurrence, placed section sketches, and typed features,
//! parameters, and expressions. It preserves PSB geometry sections as
//! [`cadmpeg_ir::unknown::UnknownRecord`] values. The crate is read-only.
//!
//! Surface prototype parameters describe family templates rather than placed
//! instances. Other per-instance coordinates, curve families, face bindings,
//! and feature evaluation remain incomplete. The decode report identifies
//! these losses.

mod compress;
pub(crate) mod container;
pub(crate) mod coverage;
pub(crate) mod curve;
pub(crate) mod datum;
pub(crate) mod decode;
pub(crate) mod feature;
/// Byte-offset constants generated from `docs/layouts/creo.toml`.
pub(crate) mod layout;
pub(crate) mod legacy;
pub(crate) mod legacy_family;
pub(crate) mod legacy_feature;
pub(crate) mod legacy_geometry;
pub(crate) mod loop_array;
#[allow(dead_code)] // Loss catalog is consumed by tests and the writer.
pub(crate) mod loss;
pub(crate) mod placement;
pub(crate) mod primdata;
pub(crate) mod psb;
pub(crate) mod reference;
pub(crate) mod scalar;
pub(crate) mod surface;
pub(crate) mod topology;
pub(crate) mod vecmath;

#[doc(hidden)]
pub mod fuzz;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{CodecBackend, Confidence, DecodeResult};

/// Codec for Creo Parametric and Pro/ENGINEER PSB `.prt` files.
#[derive(Debug, Default, Clone, Copy)]
pub struct CreoCodec;

impl CodecBackend for CreoCodec {
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
        let scan = container::scan_bytes(root.window());
        Ok(container::summarize(&scan))
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(ctx, root)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;
