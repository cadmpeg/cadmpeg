// SPDX-License-Identifier: Apache-2.0
//! Reads CATIA V5 `.CATPart` files into [`cadmpeg_ir::CadIr`].
//!
//! [`CatiaCodec`] is the normal public decode API. The optional `fuzzing`
//! feature may expose additional modules. It implements the shared [`Codec`]
//! interface: it detects the `V5_CFV2` file signature, inspects the catalogued
//! logical streams, identifies the storage variant, and decodes the record
//! families supported for that variant.
//!
//! Support level: [L2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! on the cadmpeg support ladder for the standard-nested layout; other layouts
//! are L1.
//!
//! # Decode a part
//!
//! ```
//! use std::fs::File;
//!
//! use cadmpeg_codec_catia::CatiaCodec;
//! use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
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
//! Byte-level format semantics are documented in
//! [`docs/formats/catia.md`](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md).
//!
//! # Internal layout
//!
//! `wire` reads endian scalars and tag records; `solve` holds the pure topology
//! solvers; `families/*` pair each record vocabulary with its decode route; and
//! `assemble` lowers decoded records into the neutral IR. All of these are
//! crate-private; nothing but `CatiaCodec` is part of the public API.

pub(crate) mod analytic;
mod appearance;
pub(crate) mod assemble;
mod boundary_roles;
pub(crate) mod catalog;
pub(crate) mod configuration;
pub(crate) mod container;
pub(crate) mod coverage;
pub(crate) mod decode;
pub(crate) mod design_feature;
pub(crate) mod entity_table;
pub(crate) mod families;
pub(crate) mod formula;
pub(crate) mod legacy_entity;
pub(crate) mod native;
pub(crate) mod nurbs;
pub(crate) mod object_graph;
pub(crate) mod sketch;
pub(crate) mod solve;
pub(crate) mod value_block;
pub(crate) mod variant;
pub(crate) mod wire;

#[cfg(feature = "fuzzing")]
pub mod fuzz;

/// Maximum number of exact rational-quadratic spans materialized for one
/// angular curve or surface direction from untrusted native parameters.
pub(crate) const MAX_EXACT_ARC_SPANS: usize = 4_096;

/// Maximum number of control points materialized for one NURBS surface from
/// untrusted native cardinalities.
pub(crate) const MAX_NURBS_SURFACE_CONTROL_POINTS: usize = 1_000_000;

/// Multiplies two NURBS surface dimensions within the materialization limit.
pub(crate) fn nurbs_surface_control_count(u_count: usize, v_count: usize) -> Option<usize> {
    u_count
        .checked_mul(v_count)
        .filter(|count| *count <= MAX_NURBS_SURFACE_CONTROL_POINTS)
}

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeResult};

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
mod tests;
