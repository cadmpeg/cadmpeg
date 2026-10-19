// SPDX-License-Identifier: Apache-2.0
//! Raw-lane NURBS construction and pole scaling.

use super::{
    NurbsCurve, NurbsError, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
    NurbsSurfaceLanes, PoleValue,
};
use crate::features::FinitePoint3;

impl NurbsCurve {
    /// Pair raw pole and weight lanes, then build the curve.
    pub fn from_lanes<P: PoleValue<FinitePoint3>, K: super::KnotValue>(
        degree: u32,
        knots: K,
        control_points: Vec<P>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        let poles = super::pair_curve_lanes(control_points, weights, |index, weight| {
            super::admit_weight("poles", index, weight)
        })?;
        Self::new(degree, knots, poles, periodic)
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
    /// Pair raw pole and weight grids, then build the surface.
    pub fn from_lanes<P: PoleValue<FinitePoint3>, U: super::KnotValue, V: super::KnotValue>(
        u: NurbsSurfaceAxis<U>,
        v: NurbsSurfaceAxis<V>,
        lanes: NurbsSurfaceLanes<P>,
        normal_reversed: bool,
    ) -> Result<Self, NurbsError> {
        let NurbsSurfaceLanes {
            control_points,
            weights,
        } = lanes;
        let poles = super::pair_grid_lanes(control_points, weights, |index, weight| {
            super::admit_weight("pole grid row", index, weight)
        })?;
        Self::new(u, v, poles, normal_reversed)
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
