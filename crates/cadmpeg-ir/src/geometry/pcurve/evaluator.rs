// SPDX-License-Identifier: Apache-2.0
//! Raw pcurve lanes for geometry evaluation.

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};

use super::PcurveNurbsPoles;
use crate::math::Point2;
use crate::units::FinitePoint2;

/// Raw evaluator copies of a pcurve's positions and weights.
#[derive(Debug)]
pub struct PcurveEvaluatorLanes<'ctx> {
    points: Vec<Point2>,
    weights: Option<Vec<f64>>,
    _ctx: std::marker::PhantomData<&'ctx ()>,
}

impl<'ctx> PcurveEvaluatorLanes<'ctx> {
    /// Copy positions and rational weights.
    pub fn new(
        _ctx: &'ctx DecodeContext<'_>,
        poles: &PcurveNurbsPoles<FinitePoint2>,
        _point_operation: &'static str,
        _weight_operation: &'static str,
    ) -> Result<Self, ResourceLimit> {
        let mut points = Vec::new();
        let mut weights = None;
        match poles {
            PcurveNurbsPoles::Polynomial { points: source } => {
                for point in source {
                    points.push(point.get());
                }
            }
            PcurveNurbsPoles::Rational { points: source } => {
                for pole in source {
                    points.push(pole.point.get());
                }
                let mut copied = Vec::new();
                for pole in source {
                    copied.push(pole.weight.get());
                }
                weights = Some(copied);
            }
        }
        Ok(Self {
            points,
            weights,
            _ctx: std::marker::PhantomData,
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
