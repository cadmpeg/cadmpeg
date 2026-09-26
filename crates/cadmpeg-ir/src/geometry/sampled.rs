// SPDX-License-Identifier: Apache-2.0
//! Sampled curves and polygonal surfaces with checked sample layouts.

use crate::features::FinitePoint3;
use crate::math::Point3;
use crate::scalar::{FiniteReal, NonNegativeReal, PositiveReal};
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

    /// The surface with every vertex and the chordal deviation times `scale`.
    ///
    /// The vertex count and the triangles are kept, and a positive scale
    /// keeps the deviation non-negative, so a scaled vertex or deviation is
    /// refused only when it overflows, vertices first.
    pub(crate) fn scaled(&self, scale: PositiveReal) -> Result<Self, GeometryLayoutError> {
        let vertices = self
            .vertices
            .iter()
            .map(|vertex| vertex.scaled(scale))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| geometry_layout_error("vertices must be finite"))?;
        Ok(Self {
            vertices,
            triangles: self.triangles.clone(),
            chordal_deflection: scaled_chordal_deflection(self.chordal_deflection, scale)?,
        })
    }
}

/// A recorded chordal deviation times `scale`, refused only when it
/// overflows.
fn scaled_chordal_deflection(
    chordal_deflection: NonNegativeReal,
    scale: PositiveReal,
) -> Result<NonNegativeReal, GeometryLayoutError> {
    chordal_deflection
        .scaled(scale)
        .ok_or_else(|| geometry_layout_error("chordal_deflection must be finite and non-negative"))
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
// A source states a raw parameter and point; a `PolylineCurve` holds the
// admitted sample, whose parameter is a `FiniteReal` and whose point is a
// `FinitePoint3`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PolylineVertex<R = f64, P = Point3> {
    /// Source parameter at this sample.
    pub parameter: R,
    /// Model-space sample.
    pub point: P,
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
pub enum PolylineSamples<R = f64, P = Point3> {
    /// Samples the source did not parameterize.
    Unparameterized {
        /// Ordered model-space samples.
        points: crate::features::NonEmptyMembers<P>,
    },
    /// Samples the source parameterized.
    Parameterized {
        /// Ordered samples, each with its source parameter.
        vertices: crate::features::NonEmptyMembers<PolylineVertex<R, P>>,
    },
}

/// Source-native polyline with an explicit chordal error bound.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "PolylineCurveWire"))]
#[serde(try_from = "PolylineCurveWire")]
pub struct PolylineCurve {
    samples: PolylineSamples<FiniteReal, FinitePoint3>,
    chordal_deflection: NonNegativeReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PolylineCurveWire {
    /// The polyline's sample rows.
    samples: PolylineSamples,
    /// Maximum chordal deviation recorded by the source.
    chordal_deflection: f64,
}

/// The written form of a polyline, borrowing its admitted samples.
#[derive(Serialize)]
struct PolylineCurveWriteWire<'a> {
    samples: &'a PolylineSamples<FiniteReal, FinitePoint3>,
    chordal_deflection: NonNegativeReal,
}

impl Serialize for PolylineCurve {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PolylineCurveWriteWire {
            samples: &self.samples,
            chordal_deflection: self.chordal_deflection,
        }
        .serialize(serializer)
    }
}

impl TryFrom<PolylineCurveWire> for PolylineCurve {
    type Error = GeometryLayoutError;

    fn try_from(wire: PolylineCurveWire) -> Result<Self, Self::Error> {
        Self::new(wire.samples, wire.chordal_deflection)
    }
}

impl<R, P> PolylineSamples<R, P> {
    /// Number of samples. The sample list is nonempty by type.
    #[must_use]
    pub fn count(&self) -> usize {
        match self {
            Self::Unparameterized { points } => points.len(),
            Self::Parameterized { vertices } => vertices.len(),
        }
    }
}

impl<R: Copy, P: Copy> PolylineSamples<R, P> {
    /// Ordered model-space samples.
    pub fn points(&self) -> impl Iterator<Item = P> + '_ {
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
    pub fn parameters(&self) -> Option<impl Iterator<Item = R> + '_> {
        match self {
            Self::Unparameterized { .. } => None,
            Self::Parameterized { vertices } => {
                Some(vertices.iter().map(|vertex| vertex.parameter))
            }
        }
    }
}

impl PolylineSamples {
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

    /// The samples with admitted points, absent when a coordinate is not
    /// finite.
    fn admit_points(self) -> Option<PolylineSamples<f64, FinitePoint3>> {
        Some(match self {
            Self::Unparameterized { points } => PolylineSamples::Unparameterized {
                points: points.try_map(FinitePoint3::new)?,
            },
            Self::Parameterized { vertices } => PolylineSamples::Parameterized {
                vertices: vertices.try_map(|vertex| {
                    Some(PolylineVertex {
                        parameter: vertex.parameter,
                        point: FinitePoint3::new(vertex.point)?,
                    })
                })?,
            },
        })
    }
}

