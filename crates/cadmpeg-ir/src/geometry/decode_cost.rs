// SPDX-License-Identifier: Apache-2.0
//! Byte costs of geometry values and their owned children.

use super::{
    LegacyExtensionFlags, OffsetExtension, RevisionCacheForm, RevisionSurfaceForm,
    RevisionSurfaceParameterization, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_core::decode::{cost::DecodeCost, DecodeContext};
use cadmpeg_core::CodecError;

pub(super) fn checked_sum(
    ctx: &DecodeContext<'_>,
    left: u64,
    right: u64,
    operation: &'static str,
) -> Result<u64, CodecError> {
    left.checked_add(right)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))
}

pub(super) fn inline_bytes<T>(value: &T) -> u64 {
    cadmpeg_core::decode::u64_from_index(std::mem::size_of_val(value))
}

fn array_cost<T: DecodeCost, const N: usize>(
    ctx: &DecodeContext<'_>,
    values: &[T; N],
    operation: &'static str,
) -> Result<u64, CodecError> {
    let mut bytes = 0;
    for value in values {
        bytes = checked_sum(ctx, bytes, value.decode_cost(ctx, operation)?, operation)?;
    }
    Ok(bytes)
}

impl DecodeCost for SurfaceGeometry {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        match self {
            Self::Procedural {
                construction,
                cache,
            } => (0_u8, construction, cache).decode_cost(ctx, operation),
            Self::Solved(value) => (0_u8, value).decode_cost(ctx, operation),
        }
    }
}

impl DecodeCost for SolvedSurfaceGeometry {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        let bytes = match self {
            Self::Plane(value) => inline_bytes(value),
            Self::Cylinder(value) => inline_bytes(value),
            Self::Cone(value) => inline_bytes(value),
            Self::Sphere(value) => inline_bytes(value),
            Self::Torus(value) => inline_bytes(value),
            Self::Nurbs(value) => value.decode_cost(ctx, operation)?,
            Self::Polygonal(value) => value.decode_cost(ctx, operation)?,
            Self::Transformed(value) => {
                let _depth = ctx.enter_nested(operation)?;
                ctx.charge_work(1, operation)?;
                (&value.basis, &value.transform, value.depth).decode_cost(ctx, operation)?
            }
            Self::Unknown { record } => record.decode_cost(ctx, operation)?,
        };
        checked_sum(ctx, 1, bytes, operation)
    }
}

impl DecodeCost for LegacyExtensionFlags {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        match self {
            Self::Absent {} | Self::Disabled {} => Ok(1),
            Self::Enabled {
                secondary,
                tertiary,
            } => (0_u8, secondary, tertiary).decode_cost(ctx, operation),
        }
    }
}

impl<R: DecodeCost> DecodeCost for OffsetExtension<R> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        match self {
            Self::Legacy { flags, cache } => {
                let tolerance = cache.as_ref().map(|cache| cache.fit_tolerance.get());
                (0_u8, flags, tolerance).decode_cost(ctx, operation)
            }
            Self::Revision { form } => (0_u8, form).decode_cost(ctx, operation),
        }
    }
}

impl<F: DecodeCost + Default, R: DecodeCost> DecodeCost for RevisionSurfaceForm<F, R> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        let mut bytes = (
            self.revision.get(),
            &self.flags,
            &self.cache,
            self.tail_flag,
            &self.trailing_flags,
        )
            .decode_cost(ctx, operation)?;
        bytes = checked_sum(
            ctx,
            bytes,
            array_cost(ctx, &self.support_bounds, operation)?,
            operation,
        )?;
        bytes = checked_sum(
            ctx,
            bytes,
            array_cost(ctx, &self.reference_endpoints, operation)?,
            operation,
        )?;
        bytes = checked_sum(
            ctx,
            bytes,
            array_cost(ctx, &self.second_endpoints, operation)?,
            operation,
        )?;
        checked_sum(
            ctx,
            bytes,
            array_cost(ctx, &self.discontinuities, operation)?,
            operation,
        )
    }
}

impl<P: DecodeCost> DecodeCost for RevisionCacheForm<P> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        match self {
            Self::SolvedCache { fit_tolerance } => {
                (0_u8, fit_tolerance.get()).decode_cost(ctx, operation)
            }
            Self::Parameterization(value) => (0_u8, value).decode_cost(ctx, operation),
        }
    }
}

impl<R: DecodeCost> DecodeCost for RevisionSurfaceParameterization<R> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        let bytes = (
            self.u_closure,
            self.v_closure,
            self.u_singularity,
            self.v_singularity,
        )
            .decode_cost(ctx, operation)?;
        let bytes = checked_sum(
            ctx,
            bytes,
            array_cost(ctx, &self.u_interval, operation)?,
            operation,
        )?;
        checked_sum(
            ctx,
            bytes,
            array_cost(ctx, &self.v_interval, operation)?,
            operation,
        )
    }
}
