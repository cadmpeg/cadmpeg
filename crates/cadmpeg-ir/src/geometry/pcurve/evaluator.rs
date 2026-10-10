// SPDX-License-Identifier: Apache-2.0
//! Scoped raw pcurve lanes for geometry evaluation.

use cadmpeg_core::decode::{DecodeContext, ResourceLimit, ScopedReservation};

use super::PcurveNurbsPoles;
use crate::math::Point2;
use crate::units::FinitePoint2;

/// Evaluator copies whose reservation lives until both lanes are dropped.
#[derive(Debug)]
pub struct PcurveEvaluatorLanes<'ctx> {
    points: Vec<Point2>,
    weights: Option<Vec<f64>>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx> PcurveEvaluatorLanes<'ctx> {
    /// Copy positions and rational weights through the caller's scoped storage.
    pub fn new(
        ctx: &'ctx DecodeContext<'_>,
        poles: &PcurveNurbsPoles<FinitePoint2>,
        point_operation: &'static str,
        weight_operation: &'static str,
    ) -> Result<Self, ResourceLimit> {
        let mut storage = ctx.reserve_scoped_limit(0, point_operation)?;
        let mut points = Vec::new();
        ctx.reserve_scoped_vec_limit(&mut storage, &mut points, poles.count(), point_operation)?;
        let mut weights = None;
        match poles {
            PcurveNurbsPoles::Polynomial { points: source } => {
                for point in ctx.admit_iter(source, point_operation)? {
                    points.push(point.get());
                }
            }
            PcurveNurbsPoles::Rational { points: source } => {
                for pole in ctx.admit_iter(source, point_operation)? {
                    points.push(pole.point.get());
                }
                let mut copied = Vec::new();
                ctx.reserve_scoped_vec_limit(
                    &mut storage,
                    &mut copied,
                    source.len(),
                    weight_operation,
                )?;
                for pole in ctx.admit_iter(source, weight_operation)? {
                    copied.push(pole.weight.get());
                }
                weights = Some(copied);
            }
        }
        Ok(Self {
            points,
            weights,
            _storage: storage,
        })
    }

    /// Raw positions in source order.
    pub fn points(&self) -> &[Point2] {
        &self.points
    }

    /// Raw rational weights in source order, absent for polynomial poles.
    pub fn weights(&self) -> Option<&[f64]> {
        self.weights.as_deref()
    }
}

#[cfg(test)]
mod tests;