impl PolylineSamples<f64, FinitePoint3> {
    /// The samples with admitted parameters, absent when a parameter is not
    /// finite or the parameters are not strictly monotonic.
    fn admit_parameters(self) -> Option<PolylineSamples<FiniteReal, FinitePoint3>> {
        match self {
            Self::Unparameterized { points } => Some(PolylineSamples::Unparameterized { points }),
            Self::Parameterized { vertices } => {
                let vertices = vertices.try_map(|vertex| {
                    Some(PolylineVertex {
                        parameter: FiniteReal::new(vertex.parameter)?,
                        point: vertex.point,
                    })
                })?;
                (vertices
                    .windows(2)
                    .all(|pair| pair[0].parameter < pair[1].parameter)
                    || vertices
                        .windows(2)
                        .all(|pair| pair[0].parameter > pair[1].parameter))
                .then_some(PolylineSamples::Parameterized { vertices })
            }
        }
    }
}

impl PolylineSamples<FiniteReal, FinitePoint3> {
    /// The samples with raw parameters and points.
    #[must_use]
    pub fn to_raw(&self) -> PolylineSamples {
        match self {
            Self::Unparameterized { points } => PolylineSamples::Unparameterized {
                points: points.clone().map(FinitePoint3::get),
            },
            Self::Parameterized { vertices } => PolylineSamples::Parameterized {
                vertices: vertices.clone().map(|vertex| PolylineVertex {
                    parameter: vertex.parameter.get(),
                    point: vertex.point.get(),
                }),
            },
        }
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
        let samples = Self::admit_sample_points(samples)?;
        let chordal_deflection = admit_chordal_deflection(chordal_deflection)?;
        let samples = Self::admit_sample_parameters(samples)?;
        Ok(Self {
            samples,
            chordal_deflection,
        })
    }

    /// Admit at least two samples whose every coordinate is finite.
    fn admit_sample_points(
        samples: PolylineSamples,
    ) -> Result<PolylineSamples<f64, FinitePoint3>, GeometryLayoutError> {
        if samples.count() < 2 {
            return Err(geometry_layout_error(
                "polyline must contain at least two points",
            ));
        }
        samples
            .admit_points()
            .ok_or_else(|| geometry_layout_error("points must be finite"))
    }

    /// Admit source parameters that are finite and strictly monotonic.
    fn admit_sample_parameters(
        samples: PolylineSamples<f64, FinitePoint3>,
    ) -> Result<PolylineSamples<FiniteReal, FinitePoint3>, GeometryLayoutError> {
        samples.admit_parameters().ok_or_else(|| {
            geometry_layout_error("parameters must be finite and strictly monotonic")
        })
    }

    /// Ordered admitted model-space samples.
    pub fn points(&self) -> impl Iterator<Item = FinitePoint3> + '_ {
        self.samples.points()
    }

    /// Number of samples.
    #[must_use]
    pub fn point_count(&self) -> usize {
        self.samples.count()
    }

    /// One admitted point without collecting the sample lane.
    #[must_use]
    pub fn point_at(&self, index: usize) -> Option<FinitePoint3> {
        match &self.samples {
            PolylineSamples::Unparameterized { points } => points.get(index).copied(),
            PolylineSamples::Parameterized { vertices } => vertices.get(index).map(|row| row.point),
        }
    }

    /// One source parameter, or the sample index when none was stated.
    #[must_use]
    pub fn parameter_at(&self, index: usize) -> Option<FiniteReal> {
        match &self.samples {
            PolylineSamples::Unparameterized { points } => {
                points.get(index).map(|_| FiniteReal::from_index(index))
            }
            PolylineSamples::Parameterized { vertices } => {
                vertices.get(index).map(|row| row.parameter)
            }
        }
    }

    /// Admitted source parameters, absent when the source stated none.
    pub fn parameters(&self) -> Option<impl Iterator<Item = FiniteReal> + '_> {
        self.samples.parameters()
    }

    /// Edit the raw sample rows transactionally and admit the result.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_samples(
        &mut self,
        edit: impl FnOnce(&mut PolylineSamples) -> Result<(), GeometryLayoutError>,
    ) -> Result<(), GeometryLayoutError> {
        let mut candidate = self.samples.to_raw();
        edit(&mut candidate)?;
        self.samples = Self::admit_sample_parameters(Self::admit_sample_points(candidate)?)?;
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

    /// The polyline with every sample point and the chordal deviation times
    /// `scale`.
    ///
    /// The sample count and the source parameters are kept, and a positive
    /// scale keeps the deviation non-negative, so a scaled point or deviation
    /// is refused only when it overflows, points first.
    pub(crate) fn scaled(&self, scale: PositiveReal) -> Result<Self, GeometryLayoutError> {
        let point = |point: FinitePoint3| point.scaled(scale);
        let samples = match self.samples.clone() {
            PolylineSamples::Unparameterized { points } => points
                .try_map(point)
                .map(|points| PolylineSamples::Unparameterized { points }),
            PolylineSamples::Parameterized { vertices } => vertices
                .try_map(|vertex| {
                    Some(PolylineVertex {
                        parameter: vertex.parameter,
                        point: point(vertex.point)?,
                    })
                })
                .map(|vertices| PolylineSamples::Parameterized { vertices }),
        }
        .ok_or_else(|| geometry_layout_error("points must be finite"))?;
        Ok(Self {
            samples,
            chordal_deflection: scaled_chordal_deflection(self.chordal_deflection, scale)?,
        })
    }
}

#[cfg(test)]
mod tests;
