// SPDX-License-Identifier: Apache-2.0
//! Paired model-space points and native parameters for a solved chart.

use cadmpeg_ir::math::Point3;

/// At least two chart points, each with one native parameter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChartSamples {
    points: Vec<Point3>,
    parameters: Vec<f64>,
}

impl ChartSamples {
    pub(crate) fn new(points: Vec<Point3>, parameters: Vec<f64>) -> Result<Self, &'static str> {
        if points.len() < 2 {
            return Err("ChartSamples.points requires at least two points");
        }
        if points.len() != parameters.len() {
            return Err("ChartSamples.parameters must have one value per point");
        }
        Ok(Self { points, parameters })
    }

    pub(crate) fn points(&self) -> &[Point3] {
        &self.points
    }

    pub(crate) fn parameters(&self) -> &[f64] {
        &self.parameters
    }

    pub(crate) fn endpoints(&self) -> [Point3; 2] {
        [self.points[0], self.points[self.points.len() - 1]]
    }

    pub(crate) fn parameter_range(&self) -> [f64; 2] {
        [
            self.parameters[0],
            self.parameters[self.parameters.len() - 1],
        ]
    }

    /// Replace the parameterization when both charts have the same sample count.
    pub(super) fn replace_parameters_from(&mut self, other: &Self) -> bool {
        if self.points.len() != other.points.len() {
            return false;
        }
        self.parameters.clone_from(&other.parameters);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_samples_require_paired_values_and_two_endpoints() {
        let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        assert!(ChartSamples::new(Vec::new(), Vec::new()).is_err());
        assert!(ChartSamples::new(vec![points[0]], vec![0.0]).is_err());
        assert!(ChartSamples::new(points.clone(), vec![0.0]).is_err());
        assert!(ChartSamples::new(points.clone(), vec![0.0, 1.0, 2.0]).is_err());
        let samples = ChartSamples::new(points.clone(), vec![2.0, 5.0]).unwrap();
        assert_eq!(samples.points(), points);
        assert_eq!(samples.parameters(), [2.0, 5.0]);
        assert_eq!(samples.endpoints(), [points[0], points[1]]);
        assert_eq!(samples.parameter_range(), [2.0, 5.0]);
    }
}
