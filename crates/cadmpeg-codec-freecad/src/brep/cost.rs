// SPDX-License-Identifier: Apache-2.0
//! Bytes compared by exact 3D curve equivalence.

use super::{NestedCurve, TextCurve};
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsPoles3;

impl DecodeCost for NestedCurve {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (&self.curve, self.depth).decode_cost(ctx, operation)
    }
}

impl DecodeCost for TextCurve {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        let fields = match self {
            Self::Line { origin, direction } => (origin, direction).decode_cost(ctx, operation)?,
            Self::Circle { center, axis, ref_direction, radius } =>
                (center, axis, ref_direction, radius).decode_cost(ctx, operation)?,
            Self::Ellipse { center, axis, major_direction, major_radius, minor_radius }
            | Self::Hyperbola { center, axis, major_direction, major_radius, minor_radius } =>
                (center, axis, major_direction, major_radius, minor_radius).decode_cost(ctx, operation)?,
            Self::Parabola { vertex, axis, major_direction, focal_distance } =>
                (vertex, axis, major_direction, focal_distance).decode_cost(ctx, operation)?,
            Self::Nurbs(curve) => match curve.pole_rows() {
                NurbsPoles3::Polynomial { points } => (
                    curve.degree(), curve.periodic(), curve.knots().as_slice(), (0_u8, points),
                ).decode_cost(ctx, operation)?,
                NurbsPoles3::Rational { points } => {
                    let header = (0_u8, curve.degree(), curve.periodic(), curve.knots().as_slice())
                        .decode_cost(ctx, operation)?;
                    ctx.fold(points, header, |bytes, pole| {
                        bytes.checked_add((pole.point, pole.weight).decode_cost(ctx, operation)?)
                            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))
                    }, operation)?
                }
            },
            Self::Trimmed { parameter_range, basis } =>
                (parameter_range, basis).decode_cost(ctx, operation)?,
            Self::Offset { distance, direction, basis } =>
                (distance, direction, basis).decode_cost(ctx, operation)?,
        };
        fields.checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))
    }
}
