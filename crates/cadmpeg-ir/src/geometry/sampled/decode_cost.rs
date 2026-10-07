// SPDX-License-Identifier: Apache-2.0
//! Byte cost of a polygonal surface's admitted vertices and triangles.

use super::PolygonalSurface;
use cadmpeg_core::decode::{cost::DecodeCost, DecodeContext};
use cadmpeg_core::CodecError;

impl DecodeCost for PolygonalSurface {
    fn decode_cost(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<u64, CodecError> {
        (&self.vertices, &self.triangles, self.chordal_deflection).decode_cost(ctx, operation)
    }
}
