// SPDX-License-Identifier: Apache-2.0
//! Sampled curves and polygonal surfaces with checked sample layouts.

mod admission;
use admission::{SampledAdmission, StandardAdmission};

use crate::features::FinitePoint3;
use crate::math::Point3;
use crate::scalar::{FiniteReal, NonNegativeReal, PositiveReal};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
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
fn admit_finite_vertices<A: SampledAdmission>(
    admission: &A,
    vertices: Vec<Point3>,
) -> Result<Vec<FinitePoint3>, A::Error> {
    admission.collect(
        vertices,
        "IR polygonal admitted vertices",
        |point| match FinitePoint3::new(point) {
            Some(point) => Ok(point),
            None => Err(admission.layout("vertices must be finite")?),
        },
    )
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

decode_cost_record!(
    [] PolygonalSurface;
    Self { vertices, triangles, chordal_deflection } => [vertices:  Vec<FinitePoint3>, triangles:  Vec<[u32; 3]>, chordal_deflection:  NonNegativeReal]
);

impl PolygonalSurface {
    /// Admitted polygon vertices in source order.
    pub fn vertices(&self) -> &[FinitePoint3] {
        &self.vertices
    }

    /// Triangle vertex indexes in source order.
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    /// Copy both sampled lanes through the decode collection budget.
    pub fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            vertices: super::copy_decode_slice(&self.vertices, ctx, operation)?,
            triangles: super::copy_decode_slice(&self.triangles, ctx, operation)?,
            chordal_deflection: self.chordal_deflection,
        })
    }

    /// Build a polygonal surface whose triangle indices address `vertices`.
    pub fn new(
        vertices: Vec<Point3>,
        triangles: Vec<[u32; 3]>,
        chordal_deflection: f64,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<Self, GeometryLayoutError>, CodecError> {
        admission::finish(Self::build(
            ctx,
            vertices,
            triangles,
            |vertices| admit_finite_vertices(ctx, vertices),
            || NonNegativeReal::new(chordal_deflection),
        ))
    }

    /// Build from admitted source deflection and placement scale.
    pub fn from_scaled_deflection(
        vertices: Vec<Point3>,
        triangles: Vec<[u32; 3]>,
        chordal_deflection: NonNegativeReal,
        scale: PositiveReal,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<Self, GeometryLayoutError>, CodecError> {
        admission::finish(Self::build(
            ctx,
            vertices,
            triangles,
            |vertices| admit_finite_vertices(ctx, vertices),
            || chordal_deflection.scaled(scale),
        ))
    }

    /// Build from admitted vertices, source deflection and placement scale.
    /// Only triangle relationships and scaled deflection remain to check.
    pub fn from_admitted_scaled_deflection(
        vertices: Vec<FinitePoint3>,
        triangles: Vec<[u32; 3]>,
        chordal_deflection: NonNegativeReal,
        scale: PositiveReal,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<Self, GeometryLayoutError>, CodecError> {
        admission::finish(Self::build(ctx, vertices, triangles, Ok, || {
            chordal_deflection.scaled(scale)
        }))
    }

    fn build<A: SampledAdmission, P>(
        admission: &A,
        vertices: Vec<P>,
        triangles: Vec<[u32; 3]>,
        convert: impl FnOnce(Vec<P>) -> Result<Vec<FinitePoint3>, A::Error>,
        deflection: impl FnOnce() -> Option<NonNegativeReal>,
    ) -> Result<Self, A::Error> {
        Self::check_layout(admission, vertices.len(), &triangles)?;
        let vertices = convert(vertices)?;
        let Some(chordal_deflection) = deflection() else {
            return Err(admission.layout("chordal_deflection must be finite and non-negative")?);
        };
        Ok(Self {
            vertices,
            triangles,
            chordal_deflection,
        })
    }

    fn check_layout<A: SampledAdmission>(
        admission: &A,
        vertex_count: usize,
        triangles: &[[u32; 3]],
    ) -> Result<(), A::Error> {
        if vertex_count < 3 {
            return Err(admission.layout("polygonal surface must contain at least three vertices")?);
        }
        if triangles.is_empty() {
            return Err(admission.layout("polygonal surface must contain at least one triangle")?);
        }
        for index in triangles.iter().flatten() {
            admission.work(1, "IR polygonal triangle index")?;
            if usize::try_from(*index).map_or(true, |index| index >= vertex_count) {
                return Err(admission
                    .layout("polygonal surface contains an out-of-range triangle index")?);
            }
        }
        Ok(())
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
        self.vertices = admit_finite_vertices(&StandardAdmission, candidate)?;
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
        Self::build(
            &StandardAdmission,
            wire.vertices,
            wire.triangles,
            |vertices| admit_finite_vertices(&StandardAdmission, vertices),
            || NonNegativeReal::new(wire.chordal_deflection),
        )
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

decode_cost_record!(
    [R: cadmpeg_core::decode::cost::DecodeCost, P: cadmpeg_core::decode::cost::DecodeCost] PolylineVertex<R, P>;
    Self { parameter, point } => [parameter:  R, point:  P]
);

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

decode_cost_enum!(
    [R: cadmpeg_core::decode::cost::DecodeCost, P: cadmpeg_core::decode::cost::DecodeCost] PolylineSamples<R, P>;
    Self::Unparameterized { points } => [points],
    Self::Parameterized { vertices } => [vertices],
);

/// Source-native polyline with an explicit chordal error bound.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "PolylineCurveWire"))]
#[serde(try_from = "PolylineCurveWire")]
pub struct PolylineCurve {
    samples: PolylineSamples<FiniteReal, FinitePoint3>,
    chordal_deflection: NonNegativeReal,
}

decode_cost_record!(
    [] PolylineCurve;
    Self { samples, chordal_deflection } => [samples:  PolylineSamples<FiniteReal, FinitePoint3>, chordal_deflection:  NonNegativeReal]
);

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
        Self::build(&StandardAdmission, wire.samples, || {
            NonNegativeReal::new(wire.chordal_deflection)
        })
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
}

impl PolylineSamples<FiniteReal, FinitePoint3> {
    fn has_strictly_monotonic_parameters<A: SampledAdmission>(
        &self,
        admission: &A,
    ) -> Result<bool, A::Error> {
        let Self::Parameterized { vertices } = self else {
            return Ok(true);
        };
        let mut increasing = true;
        for pair in vertices.windows(2) {
            admission.work(1, "IR polyline increasing parameter comparison")?;
            if pair[0].parameter >= pair[1].parameter {
                increasing = false;
                break;
            }
        }
        if increasing {
            return Ok(true);
        }
        for pair in vertices.windows(2) {
            admission.work(1, "IR polyline decreasing parameter comparison")?;
            if pair[0].parameter <= pair[1].parameter {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Edit admitted points transactionally. The edit supplies an admitted
    /// point, so no coordinate needs another admission.
    pub fn edit_admitted_points<E>(
        &mut self,
        mut edit: impl FnMut(FinitePoint3) -> Result<FinitePoint3, E>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<(), E>, CodecError> {
        match self {
            Self::Unparameterized { points } => edit_sample_rows(ctx, points, |point| {
                *point = edit(*point)?;
                Ok(())
            }),
            Self::Parameterized { vertices } => edit_sample_rows(ctx, vertices, |vertex| {
                vertex.point = edit(vertex.point)?;
                Ok(())
            }),
        }
    }

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

fn edit_sample_rows<T: Copy, E>(
    ctx: &DecodeContext<'_>,
    rows: &mut [T],
    mut edit: impl FnMut(&mut T) -> Result<(), E>,
) -> Result<Result<(), E>, CodecError> {
    let (copy, _storage) = ctx.copy_temporary_slice(rows, "IR sampled edit candidate")?;
    let mut candidate = copy;
    for row in &mut candidate {
        ctx.charge_work(1, "IR sampled edit callback")?;
        if let Err(error) = edit(row) {
            return Ok(Err(error));
        }
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(rows.len()),
        "IR sampled edit copy back",
    )?;
    rows.copy_from_slice(&candidate);
    Ok(Ok(()))
}

impl PolylineCurve {
    /// Copy the sample lane through the decode collection budget.
    pub fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let samples = match &self.samples {
            PolylineSamples::Unparameterized { points } => PolylineSamples::Unparameterized {
                points: super::copy_decode_slice(points, ctx, operation)?
                    .try_into()
                    .map_err(|_| ctx.refuse_codec_limit(operation, 0, 0))?,
            },
            PolylineSamples::Parameterized { vertices } => PolylineSamples::Parameterized {
                vertices: super::copy_decode_slice(vertices, ctx, operation)?
                    .try_into()
                    .map_err(|_| ctx.refuse_codec_limit(operation, 0, 0))?,
            },
        };
        Ok(Self {
            samples,
            chordal_deflection: self.chordal_deflection,
        })
    }

    /// Build from admitted sample scalars and points, checking only the
    /// sample count, computed deviation, and parameter order.
    pub fn from_checked_samples(
        samples: PolylineSamples<FiniteReal, FinitePoint3>,
        chordal_deflection: f64,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<Self, GeometryLayoutError>, CodecError> {
        admission::finish(Self::build_checked_samples(ctx, samples, || {
            NonNegativeReal::new(chordal_deflection)
        }))
    }

    fn build_checked_samples<A: SampledAdmission>(
        admission: &A,
        samples: PolylineSamples<FiniteReal, FinitePoint3>,
        deflection: impl FnOnce() -> Option<NonNegativeReal>,
    ) -> Result<Self, A::Error> {
        Self::require_sample_count(admission, samples.count())?;
        let chordal_deflection = Self::admit_deflection(admission, deflection())?;
        if !samples.has_strictly_monotonic_parameters(admission)? {
            return Err(admission.layout("parameters must be finite and strictly monotonic")?);
        }
        Ok(Self {
            samples,
            chordal_deflection,
        })
    }

    /// Build a polyline from its sample rows.
    ///
    /// A parameterized sample carries its parameter in the row, so there is no
    /// second list to pair up and no count to compare.
    pub fn new(
        samples: PolylineSamples,
        chordal_deflection: f64,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<Self, GeometryLayoutError>, CodecError> {
        admission::finish(Self::build(ctx, samples, || {
            NonNegativeReal::new(chordal_deflection)
        }))
    }

    /// Build from admitted source deflection and placement scale.
    pub fn from_scaled_deflection(
        samples: PolylineSamples<FiniteReal, FinitePoint3>,
        chordal_deflection: NonNegativeReal,
        scale: PositiveReal,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<Self, GeometryLayoutError>, CodecError> {
        admission::finish(Self::build_checked_samples(ctx, samples, || {
            chordal_deflection.scaled(scale)
        }))
    }

    fn build<A: SampledAdmission>(
        admission: &A,
        samples: PolylineSamples,
        deflection: impl FnOnce() -> Option<NonNegativeReal>,
    ) -> Result<Self, A::Error> {
        Self::admit_sample_points(admission, &samples)?;
        let chordal_deflection = Self::admit_deflection(admission, deflection())?;
        let samples = Self::admit_sample_parameters(admission, samples)?;
        Ok(Self {
            samples,
            chordal_deflection,
        })
    }

    fn require_sample_count<A: SampledAdmission>(
        admission: &A,
        count: usize,
    ) -> Result<(), A::Error> {
        if count < 2 {
            return Err(admission.layout("polyline must contain at least two points")?);
        }
        Ok(())
    }

    fn admit_deflection<A: SampledAdmission>(
        admission: &A,
        value: Option<NonNegativeReal>,
    ) -> Result<NonNegativeReal, A::Error> {
        match value {
            Some(value) => Ok(value),
            None => Err(admission.layout("chordal_deflection must be finite and non-negative")?),
        }
    }

    /// Check point finiteness before deflection and parameter admission.
    fn admit_sample_points<A: SampledAdmission>(
        admission: &A,
        samples: &PolylineSamples,
    ) -> Result<(), A::Error> {
        Self::require_sample_count(admission, samples.count())?;
        for index in 0..samples.count() {
            admission.work(1, "IR polyline point finiteness")?;
            let point = match samples {
                PolylineSamples::Unparameterized { points } => points[index],
                PolylineSamples::Parameterized { vertices } => vertices[index].point,
            };
            if !point.is_finite() {
                return Err(admission.layout("points must be finite")?);
            }
        }
        Ok(())
    }

    /// Convert final sample rows once after all source points pass admission.
    fn admit_sample_parameters<A: SampledAdmission>(
        admission: &A,
        samples: PolylineSamples,
    ) -> Result<PolylineSamples<FiniteReal, FinitePoint3>, A::Error> {
        fn members<A: SampledAdmission, T>(
            admission: &A,
            values: Vec<T>,
        ) -> Result<crate::features::NonEmptyMembers<T>, A::Error> {
            match values.try_into() {
                Ok(values) => Ok(values),
                Err(_) => Err(admission.layout("polyline must contain at least two points")?),
            }
        }
        let point = |point| match FinitePoint3::new(point) {
            Some(point) => Ok(point),
            None => Err(admission.layout("points must be finite")?),
        };
        let samples = match samples {
            PolylineSamples::Unparameterized { points } => PolylineSamples::Unparameterized {
                points: members(
                    admission,
                    admission.collect(points, "IR polyline admitted samples", point)?,
                )?,
            },
            PolylineSamples::Parameterized { vertices } => PolylineSamples::Parameterized {
                vertices: members(
                    admission,
                    admission.collect(vertices, "IR polyline admitted samples", |vertex| {
                        let Some(parameter) = FiniteReal::new(vertex.parameter) else {
                            return Err(admission
                                .layout("parameters must be finite and strictly monotonic")?);
                        };
                        Ok(PolylineVertex {
                            parameter,
                            point: point(vertex.point)?,
                        })
                    })?,
                )?,
            },
        };
        if !samples.has_strictly_monotonic_parameters(admission)? {
            return Err(admission.layout("parameters must be finite and strictly monotonic")?);
        }
        Ok(samples)
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
            PolylineSamples::Unparameterized { points } => points
                .get(index)
                .and_then(|_| FiniteReal::from_index(index)),
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
        Self::admit_sample_points(&StandardAdmission, &candidate)?;
        self.samples = Self::admit_sample_parameters(&StandardAdmission, candidate)?;
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

fn scale_admitted_points<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    points: impl Iterator<Item = &'a mut FinitePoint3>,
    scale: PositiveReal,
    message: &'static str,
) -> Result<Result<(), GeometryLayoutError>, cadmpeg_core::CodecError> {
    for point in points {
        ctx.charge_work(1, "IR sampled unit scaling work")?;
        let Some(scaled) = point.scaled(scale) else {
            return Ok(Err(GeometryLayoutError::Layout(
                ctx.copy_retained_text(message, "IR sampled refusal text")?,
            )));
        };
        *point = scaled;
    }
    Ok(Ok(()))
}

fn scale_admitted_deflection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    deflection: &mut NonNegativeReal,
    scale: PositiveReal,
) -> Result<Result<(), GeometryLayoutError>, cadmpeg_core::CodecError> {
    let Some(scaled) = deflection.scaled(scale) else {
        return Ok(Err(GeometryLayoutError::Layout(ctx.copy_retained_text(
            "chordal_deflection must be finite and non-negative",
            "IR sampled refusal text",
        )?)));
    };
    *deflection = scaled;
    Ok(Ok(()))
}

impl PolygonalSurface {
    pub(crate) fn scale_points(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<Result<(), GeometryLayoutError>, cadmpeg_core::CodecError> {
        if let Err(error) = scale_admitted_points(
            ctx,
            self.vertices.iter_mut(),
            scale,
            "vertices must be finite",
        )? {
            return Ok(Err(error));
        }
        scale_admitted_deflection(ctx, &mut self.chordal_deflection, scale)
    }
}

impl PolylineCurve {
    pub(crate) fn scale_points(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: PositiveReal,
    ) -> Result<Result<(), GeometryLayoutError>, cadmpeg_core::CodecError> {
        let result = match &mut self.samples {
            PolylineSamples::Unparameterized { points } => {
                scale_admitted_points(ctx, points.iter_mut(), scale, "points must be finite")
            }
            PolylineSamples::Parameterized { vertices } => scale_admitted_points(
                ctx,
                vertices.iter_mut().map(|vertex| &mut vertex.point),
                scale,
                "points must be finite",
            ),
        }?;
        if let Err(error) = result {
            return Ok(Err(error));
        }
        scale_admitted_deflection(ctx, &mut self.chordal_deflection, scale)
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
