// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read Siemens NX `.prt` files into [`cadmpeg_ir::document::CadIr`].
//!
//! [`NxCodec`] is the normal public decode API. A hidden `fuzz` module
//! exposes `()`-returning parser wrappers. The codec recognizes the `SPLMSSTR`
//! container signature, extracts compressed Parasolid neutral-binary streams
//! from the canonical part payload, and decodes supported geometry and
//! topology. Detection uses file content because NX and Creo share the `.prt`
//! extension.
//!
//! <!-- generated: capability nx -->
//! Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#siemens-nx-prt)).
//! <!-- /generated: capability nx -->
//!
//! Connected B-rep on selected or terminal-lineage-resolved body images
//! shows as extras. `RMFastLoad` body selection retains every body
//! whose complete nonempty topology node-ID set is covered by the active
//! object-ID set; when no body has that complete membership, it declines and
//! falls back to terminal lineage when complete.
//!
//! # Decode a part
//!
//! ```no_run
//! use std::fs::File;
//!
//! use cadmpeg_codec_nx::NxCodec;
//! use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.prt")?;
//! let result = NxCodec.decode(&mut input, &DecodeOptions::default())?;
//!
//! println!("{} bodies", result.ir().model.bodies.len());
//! for loss in &result.report().losses {
//!     println!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Codec::inspect`](cadmpeg_ir::codec::Codec::inspect) returns the SPLMSSTR directory and embedded-stream
//! classifications without decoding entities. `DecodeOptions::container_only`
//! produces metadata IR and skips entity decode.
//!
//! # Model and loss boundaries
//!
//! NX stores part geometry in zlib-compressed Parasolid partition, deltas, or
//! plain streams. The decoder converts Parasolid metre values to millimetres and
//! emits points; analytic curves and surfaces; NURBS curves and surfaces;
//! selected trimmed curves; and resolvable body, region, shell, face, loop,
//! coedge, edge, and vertex topology. Each inflated Parasolid stream is also
//! retained as an unknown record.
//!
//! Read [`cadmpeg_ir::report::decode::DecodeReport`] before using the model as a complete
//! representation. Deltas streams pair with the preceding equal-schema partition
//! in validated `UG_PART` segment order and apply
//! supported non-topology full records and exact-key tombstones using the last
//! event for each key. Valid partition topology remains authoritative. Unmatched
//! tombstone relations remain unresolved. Segment body aliases, primary-body
//! writers, and Boolean tool operands select terminal partition images when the
//! complete body lineage is unambiguous. Assembly files may contain only
//! references to external child parts.
//!
//! Ordered feature-operation records, body-write GROUP ownership, body
//! dependencies, Boolean operations, sketch record lanes, and numeric
//! expressions transfer from the NX object model. Current-body writers and
//! their complete earlier dependency closure transfer as active; other
//! operation suppression remains unresolved. Embedded
//! JT coordinates and triangle connectivity transfer as canonical tessellations.
//! Complete design history, assembly occurrence placement, material and appearance
//! assignment and `.prt` writing are not supported.
//! Part attributes transfer as document attributes. The object-model extraction
//! and attachment tier (record families, feature semantics, and IR writing) is
//! crate-internal and reached only through the decode entry point.

mod canonical_uuid;
mod container;
mod decode;
mod deltas;
mod dialect;
mod evaluation;
mod framing;
mod geometry;
mod intersection;
mod inspect;
mod iter_wire;
mod jt;
mod jt_topology;
/// Byte-offset constants generated from `docs/layouts/nx.toml`.
mod layout;
#[allow(dead_code)] // Loss catalog is consumed by tests and the writer.
mod loss;
mod native;
mod nurbs;
mod om;
mod om_tokens;
mod parasolid;
mod scan_notes;
mod payload_text;
mod printable_string;
mod topology;
mod vec3_at;

#[doc(hidden)]
pub mod fuzz;

#[doc(hidden)]
pub use native::hex::Sha256Hex;

#[doc(hidden)]
pub use evaluation::{
    saved_body_census_evidence, BodyCensusEvaluation, FeatureBoundary, UnsupportedBodyCensusReason,
};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::ContainerSummary;

/// Decoder and inspector for Siemens NX `.prt` files.
#[derive(Debug, Default, Clone, Copy)]
pub struct NxCodec;

impl CodecBackend for NxCodec {
    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);

    fn validate_native(
        ctx: &DecodeContext<'_>,
        ir: &cadmpeg_ir::CadIr,
    ) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
        let Some(namespace) = ir.native.namespace("nx") else {
            return Ok(Vec::new());
        };
        let admitted = native::display_jt::admission::DisplayJtGraph::from_namespace_with_context(
            ctx, namespace,
        )
        .and_then(|_| namespace.admit::<native::structure::occurrences::FastLoadOccurrences>());
        Ok(match admitted {
            Ok(_) => Vec::new(),
            Err(error) => {
                let message = error.to_string();
                let codec_error = CodecError::from(error);
                if matches!(codec_error, CodecError::ResourceLimit(_)) {
                    return Err(codec_error);
                }
                vec![cadmpeg_ir::report::check::Finding {
                    check: cadmpeg_ir::report::check::Check::NativeLinks,
                    severity: cadmpeg_ir::report::Severity::Error,
                    message,
                    entity: None,
                }]
            }
        })
    }

    fn detect_impl(&self, prefix: &[u8]) -> Confidence {
        if container::looks_like_nx(prefix) || container::looks_like_legacy_nx(prefix) {
            Confidence::High
        } else {
            Confidence::No
        }
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let scan = decode::scan(ctx, root)?;
        inspect::summarize(ctx, &scan)
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
