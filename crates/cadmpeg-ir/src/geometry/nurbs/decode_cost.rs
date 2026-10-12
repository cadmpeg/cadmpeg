// SPDX-License-Identifier: Apache-2.0
//! Byte costs of NURBS knots and pole grids.

use super::{KnotVector, NurbsPoleGrid, NurbsSurface, WeightedPole3};
use crate::features::FinitePoint3;
use cadmpeg_core::decode::{cost::DecodeCost, DecodeContext};
use cadmpeg_core::CodecError;

impl DecodeCost for KnotVector {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        self.0.decode_cost(ctx, operation)
    }
}

impl DecodeCost for WeightedPole3<FinitePoint3> {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (self.point, self.weight).decode_cost(ctx, operation)
    }
}

impl DecodeCost for NurbsPoleGrid<FinitePoint3> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        match self {
            Self::Polynomial { rows } => (0_u8, rows).decode_cost(ctx, operation),
            Self::Rational { rows } => (0_u8, rows).decode_cost(ctx, operation),
        }
    }
}

impl DecodeCost for NurbsSurface {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (
            (self.u_degree, self.v_degree, &self.u_knots, &self.v_knots),
            (
                &self.poles,
                self.normal_reversed,
                self.u_periodic,
                self.v_periodic,
            ),
        )
            .decode_cost(ctx, operation)
    }
}
