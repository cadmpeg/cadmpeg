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

pub(in crate::geometry) fn finish<T>(
    result: Result<T, ConstructionError>,
) -> Result<Result<T, NurbsError>, CodecError> {
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

    fn reserve<T>(
        &self,
        values: &mut Vec<T>,
        storage: &mut Option<cadmpeg_core::decode::ScopedReservation<'_>>,
        operation: &'static str,
    ) -> Result<(), Self::Error> {
        if let Some(storage) = storage {
            self.reserve_scoped_vec(storage, values, 1, operation)
                .map_err(Into::into)
        } else {
            self.reserve_vec(values, 1, operation).map_err(Into::into)
        }
    }

    fn copy_field(&self, field: &str) -> Result<String, Self::Error> {
        self.copy_retained_text(field, "IR NURBS refusal field")
            .map_err(Into::into)
    }

    fn admit_iter<S: cadmpeg_core::decode::iter_source::IterSource>(
        &self,
        values: S,
        operation: &'static str,
    ) -> Result<impl Iterator<Item = <S::Iter as Iterator>::Item>, Self::Error> {
        DecodeContext::admit_iter(self, values, operation)
            .map_err(|limit| ConstructionError::Resource(limit.into()))
    }

    fn find_by<'a, T>(
        &self,
        values: &'a [T],
        operation: &'static str,
        mut predicate: impl FnMut(&T) -> bool,
    ) -> Result<Option<&'a T>, Self::Error> {
        DecodeContext::find_by(self, values, |value| Ok(predicate(value)), operation)
            .map_err(Into::into)
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
            let poles = super::pair_curve_lanes(
                ctx,
                control_points,
                weights,
                &mut pair_storage,
                |index, weight| super::admit_weight(ctx, "poles", index, weight),
            )?;
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
        scale_grid(ctx, &mut self.control_points, |point| point, scale)
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
            let poles = super::pair_grid_lanes(
                ctx,
                control_points,
                weights,
                &mut pair_storage,
                |index, weight| super::admit_weight(ctx, "pole grid row", index, weight),
            )?;
            super::build_surface(ctx, u, v, poles, normal_reversed)
        })())
    }
}

fn scale_points<T>(
    ctx: &DecodeContext<'_>,
    points: &mut [T],
    point: impl Fn(&mut T) -> &mut FinitePoint3,
    scale: crate::scalar::PositiveReal,
) -> Result<Result<(), NurbsError>, CodecError> {
    let error = ctx.find_map(
        points.iter_mut(),
        |value| {
            let point = point(value);
            let Some(scaled) = point.scaled(scale) else {
                return Ok(Some(NurbsError::Structure(ctx.copy_retained_text(
                    "control_points contains a non-finite point",
                    "IR NURBS refusal text",
                )?)));
            };
            *point = scaled;
            Ok(None)
        },
        "IR NURBS unit scaling work",
    )?;
    Ok(error.map_or(Ok(()), Err))
}

fn scale_grid<T>(
    ctx: &DecodeContext<'_>,
    rows: &mut [Vec<T>],
    point: impl Fn(&mut T) -> &mut FinitePoint3,
    scale: crate::scalar::PositiveReal,
) -> Result<Result<(), NurbsError>, CodecError> {
    let error = ctx.find_map(
        rows.iter_mut(),
        |row| Ok(scale_points(ctx, row, &point, scale)?.err()),
        "IR NURBS unit scaling rows",
    )?;
    Ok(error.map_or(Ok(()), Err))
}

impl NurbsCurve {
    pub(crate) fn scale_points(
        &mut self,
        ctx: &DecodeContext<'_>,
        scale: crate::scalar::PositiveReal,
    ) -> Result<Result<(), NurbsError>, CodecError> {
        match &mut self.poles {
            NurbsPoles3::Polynomial { points } => scale_points(ctx, points, |point| point, scale),
            NurbsPoles3::Rational { points } => {
                scale_points(ctx, points, |pole| &mut pole.point, scale)
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
            NurbsPoleGrid::Polynomial { rows } => scale_grid(ctx, rows, |point| point, scale),
            NurbsPoleGrid::Rational { rows } => {
                scale_grid(ctx, rows, |pole| &mut pole.point, scale)
            }
        }
    }
}

#[cfg(test)]
mod tests;

/// Admit both edit passes before calling a deterministic pole map.
pub(in crate::geometry) fn map_positions<T, P: Copy, E>(
    ctx: &DecodeContext<'_>,
    points: &mut [T],
    get: impl Fn(&T) -> P,
    set: impl Fn(&mut T, P),
    map: impl Fn(usize, P) -> Result<P, E>,
) -> Result<Result<(), E>, CodecError> {
    let count = cadmpeg_core::decode::u64_from_index(points.len());
    let validation = ctx.admit_iter(&*points, "IR pole edit validation")?;
    ctx.charge_work(count, "IR pole edit mutation")?;
    for (index, point) in validation.enumerate() {
        if let Err(error) = map(index, get(point)) {
            return Ok(Err(error));
        }
    }
    for (index, point) in points.iter_mut().enumerate() {
        match map(index, get(point)) {
            Ok(value) => set(point, value),
            Err(error) => return Ok(Err(error)),
        }
    }
    Ok(Ok(()))
}
