// SPDX-License-Identifier: Apache-2.0
//! Reads CATIA V5 `.CATPart` files into [`cadmpeg_ir::CadIr`].
//!
//! [`CatiaCodec`] is the normal public decode API. A hidden `fuzz` module
//! exposes `()`-returning parser wrappers. It implements the shared
//! [`cadmpeg_ir::codec::Codec`]
//! interface: it detects the `V5_CFV2` file signature, inspects the catalogued
//! logical streams, identifies the storage variant, and decodes the record
//! families supported for that variant.
//!
//! <!-- generated: capability catia -->
//! Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#catia-v5-catpart)).
//! <!-- /generated: capability catia -->
//!
//! Recognized storage families transfer typed geometry, topology, design
//! records, and presentation data when their source relations are complete.
//!
//! # Decode a part
//!
//! ```
//! use std::fs::File;
//!
//! use cadmpeg_codec_catia::CatiaCodec;
//! use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};
//!
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.CATPart")?;
//! let decoded = CatiaCodec.decode(&mut input, &DecodeOptions::default())?;
//! println!("{} surfaces", decoded.ir().model.surfaces.len());
//! # Ok(())
//! # }
//! ```
//!
//! Read `decoded.report().losses` before consuming model relationships. A partial
//! decode preserves the native payload in an unknown record and reports the
//! model layers that remain unresolved.
//!
//! Byte-level format semantics are documented in
//! [`docs/formats/catia.md`](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md).

mod analytic;
mod appearance;
mod assemble;
mod boundary_roles;
mod catalog;
mod checked;
mod container;
mod coverage;
mod decode;
mod design_feature;
mod dialect;
mod entity_table;
mod families;
mod formula;
mod ids;
/// Byte-offset constants generated from `docs/layouts/catia.toml`.
mod layout;
mod legacy_entity;
#[allow(dead_code)] // Loss catalog is consumed by tests and the writer.
mod loss;
mod math;
mod native;
mod nurbs;
mod object_graph;
mod pmi;
mod sketch;
mod solve;
mod unique_index;
mod value_block;
mod variant;
mod wire;

#[doc(hidden)]
pub mod fuzz;

/// Maximum number of exact rational-quadratic spans materialized for one
/// angular curve or surface direction from untrusted native parameters.
const MAX_EXACT_ARC_SPANS: usize = 4_096;

/// Maximum number of control points materialized for one NURBS surface from
/// untrusted native cardinalities.
const MAX_NURBS_SURFACE_CONTROL_POINTS: usize = 1_000_000;

/// Multiplies two NURBS surface dimensions within the materialization limit.
fn nurbs_surface_control_count(u_count: usize, v_count: usize) -> Option<usize> {
    u_count
        .checked_mul(v_count)
        .filter(|count| *count <= MAX_NURBS_SURFACE_CONTROL_POINTS)
}

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::ContainerSummary;

/// The CATIA V5 `.CATPart` codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct CatiaCodec;

impl CodecBackend for CatiaCodec {
    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);

    fn detect_impl(&self, prefix: &[u8]) -> Confidence {
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

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        decode::decode(ctx, root)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;
