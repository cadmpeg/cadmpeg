// SPDX-License-Identifier: Apache-2.0
//! Caller-admitted raw NURBS construction.

use super::{
    NurbsCurve, NurbsError, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
    NurbsSurfaceLanes, PoleValue,
};
use crate::features::FinitePoint3;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

// Resource refusals stay outside the geometry refusal result.
pub(crate) enum ConstructionError {
    Resource(CodecError),
    Geometry(NurbsError),
}

impl From<CodecError> for ConstructionError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}
impl From<NurbsError> for ConstructionError {
    fn from(error: NurbsError) -> Self {
        Self::Geometry(error)
    }
}

pub(in crate::geometry) fn finish<T>(result: Result<T, ConstructionError>) -> Result<Result<T, NurbsError>, CodecError> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(ConstructionError::Geometry(NurbsError::ResourceLimit(limit))) => Err(limit.into()),
        Err(ConstructionError::Geometry(error)) => Ok(Err(error)),
        Err(ConstructionError::Resource(error)) => Err(error),
    }
}

fn structure(
    ctx: &DecodeContext<'_>,
    message: std::fmt::Arguments<'_>,
) -> Result<ConstructionError, CodecError> {
    Ok(NurbsError::Structure(ctx.format_retained(message, "IR NURBS refusal text")?).into())
}

impl super::NurbsAdmission for DecodeContext<'_> {
    type Error = ConstructionError;

    fn collect<I, T>(
        &self,
        values: Vec<I>,
        operation: &'static str,
        convert: impl FnMut(I) -> Result<T, Self::Error>,
    ) -> Result<Vec<T>, Self::Error> {
        self.try_collect_retained_with(values, operation, convert)
    }

    fn reserve<T>(&self, values: &mut Vec<T>, storage: &mut Option<cadmpeg_core::decode::ScopedReservation<'_>>, operation: &'static str) -> Result<(), Self::Error> {
        if let Some(storage) = storage {
            self.reserve_scoped_vec(storage, values, 1, operation).map_err(Into::into)
        } else {
            self.reserve_retained_vec(values, 1, operation).map_err(Into::into)
        }
    }

    fn copy_field(&self, field: &str) -> Result<String, Self::Error> {
        self.copy_retained_text(field, "IR NURBS refusal field").map_err(Into::into)
    }

    fn work(&self, count: u64, operation: &'static str) -> Result<(), Self::Error> {
        self.charge_work(count, operation).map_err(Into::into)
    }

    fn structure(&self, message: std::fmt::Arguments<'_>) -> Result<Self::Error, Self::Error> {
        structure(self, message).map_err(Into::into)
    }
}

impl NurbsCurve {
    /// Construct raw lanes with caller admission before pole pairing and conversion.
    /// Resource refusal is separate from the geometry refusal, whose order is unchanged.
    pub fn from_lanes<P: PoleValue<FinitePoint3>, K: super::KnotValue>(
        ctx: &DecodeContext<'_>,
        degree: u32,
        knots: K,
        control_points: Vec<P>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Result<Self, NurbsError>, CodecError> {
        finish((|| {
            let mut pair_storage = if weights.is_some() && !P::RETAINS_POLE_STORAGE {
                Some(ctx.reserve_scoped(0, "IR NURBS paired poles")?)
            } else {
                None
            };
            let poles = super::pair_curve_lanes(ctx, control_points, weights, &mut pair_storage,
                |index, weight| super::admit_weight(ctx, "poles", index, weight))?;
            super::build_curve(ctx, degree, knots, poles, periodic)
        })())
    }
}

impl super::BsplineSurface {
    pub(crate) fn scale_points(
        &mut self,
        ctx: &DecodeContext<'_>,
        scale: crate::scalar::PositiveReal,
    ) -> Result<Result<(), NurbsError>, CodecError> {
        scale_points(ctx, self.control_points.iter_mut().flatten(), scale)
    }
}

impl NurbsSurface {
    /// Construct raw grids with caller admission before every outer and inner allocation.
    /// Resource refusal is separate from the geometry refusal, whose order is unchanged.
    pub fn from_lanes<P: PoleValue<FinitePoint3>, U: super::KnotValue, V: super::KnotValue>(
        ctx: &DecodeContext<'_>,
        u: NurbsSurfaceAxis<U>,
        v: NurbsSurfaceAxis<V>,
        lanes: NurbsSurfaceLanes<P>,
        normal_reversed: bool,
    ) -> Result<Result<Self, NurbsError>, CodecError> {
        finish((|| {
            let NurbsSurfaceLanes {
                control_points,
                weights,
            } = lanes;
            let mut pair_storage = if weights.is_some() && !P::RETAINS_POLE_STORAGE {
                Some(ctx.reserve_scoped(0, "IR NURBS paired grid rows")?)
            } else {
                None
            };
            let poles = super::pair_grid_lanes(ctx, control_points, weights, &mut pair_storage,
                |index, weight| super::admit_weight(ctx, "pole grid row", index, weight))?;
            super::build_surface(ctx, u, v, poles, normal_reversed)
        })())
    }
}

fn scale_points<'a>(
    ctx: &DecodeContext<'_>,
    points: impl Iterator<Item = &'a mut FinitePoint3>,
    scale: crate::scalar::PositiveReal,
) -> Result<Result<(), NurbsError>, CodecError> {
    for point in points {
        ctx.charge_work(1, "IR NURBS unit scaling work")?;
        let Some(scaled) = point.scaled(scale) else {
            return Ok(Err(NurbsError::Structure(ctx.copy_retained_text(
                "control_points contains a non-finite point",
                "IR NURBS refusal text",
            )?)));
        };
        *point = scaled;
    }
    Ok(Ok(()))
}

impl NurbsCurve {
    pub(crate) fn scale_points(
        &mut self,
        ctx: &DecodeContext<'_>,
        scale: crate::scalar::PositiveReal,
    ) -> Result<Result<(), NurbsError>, CodecError> {
        match &mut self.poles {
            NurbsPoles3::Polynomial { points } => scale_points(ctx, points.iter_mut(), scale),
            NurbsPoles3::Rational { points } => {
                scale_points(ctx, points.iter_mut().map(|pole| &mut pole.point), scale)
            }
        }
    }
}

impl NurbsSurface {
    pub(crate) fn scale_points(
        &mut self,
        ctx: &DecodeContext<'_>,
        scale: crate::scalar::PositiveReal,
    ) -> Result<Result<(), NurbsError>, CodecError> {
        match &mut self.poles {
            NurbsPoleGrid::Polynomial { rows } => {
                scale_points(ctx, rows.iter_mut().flatten(), scale)
            }
            NurbsPoleGrid::Rational { rows } => scale_points(
                ctx,
                rows.iter_mut().flatten().map(|pole| &mut pole.point),
                scale,
            ),
        }
    }
}

#[cfg(test)]
mod tests;
