// SPDX-License-Identifier: Apache-2.0
//! Projected radial controls through radius validation and polar conversion.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::PositiveLength;

pub(super) struct CylindricalRadialControls<'curve, 'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    curve: &'curve NurbsCurve,
    values: Vec<Point2>,
    _storage: ScopedReservation<'ctx>,
}

impl<'curve, 'ctx, 'arena> CylindricalRadialControls<'curve, 'ctx, 'arena> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'arena>,
        curve: &'curve NurbsCurve,
        mut project: impl FnMut(Point3) -> Point2,
    ) -> Result<Self, CodecError> {
        let (values, storage) = ctx.with_scoped_storage("collect Parasolid cylinder radial poles", || {
            match curve.pole_rows() {
                NurbsPoles3::Polynomial { points } => ctx.collect_vec(
                    points.iter().map(|point| project(point.get())),
                    "collect Parasolid cylinder radial poles",
                ),
                NurbsPoles3::Rational { points } => ctx.collect_vec(
                    points.iter().map(|pole| project(pole.point.get())),
                    "collect Parasolid cylinder radial poles",
                ),
            }
        })?;
        Ok(Self { context: ctx, curve, values, _storage: storage })
    }

    pub(super) fn values(&self) -> &[Point2] { &self.values }

    pub(super) fn has_constant_radius(&self, radius: PositiveLength) -> Result<bool, CodecError> {
        let ctx = self.context;
        let (result, weights_storage) = ctx.with_scoped_storage("collect Parasolid cylinder pole weights", || {
            let weights = match self.curve.pole_rows() {
                NurbsPoles3::Polynomial { .. } => None,
                NurbsPoles3::Rational { points } => Some(ctx.collect_vec(
                    points.iter().map(|pole| pole.weight.get()),
                    "collect Parasolid cylinder pole weights",
                )?),
            };
            super::pcurves::quadratic_nurbs_has_constant_radius(ctx, &self.values, weights.as_deref(), self.curve.knots(), radius)
        })?;
        drop(weights_storage);
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
