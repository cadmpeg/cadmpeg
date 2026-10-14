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

impl super::NurbsAdmission for DecodeContext<'_> {
    type Error = ConstructionError;

    fn structure(&self, message: std::fmt::Arguments<'_>) -> Self::Error {
        NurbsError::Structure(message.to_string()).into()
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
            let poles = super::pair_curve_lanes(ctx, control_points, weights, |index, weight| {
                super::admit_weight(ctx, "poles", index, weight)
            })?;
            super::build_curve(ctx, degree, knots, poles, periodic)
        })())
    }
}

impl super::BsplineSurface {
    pub(crate) fn scale_points(
        &mut self,
        scale: crate::scalar::PositiveReal,
    ) -> Result<(), NurbsError> {
        scale_points(self.control_points.iter_mut().flatten(), scale)
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
            let poles = super::pair_grid_lanes(ctx, control_points, weights, |index, weight| {
                super::admit_weight(ctx, "pole grid row", index, weight)
            })?;
            super::build_surface(ctx, u, v, poles, normal_reversed)
        })())
    }
}

fn scale_points<'a>(
    points: impl Iterator<Item = &'a mut FinitePoint3>,
    scale: crate::scalar::PositiveReal,
) -> Result<(), NurbsError> {
    for point in points {
        *point = point.scaled(scale).ok_or_else(|| {
            NurbsError::Structure("control_points contains a non-finite point".to_owned())
        })?;
    }
    Ok(())
}

impl NurbsCurve {
    pub(crate) fn scale_points(
        &mut self,
        scale: crate::scalar::PositiveReal,
    ) -> Result<(), NurbsError> {
        match &mut self.poles {
            NurbsPoles3::Polynomial { points } => scale_points(points.iter_mut(), scale),
            NurbsPoles3::Rational { points } => {
                scale_points(points.iter_mut().map(|pole| &mut pole.point), scale)
            }
        }
    }
}

impl NurbsSurface {
    pub(crate) fn scale_points(
        &mut self,
        scale: crate::scalar::PositiveReal,
    ) -> Result<(), NurbsError> {
        match &mut self.poles {
            NurbsPoleGrid::Polynomial { rows } => scale_points(rows.iter_mut().flatten(), scale),
            NurbsPoleGrid::Rational { rows } => {
                scale_points(rows.iter_mut().flatten().map(|pole| &mut pole.point), scale)
            }
        }
    }
}

#[cfg(test)]
mod tests;
