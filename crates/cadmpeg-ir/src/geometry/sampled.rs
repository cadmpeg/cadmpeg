// SPDX-License-Identifier: Apache-2.0
//! Sampled curves and polygonal surfaces with checked sample layouts.

use crate::features::FinitePoint3;
use crate::math::Point3;
use crate::scalar::NonNegativeReal;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Structural error in a sampled polyline or polygonal carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeometryLayoutError {
    /// A sample or vertex layout the carrier cannot state. The carrier states
    /// this case; an outside caller states only [`Self::EditRefused`].
    #[non_exhaustive]
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

/// Admit a recorded chordal deviation that is finite and non-negative.
fn admit_chordal_deflection(
    chordal_deflection: f64,
) -> Result<NonNegativeReal, GeometryLayoutError> {
    NonNegativeReal::new(chordal_deflection)
        .ok_or_else(|| geometry_layout_error("chordal_deflection must be finite and non-negative"))
}

/// Admit polygonal-surface vertices whose every coordinate is finite.
fn admit_finite_vertices(vertices: Vec<Point3>) -> Result<Vec<FinitePoint3>, GeometryLayoutError> {
    vertices
        .into_iter()
        .map(FinitePoint3::new)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| geometry_layout_error("vertices must be finite"))
}

/// Source-native polygonal surface with an explicit chordal error bound.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PolygonalSurface {
    #[cfg_attr(feature = "schema", schemars(with = "Vec<Point3>"))]
    vertices: Vec<FinitePoint3>,
    triangles: Vec<[u32; 3]>,
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    chordal_deflection: NonNegativeReal,
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
        let vertices = admit_finite_vertices(vertices)?;
        let chordal_deflection = admit_chordal_deflection(chordal_deflection)?;
        Ok(Self {
            vertices,
            triangles,
            chordal_deflection,
        })
    }

    /// Edit finite vertices transactionally.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    /// The edit keeps the vertex count and the triangles, so only the edited
    /// coordinates are admitted again.
    pub fn edit_vertices(
        &mut self,
        edit: impl FnOnce(&mut [Point3]) -> Result<(), GeometryLayoutError>,
    ) -> Result<(), GeometryLayoutError> {
        let mut candidate: Vec<Point3> = self.vertices.iter().map(|point| point.get()).collect();
        edit(&mut candidate)?;
        self.vertices = admit_finite_vertices(candidate)?;
        Ok(())
    }

    /// Maximum chordal deviation recorded by the source.
    #[must_use]
    pub const fn chordal_deflection(&self) -> NonNegativeReal {
        self.chordal_deflection
    }

    /// Set the recorded chordal deviation.
    pub fn set_chordal_deflection(
        &mut self,
        chordal_deflection: f64,
    ) -> Result<(), GeometryLayoutError> {
        self.chordal_deflection = admit_chordal_deflection(chordal_deflection)?;
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
    chordal_deflection: NonNegativeReal,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PolylineCurveWire {
    /// The polyline's sample rows.
    samples: PolylineSamples,
    /// Maximum chordal deviation recorded by the source.
    chordal_deflection: f64,
}

impl From<PolylineCurve> for PolylineCurveWire {
    fn from(curve: PolylineCurve) -> Self {
        Self {
            samples: curve.samples,
            chordal_deflection: curve.chordal_deflection.get(),
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
    pub fn count(&self) -> usize {
        match self {
            Self::Unparameterized { points } => points.len(),
            Self::Parameterized { vertices } => vertices.len(),
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
        Self::admit_sample_points(&samples)?;
        let chordal_deflection = admit_chordal_deflection(chordal_deflection)?;
        Self::admit_sample_parameters(&samples)?;
        Ok(Self {
            samples,
            chordal_deflection,
        })
    }

    /// Admit at least two samples whose every coordinate is finite.
    fn admit_sample_points(samples: &PolylineSamples) -> Result<(), GeometryLayoutError> {
        if samples.count() < 2 {
            return Err(geometry_layout_error(
                "polyline must contain at least two points",
            ));
        }
        if samples.points().any(|point| !point.is_finite()) {
            return Err(geometry_layout_error("points must be finite"));
        }
        Ok(())
    }

    /// Admit source parameters that are finite and strictly monotonic.
    fn admit_sample_parameters(samples: &PolylineSamples) -> Result<(), GeometryLayoutError> {
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
        Ok(())
    }

    /// Ordered model-space samples.
    pub fn points(&self) -> impl Iterator<Item = Point3> + '_ {
        self.samples.points()
    }

    /// Number of samples.
    #[must_use]
    pub fn point_count(&self) -> usize {
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
        Self::admit_sample_points(&candidate)?;
        Self::admit_sample_parameters(&candidate)?;
        self.samples = candidate;
        Ok(())
    }

    /// Maximum chordal deviation recorded by the source.
    #[must_use]
    pub const fn chordal_deflection(&self) -> NonNegativeReal {
        self.chordal_deflection
    }

    /// Set the recorded chordal deviation.
    pub fn set_chordal_deflection(
        &mut self,
        chordal_deflection: f64,
    ) -> Result<(), GeometryLayoutError> {
        self.chordal_deflection = admit_chordal_deflection(chordal_deflection)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
