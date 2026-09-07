// SPDX-License-Identifier: Apache-2.0
//! Checked physical chart layouts and paired samples for solved charts.

use cadmpeg_ir::math::Point3;

/// At least two chart points, each with one native parameter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChartSamples {
    samples: crate::om::nonempty::NonEmpty<(Point3, f64)>,
}

impl ChartSamples {
    fn new(samples: impl IntoIterator<Item = (Point3, f64)>) -> Option<Self> {
        let samples = crate::om::nonempty::NonEmpty::new(samples)?;
        (samples.len() >= 2).then_some(Self { samples })
    }

    #[cfg(test)]
    pub(crate) fn from_test_values(
        points: Vec<Point3>,
        parameters: Vec<f64>,
    ) -> Result<Self, &'static str> {
        if points.len() != parameters.len() {
            return Err("parameters: one value per point required");
        }
        Self::new(points.into_iter().zip(parameters)).ok_or("points: at least two points required")
    }

    pub(crate) fn points(&self) -> Vec<Point3> {
        self.samples.iter().map(|sample| sample.0).collect()
    }

    pub(crate) fn parameters(&self) -> Vec<f64> {
        self.samples.iter().map(|sample| sample.1).collect()
    }

    pub(crate) fn endpoints(&self) -> [Point3; 2] {
        [self.samples.first().0, self.samples.last().0]
    }

    pub(crate) fn parameter_range(&self) -> [f64; 2] {
        [self.samples.first().1, self.samples.last().1]
    }

    /// Replace the parameterization when both charts have the same sample count.
    pub(super) fn replace_parameters_from(&mut self, other: &Self) -> bool {
        if self.samples.len() != other.samples.len() {
            return false;
        }
        let Some(replacement) = Self::new(
            self.samples
                .iter()
                .zip(other.samples.iter())
                .map(|(point, parameter)| (point.0, parameter.1)),
        ) else {
            return false;
        };
        *self = replacement;
        true
    }
}

/// Serialized value used by both missing-parameter error slots.
pub(crate) const MISSING_PARAMETER: f64 = -31_415_800_000_000.0;

/// Finite chart preamble with a nonzero scale and positive chordal error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChartPreamble {
    base_parameter: f64,
    base_scale: f64,
    chordal_error: f64,
    angular_error: f64,
}
impl ChartPreamble {
    pub(crate) fn new(
        base_parameter: f64,
        base_scale: f64,
        chordal_error: f64,
        angular_error: f64,
    ) -> Result<Self, &'static str> {
        if !base_parameter.is_finite() {
            return Err("base_parameter: must be finite");
        }
        if !base_scale.is_finite() || base_scale == 0.0 {
            return Err("base_scale: must be finite and nonzero");
        }
        if !chordal_error.is_finite() || chordal_error <= 0.0 {
            return Err("chordal_error: must be finite and positive");
        }
        if !angular_error.is_finite() {
            return Err("angular_error: must be finite");
        }
        Ok(Self {
            base_parameter,
            base_scale,
            chordal_error,
            angular_error,
        })
    }
    pub(crate) fn base_parameter(self) -> f64 {
        self.base_parameter
    }
    pub(crate) fn base_scale(self) -> f64 {
        self.base_scale
    }
    pub(crate) fn chordal_error(self) -> f64 {
        self.chordal_error
    }
    pub(crate) fn angular_error(self) -> f64 {
        self.angular_error
    }
}

#[derive(Debug, Clone, PartialEq)]
enum SourceEncoding {
    Xyz3,
    Ext11 {
        parameters: Vec<f64>,
        support_uv: super::SupportUv,
    },
}

