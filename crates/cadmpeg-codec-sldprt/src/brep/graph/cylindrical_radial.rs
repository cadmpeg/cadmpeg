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
        let (values, storage) =
            ctx.with_scoped_storage("collect Parasolid cylinder radial poles", || {
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
        Ok(Self {
            context: ctx,
            curve,
            values,
            _storage: storage,
        })
    }

    pub(super) fn values(&self) -> &[Point2] {
        &self.values
    }

    /// Consume projected controls after their final use. Raw polar poles and
    /// copied weights expire on conversion; knots and admitted poles survive.
    pub(super) fn into_polar<T>(
        self,
        mut axial: impl FnMut(Point3) -> f64,
        refuse: impl FnOnce(&cadmpeg_ir::geometry::nurbs::NurbsError) -> Result<T, CodecError>,
    ) -> Result<Result<cadmpeg_ir::geometry::pcurve::PolarPcurveNurbs, T>, CodecError> {
        let ctx = self.context;
        let curve = self.curve;
        let mut raw_storage = ctx.reserve_scoped(0, "hold cylindrical polar raw lanes")?;
        let mut poles = Vec::new();
        raw_storage.with_storage(|| {
            ctx.reserve_capacity(
                &mut poles,
                curve.pole_count(),
                "collect cylindrical polar poles",
            )
        })?;
        for (index, radial) in ctx
            .admit_iter(
                self.values(),
                "scan SLDPRT derive_cylindrical_pcurves values",
            )?
            .enumerate()
        {
            let Some(point) = curve.pole_rows().point_at(index) else {
                continue;
            };
            raw_storage.with_storage(|| {
                ctx.push_vec(
                    &mut poles,
                    cadmpeg_ir::geometry::pcurve::PolarNurbsPole {
                        radial: *radial,
                        axial: axial(point.get()),
                    },
                    "collect cylindrical polar poles",
                )
            })?;
        }
        drop(self);
        let candidate_storage;
        let candidate;
        (candidate, candidate_storage) =
            ctx.with_scoped_storage("hold cylindrical polar construction", || {
                // Preserve knot-copy before weight-copy and IR validation.
                let knots = curve
                    .knots()
                    .try_clone_for_decode(ctx, "copy cylindrical polar knots")?;
                let weights = raw_storage.with_storage(|| match curve.pole_rows() {
                    NurbsPoles3::Rational { points } => Ok(Some(ctx.collect_vec(
                        points.iter().map(|pole| pole.weight),
                        "copy cylindrical polar weights",
                    )?)),
                    NurbsPoles3::Polynomial { .. } => Ok::<_, CodecError>(None),
                })?;
                cadmpeg_ir::geometry::pcurve::PolarPcurveNurbs::from_checked_lanes(
                    ctx,
                    curve.degree(),
                    knots,
                    poles,
                    weights,
                    curve.periodic(),
                )
            })?;
        drop(raw_storage);
        match candidate {
            Ok(polar) => Ok(Ok(candidate_storage.commit_value(polar)?)),
            Err(error) => {
                let refusal = refuse(&error)?;
                drop(error);
                drop(candidate_storage);
                Ok(Err(refusal))
            }
        }
    }

    pub(super) fn has_constant_radius(&self, radius: PositiveLength) -> Result<bool, CodecError> {
        let ctx = self.context;
        let (result, weights_storage) =
            ctx.with_scoped_storage("collect Parasolid cylinder pole weights", || {
                let weights = match self.curve.pole_rows() {
                    NurbsPoles3::Polynomial { .. } => None,
                    NurbsPoles3::Rational { points } => Some(ctx.collect_vec(
                        points.iter().map(|pole| pole.weight.get()),
                        "collect Parasolid cylinder pole weights",
                    )?),
                };
                super::pcurves::quadratic_nurbs_has_constant_radius(
                    ctx,
                    &self.values,
                    weights.as_deref(),
                    self.curve.knots(),
                    radius,
                )
            })?;
        drop(weights_storage);
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
