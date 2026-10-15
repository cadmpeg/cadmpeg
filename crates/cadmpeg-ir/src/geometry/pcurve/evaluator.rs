// SPDX-License-Identifier: Apache-2.0
//! Raw pcurve lanes for geometry evaluation.

use super::PcurveNurbsPoles;
use crate::math::Point2;
use crate::units::FinitePoint2;

/// Raw evaluator copies of a pcurve's positions and weights.
#[derive(Debug)]
pub struct PcurveEvaluatorLanes {
    points: Vec<Point2>,
    weights: Option<Vec<f64>>,
}

impl PcurveEvaluatorLanes {
    /// Copy positions and rational weights.
    pub fn new(poles: &PcurveNurbsPoles<FinitePoint2>) -> Self {
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
        Self { points, weights }
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
