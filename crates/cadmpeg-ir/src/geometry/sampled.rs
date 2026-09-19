// SPDX-License-Identifier: Apache-2.0
//! Sampled curves and polygonal surfaces with checked sample layouts.

use crate::math::Point3;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Structural error in a sampled polyline or polygonal carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeometryLayoutError {
    /// A sample or vertex layout the carrier cannot state.
    Layout(String),
    /// An edit closure refused the value it was given, stating its own reason.
    EditRefused(String),
}

impl std::fmt::Display for GeometryLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Layout(message) | Self::EditRefused(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for GeometryLayoutError {}

fn geometry_layout_error(message: impl Into<String>) -> GeometryLayoutError {
    GeometryLayoutError::Layout(message.into())
}

/// Source-native polygonal surface with an explicit chordal error bound.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PolygonalSurface {
    vertices: Vec<Point3>,
    triangles: Vec<[u32; 3]>,
    chordal_deflection: f64,
}

impl PolygonalSurface {
    /// Build a polygonal surface whose triangle indices address `vertices`.
    pub fn new(
        vertices: Vec<Point3>,
        triangles: Vec<[u32; 3]>,
        chordal_deflection: f64,
    ) -> Result<Self, GeometryLayoutError> {
        if vertices.len() < 3 {
            return Err(geometry_layout_error(
                "polygonal surface must contain at least three vertices",
            ));
        }
        if triangles.is_empty() {
            return Err(geometry_layout_error(
                "polygonal surface must contain at least one triangle",
            ));
        }
        if triangles
            .iter()
            .flatten()
            .any(|index| *index as usize >= vertices.len())
        {
            return Err(geometry_layout_error(
                "polygonal surface contains an out-of-range triangle index",
            ));
        }
        if vertices.iter().any(|point| !point.is_finite()) {
            return Err(geometry_layout_error("vertices must be finite"));
        }
        if !chordal_deflection.is_finite() || chordal_deflection < 0.0 {
            return Err(geometry_layout_error(
                "chordal_deflection must be finite and non-negative",
            ));
        }
        Ok(Self {
            vertices,
            triangles,
            chordal_deflection,
        })
    }

    /// Ordered model-space vertices.
    #[must_use]
    pub fn vertices(&self) -> &[Point3] {
        &self.vertices
    }

    /// Edit finite vertices transactionally.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_vertices(
        &mut self,
        edit: impl FnOnce(&mut [Point3]) -> Result<(), GeometryLayoutError>,
    ) -> Result<(), GeometryLayoutError> {
        let mut candidate = self.vertices.clone();
        edit(&mut candidate)?;
        *self = Self::new(candidate, self.triangles.clone(), self.chordal_deflection)?;
        Ok(())
    }

    /// Zero-based triangle indices into [`Self::vertices`].
    #[must_use]
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    /// Maximum chordal deviation recorded by the source.
    #[must_use]
    pub const fn chordal_deflection(&self) -> f64 {
        self.chordal_deflection
    }

    /// Set the recorded chordal deviation.
    pub fn set_chordal_deflection(
        &mut self,
        chordal_deflection: f64,
    ) -> Result<(), GeometryLayoutError> {
        if !chordal_deflection.is_finite() || chordal_deflection < 0.0 {
            return Err(geometry_layout_error(
                "chordal_deflection must be finite and non-negative",
            ));
        }
        self.chordal_deflection = chordal_deflection;
        Ok(())
    }
}

impl<'de> Deserialize<'de> for PolygonalSurface {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            vertices: Vec<Point3>,
            triangles: Vec<[u32; 3]>,
            chordal_deflection: f64,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.vertices, wire.triangles, wire.chordal_deflection)
            .map_err(serde::de::Error::custom)
    }
}

/// One polyline sample with the source parameter recorded at it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PolylineVertex {
    /// Source parameter at this sample.
    pub parameter: f64,
    /// Model-space sample.
    pub point: Point3,
}

/// The samples of a polyline, with or without source parameters.
///
/// A parameterized polyline carries one parameter per sample in the sample
/// row, so a parameter list that does not match the sample count has no
/// spelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PolylineSamples {
    /// Samples the source did not parameterize.
    Unparameterized {
        /// Ordered model-space samples.
        points: crate::features::NonEmptyMembers<Point3>,
    },
    /// Samples the source parameterized.
    Parameterized {
        /// Ordered samples, each with its source parameter.
        vertices: crate::features::NonEmptyMembers<PolylineVertex>,
    },
}

/// Source-native polyline with an explicit chordal error bound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "PolylineCurveWire"))]
#[serde(try_from = "PolylineCurveWire", into = "PolylineCurveWire")]
pub struct PolylineCurve {
    samples: PolylineSamples,
    chordal_deflection: f64,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PolylineCurveWire {
    samples: PolylineSamples,
    chordal_deflection: f64,
}

impl From<PolylineCurve> for PolylineCurveWire {
    fn from(curve: PolylineCurve) -> Self {
        Self {
            samples: curve.samples,
            chordal_deflection: curve.chordal_deflection,
        }
    }
}

impl TryFrom<PolylineCurveWire> for PolylineCurve {
    type Error = GeometryLayoutError;