/// At least two finite source points with the fields required by their Hvec layout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceChartData {
    points: Vec<Point3>,
    encoding: SourceEncoding,
}
impl SourceChartData {
    fn checked(points: Vec<Point3>, encoding: SourceEncoding) -> Result<Self, &'static str> {
        u32::try_from(points.len()).map_err(|_| "points: count exceeds u32")?;
        if points.len() < 2 {
            return Err("points: at least two points required");
        }
        if !points.iter().all(|point| {
            [point.x, point.y, point.z]
                .iter()
                .all(|value| value.is_finite())
        }) {
            return Err("points: coordinates must be finite");
        }
        Ok(Self { points, encoding })
    }

    pub(crate) fn xyz3(points: Vec<Point3>) -> Result<Self, &'static str> {
        if !points.windows(2).any(|pair| pair[0] != pair[1]) {
            return Err("points: xyz3 requires distinct points");
        }
        Self::checked(points, SourceEncoding::Xyz3)
    }

    pub(crate) fn ext11(
        points: Vec<Point3>,
        parameters: Vec<f64>,
        support_uv: [Option<Vec<[f64; 2]>>; 2],
    ) -> Result<Self, &'static str> {
        if parameters.len() != points.len() {
            return Err("native_parameters: one value per point required");
        }
        if !parameters.iter().all(|value| value.is_finite())
            || parameters.windows(2).any(|pair| pair[1] <= pair[0])
        {
            return Err("native_parameters: finite strictly increasing values required");
        }
        let support_uv = support_uv.map(|lane| {
            lane.map(|values| {
                super::SupportUvLane::new(values, points.len())
                    .ok_or("ext_support_uv: one pair per point required")
            })
            .transpose()
        });
        let [first, second] = support_uv;
        let support_uv = [first?, second?];
        for lane in support_uv.iter().flatten() {
            if !lane
                .iter()
                .flatten()
                .all(|value| value.is_finite() && *value != MISSING_PARAMETER)
            {
                return Err("ext_support_uv: finite present parameter values required");
            }
        }
        Self::checked(
            points,
            SourceEncoding::Ext11 {
                parameters,
                support_uv,
            },
        )
    }

    pub(crate) fn points(&self) -> &[Point3] {
        &self.points
    }
    pub(crate) fn count(&self) -> u32 {
        self.points.len() as u32
    }
    pub(crate) fn point_layout(&self) -> super::ChartPointLayout {
        match self.encoding {
            SourceEncoding::Xyz3 => super::ChartPointLayout::Xyz3,
            SourceEncoding::Ext11 { .. } => super::ChartPointLayout::Ext11,
        }
    }
    pub(crate) fn native_parameters(&self) -> Option<&[f64]> {
        match &self.encoding {
            SourceEncoding::Xyz3 => None,
            SourceEncoding::Ext11 { parameters, .. } => Some(parameters),
        }
    }
    pub(crate) fn support_uv(&self) -> super::SupportUv {
        match &self.encoding {
            SourceEncoding::Xyz3 => [None, None],
            SourceEncoding::Ext11 { support_uv, .. } => support_uv.clone(),
        }
    }

    pub(crate) fn into_samples(
        self,
        preamble: ChartPreamble,
    ) -> Option<(ChartSamples, super::SupportUv)> {
        let (parameters, support_uv) = match self.encoding {
            SourceEncoding::Xyz3 => {
                let mut parameter = preamble.base_parameter();
                let parameters = std::iter::once(parameter)
                    .chain(self.points.windows(2).map(|pair| {
                        let chord_m = super::distance(pair[0], pair[1]) / 1000.0;
                        parameter += chord_m * preamble.base_scale();
                        parameter
                    }))
                    .collect();
                (parameters, [None, None])
            }
            SourceEncoding::Ext11 {
                parameters,
                support_uv,
            } => (parameters, support_uv),
        };
        Some((
            ChartSamples::new(self.points.into_iter().zip(parameters))?,
            support_uv,
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_samples_require_paired_values_and_two_endpoints() {
        let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        let make_samples = |points, parameters| {
            SourceChartData::ext11(points, parameters, [None, None]).map(|data| {
                data.into_samples(ChartPreamble::new(0.0, 1.0, 0.01, 0.0).unwrap())
                    .unwrap()
                    .0
            })
        };
        assert!(make_samples(Vec::new(), Vec::new()).is_err());
        assert!(make_samples(vec![points[0]], vec![0.0]).is_err());
        assert!(make_samples(points.clone(), vec![0.0]).is_err());
        assert!(make_samples(points.clone(), vec![0.0, 1.0, 2.0]).is_err());
        let samples = make_samples(points.clone(), vec![2.0, 5.0]).unwrap();
        assert_eq!(samples.points(), points);
        assert_eq!(samples.parameters(), [2.0, 5.0]);
        assert_eq!(samples.endpoints(), [points[0], points[1]]);
        assert_eq!(samples.parameter_range(), [2.0, 5.0]);
    }
    #[test]
    fn source_layouts_own_parameter_and_uv_constraints() {
        let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
        assert!(SourceChartData::xyz3(vec![points[0]; 2]).is_err());
        assert!(
            SourceChartData::xyz3(vec![points[0], Point3::new(f64::INFINITY, 0.0, 0.0)]).is_err()
        );
        assert!(SourceChartData::ext11(points.clone(), vec![2.0, 1.0], [None, None]).is_err());
        assert!(SourceChartData::ext11(
            points.clone(),
            vec![1.0, 2.0],
            [Some(vec![[0.0; 2]]), None]
        )
        .is_err());
        assert!(SourceChartData::ext11(
            points.clone(),
            vec![1.0, 2.0],
            [Some(vec![[MISSING_PARAMETER; 2]; 2]), None]
        )
        .is_err());
        let xyz = SourceChartData::xyz3(points.clone()).unwrap();
        let (samples, uv) = xyz
            .into_samples(ChartPreamble::new(2.0, 1000.0, 0.01, 0.0).unwrap())
            .unwrap();
        assert_eq!(samples.points(), points);
        assert_eq!(samples.parameters(), [2.0, 3.0]);
        assert_eq!(uv, [None, None]);
    }
}
