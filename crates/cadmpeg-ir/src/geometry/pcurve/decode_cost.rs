// SPDX-License-Identifier: Apache-2.0
//! Byte costs of pcurve carriers and their inline bases.

use super::{
    PcurveGeometry, PolarNurbsPole, PolarNurbsPoles, PolarPcurveNurbs, WeightedPolarNurbsPole,
};
use crate::geometry::decode_cost::{checked_sum, inline_bytes};
use crate::scalar::FiniteReal;
use crate::units::FinitePoint2;
use cadmpeg_core::decode::{cost::DecodeCost, DecodeContext};
use cadmpeg_core::CodecError;

impl DecodeCost for PcurveGeometry {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        let bytes = match self {
            Self::Line(value) => inline_bytes(value),
            Self::PolarHarmonic(value) => inline_bytes(value),
            Self::SphericalGreatCircle(value) => inline_bytes(value),
            Self::Circle(value) => inline_bytes(value),
            Self::Ellipse(value) => inline_bytes(value),
            Self::Harmonic(value) => inline_bytes(value),
            Self::Parabola(value) => inline_bytes(value),
            Self::Hyperbola(value) => inline_bytes(value),
            Self::Hyperbolic(value) => inline_bytes(value),
            Self::Nurbs { nurbs } => nurbs.decode_cost(ctx, operation)?,
            Self::PolarNurbs { nurbs } => nurbs.decode_cost(ctx, operation)?,
            Self::Transformed(value) => {
                let _depth = ctx.enter_nested(operation)?;
                ctx.charge_work(1, operation)?;
                (&value.basis, value.transform.affine_rows(), value.depth)
                    .decode_cost(ctx, operation)?
            }
            Self::Trimmed(value) => {
                let _depth = ctx.enter_nested(operation)?;
                ctx.charge_work(1, operation)?;
                (
                    &value.basis,
                    value.parameter_range.endpoints(),
                    value.same_sense,
                    value.depth,
                )
                    .decode_cost(ctx, operation)?
            }
            Self::Offset(value) => {
                let _depth = ctx.enter_nested(operation)?;
                ctx.charge_work(1, operation)?;
                (&value.basis, value.distance, value.depth).decode_cost(ctx, operation)?
            }
        };
        checked_sum(ctx, 1, bytes, operation)
    }
}

impl DecodeCost for PolarNurbsPole<FinitePoint2, FiniteReal> {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (self.radial, self.axial).decode_cost(ctx, operation)
    }
}

impl DecodeCost for WeightedPolarNurbsPole<FinitePoint2, FiniteReal> {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (self.radial, self.axial, self.weight).decode_cost(ctx, operation)
    }
}

impl DecodeCost for PolarNurbsPoles<FinitePoint2, FiniteReal> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        match self {
            Self::Polynomial { poles } => (0_u8, poles).decode_cost(ctx, operation),
            Self::Rational { poles } => (0_u8, poles).decode_cost(ctx, operation),
        }
    }
}

impl DecodeCost for PolarPcurveNurbs {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (self.degree, &self.knots, &self.poles, self.periodic).decode_cost(ctx, operation)
    }
}