    fn try_from(wire: PolylineCurveWire) -> Result<Self, Self::Error> {
        Self::new(wire.samples, wire.chordal_deflection)
    }
}

impl PolylineSamples {
    /// Number of samples. The sample list is nonempty by type.
    #[must_use]
    pub fn count(&self) -> std::num::NonZeroUsize {
        match self {
            Self::Unparameterized { points } => points.count(),
            Self::Parameterized { vertices } => vertices.count(),
        }
    }

    /// Ordered model-space samples.
    pub fn points(&self) -> impl Iterator<Item = Point3> + '_ {
        let (points, vertices) = match self {
            Self::Unparameterized { points } => (Some(points), None),
            Self::Parameterized { vertices } => (None, Some(vertices)),
        };
        points
            .into_iter()
            .flatten()
            .copied()
            .chain(vertices.into_iter().flatten().map(|vertex| vertex.point))
    }

    /// Source parameters, absent when the source stated none.
    pub fn parameters(&self) -> Option<impl Iterator<Item = f64> + '_> {
        match self {
            Self::Unparameterized { .. } => None,
            Self::Parameterized { vertices } => {
                Some(vertices.iter().map(|vertex| vertex.parameter))
            }
        }
    }

    /// Edit each sample point, keeping the prior points on a refusal.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_points(
        &mut self,
        mut edit: impl FnMut(&mut Point3) -> Result<(), GeometryLayoutError>,
    ) -> Result<(), GeometryLayoutError> {
        let mut candidate = self.clone();
        match &mut candidate {
            Self::Unparameterized { points } => {
                for point in points.iter_mut() {
                    edit(point)?;
                }
            }
            Self::Parameterized { vertices } => {
                for vertex in vertices.iter_mut() {
                    edit(&mut vertex.point)?;
                }
            }
        }
        *self = candidate;
        Ok(())
    }
}

impl PolylineCurve {
    /// Build a polyline from its sample rows.
    ///
    /// A parameterized sample carries its parameter in the row, so there is no
    /// second list to pair up and no count to compare.
    pub fn new(
        samples: PolylineSamples,
        chordal_deflection: f64,
    ) -> Result<Self, GeometryLayoutError> {
        if samples.count().get() < 2 {
            return Err(geometry_layout_error(
                "polyline must contain at least two points",
            ));
        }
        if samples.points().any(|point| !point.is_finite()) {
            return Err(geometry_layout_error("points must be finite"));
        }
        if !chordal_deflection.is_finite() || chordal_deflection < 0.0 {
            return Err(geometry_layout_error(
                "chordal_deflection must be finite and non-negative",
            ));
        }
        if let Some(parameters) = samples.parameters() {
            let parameters: Vec<f64> = parameters.collect();
            if !parameters.iter().all(|value| value.is_finite())
                || !(parameters.windows(2).all(|pair| pair[0] < pair[1])
                    || parameters.windows(2).all(|pair| pair[0] > pair[1]))
            {
                return Err(geometry_layout_error(
                    "parameters must be finite and strictly monotonic",
                ));
            }
        }
        Ok(Self {
            samples,
            chordal_deflection,
        })
    }

    /// The polyline's sample rows.
    #[must_use]
    pub const fn samples(&self) -> &PolylineSamples {
        &self.samples
    }

    /// Ordered model-space samples.
    pub fn points(&self) -> impl Iterator<Item = Point3> + '_ {
        self.samples.points()
    }

    /// Number of samples.
    #[must_use]
    pub fn point_count(&self) -> std::num::NonZeroUsize {
        self.samples.count()
    }

    /// Source parameters, absent when the source stated none.
    pub fn parameters(&self) -> Option<impl Iterator<Item = f64> + '_> {
        self.samples.parameters()
    }

    /// Edit the sample rows transactionally.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_samples(
        &mut self,
        edit: impl FnOnce(&mut PolylineSamples) -> Result<(), GeometryLayoutError>,
    ) -> Result<(), GeometryLayoutError> {
        let mut candidate = self.samples.clone();
        edit(&mut candidate)?;
        *self = Self::new(candidate, self.chordal_deflection)?;
        Ok(())
    }

    /// Maximum chordal deviation recorded by the source.
    #[must_use]
    pub const fn chordal_deflection(&self) -> f64 {
        self.chordal_deflection
    }

    /// Set the recorded chordal deviation.
    pub fn set_chordal_deflection(
        &mut self,
        chordal_deflection: f64,
    ) -> Result<(), GeometryLayoutError> {
        if !chordal_deflection.is_finite() || chordal_deflection < 0.0 {
            return Err(geometry_layout_error(
                "chordal_deflection must be finite and non-negative",
            ));
        }
        self.chordal_deflection = chordal_deflection;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
