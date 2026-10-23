// SPDX-License-Identifier: Apache-2.0
//! Exact B-rep geometry stream of an `.ol` member (specification §8).
//!
//! [`parse`] reads the expanded TLV payload of an exact-geometry stream into
//! an [`model::ExactModel`]: the face, edge-curve, and pcurve sections with
//! their indexes checked. [`emit`] resolves the derived topology of §8.17 to
//! §8.20 (coedge sense, loop traversal, face regions, vertices, and shells)
//! and writes the neutral B-rep into a model.

pub(crate) mod emit;
pub(crate) mod eval;
pub(crate) mod frames;
pub(crate) mod model;
pub(crate) mod records;
pub(crate) mod resolve;
pub(crate) mod topology;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(crate) use emit::emit;

/// Stream identity of the exact-geometry stream with `f32` scalars.
pub(crate) const EXACT_STREAM_F32: u32 = 0x0100_0003;
/// Stream identity of the exact-geometry stream with `f64` scalars.
pub(crate) const EXACT_STREAM_F64: u32 = 0x0100_0004;

/// Reads the expanded payload of the exact-geometry stream `stream_id`.
pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    stream_id: u32,
    payload: View<'_>,
) -> Result<model::ExactModel, CodecError> {
    records::read_stream(ctx, stream_id, payload)
}
