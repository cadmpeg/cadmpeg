// SPDX-License-Identifier: Apache-2.0
//! Geometry carriers: analytic surfaces, analytic curves, NURBS, and pcurves.
//!
//! Carriers are stored in their own arenas and referenced by id from the
//! topology graph (a face references a [`Surface`], an edge a [`Curve`], a
//! coedge a [`pcurve::Pcurve`]). One carrier may therefore support several topological
//! entities.

use crate::features::{FinitePoint3, FiniteVector3};
use crate::ids::{CurveId, PcurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use crate::math::{
    sum::{fast_dot, ExactSignedSum},
    Point3, Vector3,
};
use crate::provenance::SourceObjectAssociation;
use crate::scalar::{FiniteReal, NonNegativeReal, PositiveI64};
use crate::transform::Transform;
use crate::units::FiniteVector;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroI64;

pub mod analytic;
pub mod nurbs;
pub mod pcurve;
pub mod sampled;
pub mod scaling;
use analytic::{
    CircleCurve, ConeSurface, CylinderSurface, DegenerateCurve, EllipseCurve, HyperbolaCurve,
    LineCurve, ParabolaCurve, PlaneSurface, SphereSurface, TorusSurface,
};
use nurbs::{NurbsCurve, NurbsSurface};
use pcurve::PcurveGeometry;
use sampled::{PolygonalSurface, PolylineCurve};

/// Checked procedural curve payloads.
pub mod curve_payloads;
mod lanes;
/// Checked procedural surface payloads.
pub mod surface_payloads;

/// Greatest number of nesting carriers the IR admits over one geometry leaf.
///
/// A nesting carrier holds its basis inline in a `Box` rather than by arena id:
/// [`PlacedSurface`], [`PlacedCurve`], and a pcurve's
/// [`PlacedPcurve`](pcurve::PlacedPcurve),
/// [`TrimmedPcurve`](pcurve::TrimmedPcurve) and
/// [`OffsetPcurve`](pcurve::OffsetPcurve). Each one's `try_new` refuses a
/// result past this depth and stores the depth it holds, so no value of the
/// three carriers nests deeper however it was built or deserialized, and every
/// consumer that reads through a basis is bounded without a check of its own.
pub const MAX_GEOMETRY_NESTING: usize = 256;

/// Admitted conditional flag shapes in the pre-revision offset-surface layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum LegacyExtensionFlags {
    /// The compact `offsur` layout has no extension flag.
    Absent {},
    /// The extension gate is false, so no dependent flags follow it.
    Disabled {},
    /// The extension gate is true and carries its required flag and optional
    /// later-revision flag.
    Enabled {
        /// Required flag following the true extension gate.
        secondary: bool,
        /// Optional flag admitted by the later legacy layout.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tertiary"
        )]
        tertiary: Option<bool>,
    },
}

/// Mutually exclusive pre-revision and revision-gated offset layouts.
// A source states raw scalars; an `OffsetSurfaceConstruction` holds the
// admitted extension, whose scalars are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "layout", rename_all = "snake_case", deny_unknown_fields)]
// Variant payloads retain the native layout as one value without separate heap ownership.
#[allow(clippy::large_enum_variant)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub enum OffsetExtension<R = f64> {
    /// Pre-revision conditional flag sequence.
    Legacy {
        /// Conditional flag sequence in its positional wire form.
        flags: LegacyExtensionFlags,
        /// Solved-cache fit contract this layout states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    /// Revision-gated fields with the required four-boolean carrier run.
    Revision {
        /// Revision-gated form whose carrier run is exactly four booleans.
        form: RevisionSurfaceForm<[bool; 4], R>,
    },
}

/// Analytic, NURBS, or opaque surface geometry established without a
/// construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SolvedSurfaceGeometry {
    /// Infinite plane through `origin` with the given `normal`.
    Plane(PlaneSurface),
    /// Right circular cylinder of the given `radius` about the axis line.
    Cylinder(CylinderSurface),
    /// Right elliptical cone. `radius` is the major radius at `origin`;
    /// `ratio` is the minor-to-major radius ratio; `half_angle` is the major
    /// half-angle between the axis and the cone surface, in radians.
    Cone(ConeSurface),
    /// Sphere.
    Sphere(SphereSurface),
    /// Torus. `major_radius` is the distance from `center` to the tube center;
    /// `minor_radius` is the tube radius.
    Torus(TorusSurface),
    /// Free-form NURBS surface.
    Nurbs(NurbsSurface),
    /// Source-native polygonal surface with an explicit chordal error bound.
    Polygonal(PolygonalSurface),
    /// Exact affine placement of an inline basis surface.
    Transformed(PlacedSurface),
    /// Surface geometry that has no typed neutral representation.
    ///
    /// `record` links to retained source bytes when available.
    ///
    /// A [`Surface`] carrying this variant should have entity exactness
    /// [`Exactness::Unknown`](crate::provenance::Exactness::Unknown) in the
    /// document's [`Annotations`](crate::annotations::Annotations): the shape was
    /// not established, so nothing about it is byte-exact or derived.
    Unknown {
        /// Link to the preserved raw record, when the decoder kept the bytes.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_record"
        )]
        record: Option<UnknownId>,
    },
}

impl SolvedSurfaceGeometry {
    /// Placements enclosing the leaf of this carrier's inline basis chain.
    ///
    /// [`PlacedSurface`] stores its own depth, so this reads one field and
    /// building a chain costs one addition per placement.
    #[must_use]
    pub(crate) const fn nesting_depth(&self) -> usize {
        match self {
            Self::Transformed(placed) => placed.depth,
            Self::Plane(_)
            | Self::Cylinder(_)
            | Self::Cone(_)
            | Self::Sphere(_)
            | Self::Torus(_)
            | Self::Nurbs(_)
            | Self::Polygonal(_)
            | Self::Unknown { .. } => 0,
        }
    }
}

/// Exact affine placement of an inline basis surface.
///
/// `try_new` is the only constructor and refuses a chain deeper than
/// [`MAX_GEOMETRY_NESTING`], so no [`SolvedSurfaceGeometry`] value nests past
/// the bound however it was built or read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PlacedSurfaceWire")]
pub struct PlacedSurface {
    basis: Box<SolvedSurfaceGeometry>,
    transform: Transform,
    #[serde(skip)]
    depth: usize,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PlacedSurfaceWire {
    /// Unplaced basis geometry with unchanged parameterization.
    basis: Box<SolvedSurfaceGeometry>,
    /// Affine map from basis coordinates to model coordinates.
    transform: Transform,
}

impl PlacedSurface {
    /// Place a basis surface, refusing a chain past [`MAX_GEOMETRY_NESTING`].
    ///
    /// # Errors
    ///
    /// Refuses a basis already at the bound, whose placement would produce a
    /// carrier one deeper than the IR admits.
    pub fn try_new(
        basis: Box<SolvedSurfaceGeometry>,
        transform: Transform,
    ) -> Result<Self, &'static str> {
        let Some(depth) = basis
            .nesting_depth()
            .checked_add(1)
            .filter(|depth| *depth <= MAX_GEOMETRY_NESTING)
        else {
            return Err("PlacedSurface.basis nests past the admitted inline basis depth");
        };
        Ok(Self {
            basis,
            transform,
            depth,
        })
    }

    /// Return the basis.
    #[must_use]
    pub const fn basis(&self) -> &SolvedSurfaceGeometry {
        &self.basis
    }

    /// Return the transform.
    #[must_use]
    pub const fn transform(&self) -> &Transform {
        &self.transform
    }

    /// Replace the transform. The basis chain, and so the depth, is unchanged.
    pub const fn set_transform(&mut self, transform: Transform) {
        self.transform = transform;
    }
}

impl TryFrom<PlacedSurfaceWire> for PlacedSurface {
    type Error = &'static str;
    fn try_from(wire: PlacedSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.basis, wire.transform)
    }
}

/// Analytic, NURBS, procedural, or opaque surface geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SurfaceGeometry {
    /// Exact surface defined by a procedural construction in the same model.
    Procedural {
        /// Construction that produces this carrier.
        construction: ProceduralSurfaceId,
        /// Solved carrier geometry retained from the source cache.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_surface_geometry_cache"
        )]
        #[cfg_attr(feature = "schema", schemars(skip))]
        cache: Option<SolvedSurfaceGeometry>,
    },
    /// A carrier whose shape is established without a construction.
    #[serde(untagged)]
    Solved(SolvedSurfaceGeometry),
}

impl SurfaceGeometry {
    /// Construction that owns this carrier, when it is procedural.
    #[must_use]
    pub const fn procedural_construction(&self) -> Option<&ProceduralSurfaceId> {
        match self {
            Self::Procedural { construction, .. } => Some(construction),
            Self::Solved(_) => None,
        }
    }

    /// Solved carrier retained from the source cache, when this carrier is
    /// procedural.
    #[must_use]
    pub const fn solved_cache(&self) -> Option<&SolvedSurfaceGeometry> {
        match self {
            Self::Procedural { cache, .. } => cache.as_ref(),
            Self::Solved(_) => None,
        }
    }

    /// Geometry that evaluates this carrier without following a construction.
    #[must_use]
    pub const fn solved(&self) -> Option<&SolvedSurfaceGeometry> {
        match self {
            Self::Procedural { cache, .. } => cache.as_ref(),
            Self::Solved(geometry) => Some(geometry),
        }
    }
}

/// An identified surface carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Surface {
    /// Arena id.
    pub id: SurfaceId,
    /// Surface shape.
    pub geometry: SurfaceGeometry,
    /// Native source-object identity and effective display metadata.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_object"
    )]
    pub source_object: Option<SourceObjectAssociation>,
}

/// The analytic or free-form shape of a 3D curve carrier established without a
/// construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SolvedCurveGeometry {
    /// Infinite line.
    Line(LineCurve),
    /// Full circle.
    Circle(CircleCurve),
    /// Ellipse.
    Ellipse(EllipseCurve),
    /// Parabola in STEP conic form.
    Parabola(ParabolaCurve),
    /// Hyperbola in STEP conic form.
    Hyperbola(HyperbolaCurve),
    /// A curve collapsed to one model-space point at a topological singularity.
    Degenerate(DegenerateCurve),
    /// Ordered child curves joined into one bounded carrier.
    Composite {
        /// Ordered curve uses and their continuity contracts.
        segments: CompositeCurveSegments,
        /// Whether the source classifies the complete curve as self-intersecting.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        self_intersect: Option<bool>,
    },
    /// Free-form NURBS curve.
    Nurbs(NurbsCurve),
    /// Source-native polyline with an explicit chordal error bound.
    Polyline(PolylineCurve),
    /// Exact affine placement of an inline basis curve.
    Transformed(PlacedCurve),
    /// Native curve carrier whose shape is not decoded.
    Unknown {
        /// Retained native record containing the curve carrier.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_record"
        )]
        record: Option<UnknownId>,
    },
}

impl SolvedCurveGeometry {
    /// Placements enclosing the leaf of this carrier's inline basis chain.
    ///
    /// [`PlacedCurve`] stores its own depth, so this reads one field and
    /// building a chain costs one addition per placement.
    #[must_use]
    pub(crate) const fn nesting_depth(&self) -> usize {
        match self {
            Self::Transformed(placed) => placed.depth,
            Self::Line(_)
            | Self::Circle(_)
            | Self::Ellipse(_)
            | Self::Parabola(_)
            | Self::Hyperbola(_)
            | Self::Degenerate(_)
            | Self::Composite { .. }
            | Self::Nurbs(_)
            | Self::Polyline(_)
            | Self::Unknown { .. } => 0,
        }
    }
}

/// Exact affine placement of an inline basis curve.
///
/// `try_new` is the only constructor and refuses a chain deeper than
/// [`MAX_GEOMETRY_NESTING`], so no [`SolvedCurveGeometry`] value nests past the
/// bound however it was built or read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PlacedCurveWire")]
pub struct PlacedCurve {
    basis: Box<SolvedCurveGeometry>,
    transform: Transform,
    #[serde(skip)]
    depth: usize,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PlacedCurveWire {
    /// Unplaced basis geometry with unchanged parameterization.
    basis: Box<SolvedCurveGeometry>,
    /// Affine map from basis coordinates to model coordinates.
    transform: Transform,
}

impl PlacedCurve {
    /// Place a basis curve, refusing a chain past [`MAX_GEOMETRY_NESTING`].
    ///
    /// # Errors
    ///
    /// Refuses a basis already at the bound, whose placement would produce a
    /// carrier one deeper than the IR admits.
    pub fn try_new(
        basis: Box<SolvedCurveGeometry>,
        transform: Transform,
    ) -> Result<Self, &'static str> {
        let Some(depth) = basis
            .nesting_depth()
            .checked_add(1)
            .filter(|depth| *depth <= MAX_GEOMETRY_NESTING)
        else {
            return Err("PlacedCurve.basis nests past the admitted inline basis depth");
        };
        Ok(Self {
            basis,
            transform,
            depth,
        })
    }

    /// Return the basis.
    #[must_use]
    pub const fn basis(&self) -> &SolvedCurveGeometry {
        &self.basis
    }

    /// Return the transform.
    #[must_use]
    pub const fn transform(&self) -> &Transform {
        &self.transform
    }

    /// Replace the transform. The basis chain, and so the depth, is unchanged.
    pub const fn set_transform(&mut self, transform: Transform) {
        self.transform = transform;
    }
}

impl TryFrom<PlacedCurveWire> for PlacedCurve {
    type Error = &'static str;
    fn try_from(wire: PlacedCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.basis, wire.transform)
    }
}

/// The analytic, free-form, procedural, or opaque shape of a 3D curve carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CurveGeometry {
    /// Exact curve defined by a procedural construction in the same model.
    Procedural {
        /// Construction that produces this carrier.
        construction: ProceduralCurveId,
        /// Solved carrier geometry retained from the source cache.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_curve_geometry_cache"
        )]
        #[cfg_attr(feature = "schema", schemars(skip))]
        cache: Option<SolvedCurveGeometry>,
    },
    /// A carrier whose shape is established without a construction.
    #[serde(untagged)]
    Solved(SolvedCurveGeometry),
}

impl CurveGeometry {
    /// Construction that owns this carrier, when it is procedural.
    #[must_use]
    pub const fn procedural_construction(&self) -> Option<&ProceduralCurveId> {
        match self {
            Self::Procedural { construction, .. } => Some(construction),
            Self::Solved(_) => None,
        }
    }

    /// Solved carrier retained from the source cache, when this carrier is
    /// procedural.
    #[must_use]
    pub const fn solved_cache(&self) -> Option<&SolvedCurveGeometry> {
        match self {
            Self::Procedural { cache, .. } => cache.as_ref(),
            Self::Solved(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn solved_cache_mut(&mut self) -> Option<&mut SolvedCurveGeometry> {
        match self {
            Self::Procedural { cache, .. } => cache.as_mut(),
            Self::Solved(_) => None,
        }
    }

    /// Geometry that evaluates this carrier without following a construction.
    #[must_use]
    pub const fn solved(&self) -> Option<&SolvedCurveGeometry> {
        match self {
            Self::Procedural { cache, .. } => cache.as_ref(),
            Self::Solved(geometry) => Some(geometry),
        }
    }
}

/// Non-empty ordered child uses of a composite curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<CompositeCurveSegment>")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CompositeCurveSegments(Vec<CompositeCurveSegment>);

impl TryFrom<Vec<CompositeCurveSegment>> for CompositeCurveSegments {
    type Error = &'static str;

    fn try_from(segments: Vec<CompositeCurveSegment>) -> Result<Self, Self::Error> {
        if segments.is_empty() {
            return Err("composite curve segments must not be empty");
        }
        Ok(Self(segments))
    }
}

impl std::ops::Deref for CompositeCurveSegments {
    type Target = [CompositeCurveSegment];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for CompositeCurveSegments {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a> IntoIterator for &'a CompositeCurveSegments {
    type Item = &'a CompositeCurveSegment;
    type IntoIter = std::slice::Iter<'a, CompositeCurveSegment>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// One directed child use in a composite curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CompositeCurveSegment {
    /// Referenced child curve carrier.
    pub curve: CurveId,
    /// Whether the child parameter direction is retained.
    pub same_sense: bool,
    /// Required continuity from the preceding segment to this segment.
    pub transition: CompositeCurveTransition,
}

/// STEP composite-curve transition continuity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CompositeCurveTransition {
    /// No positional continuity is asserted.
    Discontinuous,
    /// Positional continuity.
    Continuous,
    /// Positional and tangent continuity.
    ContSameGradient,
    /// Positional, tangent, and curvature continuity.
    ContSameGradientSameCurvature,
}

/// Derive a stable in-plane reference direction from an axis.
///
/// The least-aligned global basis axis is projected onto the plane normal to
/// `axis`, then normalized. Degenerate axes fall back to global x.
pub fn derive_reference_direction(axis: Vector3) -> Vector3 {
    let norm = axis.norm();
    let axis = if norm.is_finite() && norm != 0.0 {
        Vector3::new(axis.x / norm, axis.y / norm, axis.z / norm)
    } else if let Some(unit) = crate::features::FiniteVector3::new(axis)
        .and_then(crate::features::FiniteVector3::unit_nonzero)
    {
        unit
    } else {
        return Vector3::new(1.0, 0.0, 0.0);
    };
    let basis = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if axis.y.abs() <= axis.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let dot = axis.x * basis.x + axis.y * basis.y + axis.z * basis.z;
    let projected = Vector3::new(
        basis.x - dot * axis.x,
        basis.y - dot * axis.y,
        basis.z - dot * axis.z,
    );
    let projected_norm = projected.norm();
    Vector3::new(
        projected.x / projected_norm,
        projected.y / projected_norm,
        projected.z / projected_norm,
    )
}

/// A 3D curve carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Curve {
    /// Arena id.
    pub id: CurveId,
    /// Curve shape.
    pub geometry: CurveGeometry,
    /// Native source-object identity and effective display metadata.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_object"
    )]
    pub source_object: Option<SourceObjectAssociation>,
}

/// Four optional finite parameter bounds retained from one native surface
/// record.
///
/// The positions are the native record's fields. Their meaning is supplied
/// by the owning subtype, so a partial quartet remains valid. `None` in one
/// position means that field was absent in the source record.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "[Option<f64>; 4]", into = "[Option<f64>; 4]")]
pub struct RecordBounds([Option<f64>; 4]);

/// A record-bound quartet contained a non-finite value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("record bounds must contain only finite values")]
pub struct RecordBoundsError;

impl RecordBounds {
    /// Admit a native quartet after checking every present value.
    pub fn try_new(raw: [Option<f64>; 4]) -> Result<Self, RecordBoundsError> {
        if raw.iter().flatten().all(|value| value.is_finite()) {
            Ok(Self(raw))
        } else {
            Err(RecordBoundsError)
        }
    }

    /// Build the quartet `[u_lower, v_lower, u_upper, v_upper]` of a
    /// parameter box: its lower corner, then its upper corner. Every endpoint
    /// of an admitted interval is finite, so nothing is checked.
    #[must_use]
    pub const fn from_corners(
        u: crate::topology::IncreasingParameterInterval,
        v: crate::topology::IncreasingParameterInterval,
    ) -> Self {
        Self([
            Some(u.lower()),
            Some(v.lower()),
            Some(u.upper()),
            Some(v.upper()),
        ])
    }

    /// Build the quartet of four present finite values in the native
    /// record's field order. Every value is finite, so nothing is checked.
    #[must_use]
    pub const fn from_finite([first, second, third, fourth]: [FiniteReal; 4]) -> Self {
        Self([
            Some(first.get()),
            Some(second.get()),
            Some(third.get()),
            Some(fourth.get()),
        ])
    }

    /// Return the admitted quartet. Every present position is finite.
    #[must_use]
    pub const fn get(self) -> [Option<f64>; 4] {
        self.0
    }
}

impl TryFrom<[Option<f64>; 4]> for RecordBounds {
    type Error = RecordBoundsError;

    fn try_from(raw: [Option<f64>; 4]) -> Result<Self, Self::Error> {
        Self::try_new(raw)
    }
}

impl From<RecordBounds> for [Option<f64>; 4] {
    fn from(bounds: RecordBounds) -> Self {
        bounds.get()
    }
}

/// A neutral surface construction linked to the carrier it produces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProceduralSurface {
    /// Stable construction identity.
    pub id: ProceduralSurfaceId,
    /// Neutral construction definition, which states its own cache contract.
    definition: ProceduralSurfaceDefinition,
    /// Four optional U/V parameter bounds following the record's subtype
    /// scope. For a procedural extrusion or revolution, the first pair is
    /// the neutral surface-carrier interval; its definition retains the
    /// source directrix interval separately. `None` when the record stores no
    /// bound fields.
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    record_bounds: Option<RecordBounds>,
}

/// Parameter fields carried by exact and loft spline-surface constructions.
// A source states raw scalars; a `LoftSurfacePayload` holds the admitted
// fields, whose scalars are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SplineSurfaceParameters<R = f64> {
    /// Ordered semantic U and V intervals in the legacy layout.
    OrderedRanges {
        /// Ordered U and V intervals.
        ranges: [[R; 2]; 2],
    },
    /// Two parameter intervals in a revision-gated layout, each stored as an
    /// ordered `[lo, hi]` pair of optional bounds. For exact and t-spline
    /// surfaces these are the surface's unextended (pre-extension) parameter
    /// ranges; for loft surfaces they are wrap ranges, where a reversed pair
    /// (`lo > hi`) encodes an empty interval (no wrap). `None` is a false
    /// bound-presence flag.
    RevisionRanges {
        /// Two parameter intervals in serialized field order.
        intervals: [[Option<R>; 2]; 2],
    },
}

/// Mutually exclusive legacy and revision-gated exact-spline layouts.
// A source states raw scalars; an `ExactSurfacePayload` holds the admitted
// spline, whose scalars are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "layout", rename_all = "snake_case", deny_unknown_fields)]
// Variant payloads retain the native layout as one value without separate heap ownership.
#[allow(clippy::large_enum_variant)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub enum ExactSpline<R = f64> {
    /// Legacy solved-cache layout with ordered U/V ranges.
    Legacy {
        /// Ordered U and V parameter ranges.
        ranges: [[R; 2]; 2],
        /// Native ASM extension integer following the ranges.
        extension: i64,
        /// Solved-cache fit contract this layout states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    /// Revision-gated layout with optional interval bounds and shared form.
    Revision {
        /// Two optional-bound parameter intervals in wire order.
        intervals: [[Option<R>; 2]; 2],
        /// Native ASM extension enum following the intervals.
        extension: i64,
        /// Required revision-gated form.
        form: RevisionSurfaceForm<Vec<bool>, R>,
    },
}

/// One component and its native construction scalar.
// A source states a raw scalar; a compound construction holds the admitted
// component, whose scalar is a `FiniteReal`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CompoundComponent<T, R = f64> {
    /// Scalar paired with this component.
    pub parameter: R,
    /// Component geometry or its resolved identity.
    pub component: T,
}

/// A non-empty compound curve with finite construction parameters.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CompoundCurveConstructionWire")]
pub struct CompoundCurveConstruction {
    parameters: Vec<FiniteReal>,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<CompoundComponent<CurveId>>"))]
    components: Vec<CompoundComponent<CurveId, FiniteReal>>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cache"
    )]
    cache: Option<LegacyCache>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct CompoundCurveConstructionWire {
    /// Finite leading and per-component construction parameters.
    parameters: Vec<f64>,
    /// Component curves in construction order.
    components: Vec<CompoundComponent<CurveId>>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cache"
    )]
    cache: Option<LegacyCache>,
}

impl TryFrom<CompoundCurveConstructionWire> for CompoundCurveConstruction {
    type Error = &'static str;
    fn try_from(wire: CompoundCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.parameters, wire.components, wire.cache)
    }
}

impl Serialize for CompoundCurveConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        CompoundCurveConstructionWire {
            parameters: FiniteReal::raw_lane(&self.parameters),
            components: self
                .components
                .iter()
                .map(CompoundComponent::to_raw)
                .collect(),
            cache: self.cache,
        }
        .serialize(serializer)
    }
}

impl CompoundCurveConstruction {
    /// Admit at least one component and finite leading and component parameters.
    pub fn try_new(
        parameters: Vec<f64>,
        components: Vec<CompoundComponent<CurveId>>,
        cache: Option<LegacyCache>,
    ) -> Result<Self, &'static str> {
        const INVALID: &str = "compound curve parameters must be finite";
        if components.is_empty() {
            return Err("compound curve components must not be empty");
        }
        let [parameters] = FiniteReal::lanes([parameters]).ok_or(INVALID)?;
        let components = components
            .into_iter()
            .map(CompoundComponent::admit)
            .collect::<Option<Vec<_>>>()
            .ok_or(INVALID)?;
        Ok(Self {
            parameters,
            components,
            cache,
        })
    }

    /// Return the parameters.
    #[must_use]
    pub fn parameters(&self) -> &[FiniteReal] {
        &self.parameters
    }

    /// Return the components.
    #[must_use]
    pub fn components(&self) -> &[CompoundComponent<CurveId, FiniteReal>] {
        &self.components
    }
}

/// Neutral semantics for a procedural surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralSurfaceDefinition {
    /// Exact native NURBS surface with retained parameter fields.
    Exact(surface_payloads::ExactSurfacePayload),
    /// Ordered native compound of a solved surface and component surfaces.
    Compound(surface_payloads::CompoundSurfacePayload),
    /// Exact rectangular restriction of an embedded support surface.
    SubSurface(surface_payloads::SubSurfaceConstruction),
    /// Taper of a support surface around a reference curve.
    Taper(surface_payloads::TaperSurfaceConstruction),
    /// Native loft defined by two section graphs and closure contracts.
    Loft(surface_payloads::LoftSurfacePayload),
    /// Native compound-loft construction.
    CompoundLoft(surface_payloads::CompoundLoftSurfacePayload),
    /// Revision-gated compound-loft construction.
    RevisionCompoundLoft {
        /// Complete native revision-gated compound-loft graph.
        construction: Box<RevisionCompoundLoftConstruction>,
    },
    /// Native scaled compound-loft construction.
    ScaledCompoundLoft(surface_payloads::ScaledCompoundLoftSurfacePayload),
    /// Native skinned spline surface.
    Skin(surface_payloads::SkinSurfacePayload),
    /// Native surface defined by recursive law formulas.
    Law(surface_payloads::LawSurfacePayload),
    /// Native curve-network spline surface.
    Net(surface_payloads::NetSurfacePayload),
    /// Native curvature-continuous two-sided blend.
    G2Blend(surface_payloads::G2BlendSurfacePayload),
    /// Revision-gated curvature-continuous blend in the variable-blend side
    /// layout.
    RevisionG2Blend {
        /// Complete native revision-gated G2 construction graph.
        construction: Box<RevisionG2BlendConstruction>,
    },
    /// Native variable-radius two-sided blend.
    VariableBlend(surface_payloads::VariableBlendSurfacePayload),
    /// Native vertex-blend patch.
    VertexBlend(surface_payloads::VertexBlendSurfacePayload),
    /// Translation of a directrix along a direction.
    Extrusion(surface_payloads::ExtrusionSurfaceConstruction),
    /// Unbounded linear sweep of a directrix.
    LinearSweep(surface_payloads::LinearSweepSurfaceConstruction),
    /// Revolution of a directrix about an axis.
    Revolution(surface_payloads::RevolutionSurfaceConstruction),
    /// Full revolution of a directrix about an axis.
    AxisRevolution(surface_payloads::AxisRevolutionSurfaceConstruction),
    /// Sum of two ordered curves from a base point.
    Sum(surface_payloads::SumSurfaceConstruction),
    /// Sweep of a profile along a spine.
    Sweep(surface_payloads::SweepSurfacePayload),
    /// T-spline face with its shared subtransform program.
    TSpline {
        /// Complete native T-spline wrapper construction.
        construction: Box<TSplineSurfaceConstruction>,
    },
    /// Surface generated along an inline circular or linear helix path.
    Helix {
        /// Complete native helix-surface construction.
        construction: Box<HelixSurfaceConstruction>,
    },
    /// Native deformable spline surface.
    Deformable(surface_payloads::DeformableSurfacePayload),
    /// Offset from a support surface.
    Offset(surface_payloads::OffsetSurfaceConstruction),
    /// Rectangular parameter sub-range of a support surface.
    Subset(surface_payloads::SubsetSurfaceConstruction),
    /// Affine replica of a surface carrier, retaining the parent surface
    /// construction and its parameter domain.
    Replica {
        /// Surface being replicated.
        source: SurfaceId,
        /// Affine map from the parent surface coordinates to this surface.
        transform: Transform,
    },
    /// Parallel offset from a support surface.
    ParallelOffset(surface_payloads::ParallelOffsetSurfaceConstruction),
    /// Self-intersecting torus with an explicitly selected outer or inner sheet.
    DegenerateTorus {
        /// Whether the outer sheet is selected at the self-intersection.
        select_outer: bool,
    },
    /// Surface domain bounded by ordered curves on a supporting surface.
    CurveBounded {
        /// Supporting surface whose parameterization defines the domain.
        support: SurfaceId,
        /// Boundary curves on the support.
        boundaries: Vec<CurveId>,
        /// Parameter-space carriers used by surface-curve boundary segments.
        ///
        /// These are separate from `boundaries`: one STEP surface curve can
        /// carry both a model-space curve and pcurves on the support surface.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        boundary_pcurves: Vec<PcurveId>,
        /// Whether the support's natural outer boundary is implicit.
        implicit_outer: bool,
    },
    /// Ruled surface joining two directrices.
    Ruled {
        /// First bounding curve of the ruled surface.
        first: CurveId,
        /// Second bounding curve of the ruled surface.
        second: CurveId,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Rolling-ball or law-driven blend between two support surfaces.
    Blend(surface_payloads::BlendSurfacePayload),
    /// Rolling-ball surface defined by aligned quintic value/derivative jets.
    RollingBallJet(RollingBallJetStations),
    /// Preserved construction without a neutral interpretation.
    Unknown {
        /// Reference to the preserved raw source record, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
}

#[derive(Deserialize)]
#[serde(
    remote = "ProceduralSurfaceDefinition",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum ProceduralSurfaceDefinitionWire {
    Exact(surface_payloads::ExactSurfacePayload),
    Compound(surface_payloads::CompoundSurfacePayload),
    SubSurface(surface_payloads::SubSurfaceConstruction),
    Taper(surface_payloads::TaperSurfaceConstruction),
    Loft(surface_payloads::LoftSurfacePayload),
    CompoundLoft(surface_payloads::CompoundLoftSurfacePayload),
    RevisionCompoundLoft {
        construction: Box<RevisionCompoundLoftConstruction>,
    },
    ScaledCompoundLoft(surface_payloads::ScaledCompoundLoftSurfacePayload),
    Skin(surface_payloads::SkinSurfacePayload),
    Law(surface_payloads::LawSurfacePayload),
    Net(surface_payloads::NetSurfacePayload),
    G2Blend(surface_payloads::G2BlendSurfacePayload),
    RevisionG2Blend {
        construction: Box<RevisionG2BlendConstruction>,
    },
    VariableBlend(surface_payloads::VariableBlendSurfacePayload),
    VertexBlend(surface_payloads::VertexBlendSurfacePayload),
    Extrusion(surface_payloads::ExtrusionSurfaceConstruction),
    LinearSweep(surface_payloads::LinearSweepSurfaceConstruction),
    Revolution(surface_payloads::RevolutionSurfaceConstruction),
    AxisRevolution(surface_payloads::AxisRevolutionSurfaceConstruction),
    Sum(surface_payloads::SumSurfaceConstruction),
    Sweep(surface_payloads::SweepSurfacePayload),
    TSpline {
        construction: Box<TSplineSurfaceConstruction>,
    },
    Helix {
        construction: Box<HelixSurfaceConstruction>,
    },
    Deformable(surface_payloads::DeformableSurfacePayload),
    Offset(surface_payloads::OffsetSurfaceConstruction),
    Subset(surface_payloads::SubsetSurfaceConstruction),
    Replica {
        source: SurfaceId,
        transform: Transform,
    },
    ParallelOffset(surface_payloads::ParallelOffsetSurfaceConstruction),
    DegenerateTorus {
        select_outer: bool,
    },
    CurveBounded {
        support: SurfaceId,
        boundaries: Vec<CurveId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        boundary_pcurves: Vec<PcurveId>,
        implicit_outer: bool,
    },
    Ruled {
        first: CurveId,
        second: CurveId,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    Blend(surface_payloads::BlendSurfacePayload),
    RollingBallJet(RollingBallJetStations),
    Unknown {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_record"
        )]
        record: Option<UnknownId>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
}

impl<'de> Deserialize<'de> for ProceduralSurfaceDefinition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ProceduralSurfaceDefinitionWire::deserialize(deserializer)
    }
}

impl ProceduralSurfaceDefinition {
    fn revision_cache(
        &self,
    ) -> Option<&RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>>> {
        match self {
            Self::Exact(payload) => payload.revision_cache(),
            Self::Taper(definition_payload) => {
                let revision_form = definition_payload.revision_form();
                revision_form.as_ref().map(|form| &form.cache)
            }
            Self::Extrusion(definition_payload) => {
                let revision_form = definition_payload.revision_form();
                revision_form.as_ref().map(|form| &form.cache)
            }
            Self::Revolution(definition_payload) => {
                let revision_form = definition_payload.revision_form();
                revision_form.as_ref().map(|form| &form.cache)
            }
            Self::Sum(payload) => payload.revision_form().map(|form| &form.cache),
            Self::Offset(payload) => match payload.extension() {
                OffsetExtension::Revision { form } => Some(&form.cache),
                OffsetExtension::Legacy { .. } => None,
            },
            Self::Loft(payload) => payload.revision_cache(),
            Self::RevisionCompoundLoft { construction } => Some(&construction.cache),
            Self::RevisionG2Blend { construction } => Some(&construction.cache),
            Self::Sweep(payload) => payload.revision_cache(),
            Self::TSpline { construction } => construction.cache.form().map(|form| &form.cache),
            Self::Deformable(payload) => payload.revision_cache(),
            Self::Blend(payload) => payload.revision_cache(),
            Self::Compound(_)
            | Self::SubSurface(_)
            | Self::CompoundLoft(_)
            | Self::ScaledCompoundLoft(_)
            | Self::Skin(_)
            | Self::Law(_)
            | Self::Net(_)
            | Self::G2Blend(_)
            | Self::VariableBlend(_)
            | Self::VertexBlend(_)
            | Self::LinearSweep(_)
            | Self::AxisRevolution(_)
            | Self::Helix { .. }
            | Self::Subset(_)
            | Self::Replica { .. }
            | Self::ParallelOffset(_)
            | Self::DegenerateTorus { .. }
            | Self::CurveBounded { .. }
            | Self::Ruled { .. }
            | Self::RollingBallJet(_)
            | Self::Unknown { .. } => None,
        }
    }

    /// Write the fit tolerance of the revision-gated solved cache.
    ///
    /// The narrow write route: the borrow of the cache form stays inside this
    /// method, so no construction lends its admitted interior for writing.
    fn write_revision_fit_tolerance(
        &mut self,
        value: FitTolerance,
        write: ToleranceWrite,
    ) -> RevisionCacheWrite {
        match self {
            Self::Exact(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::Taper(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::Extrusion(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::Revolution(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::Sum(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::Offset(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::Loft(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::RevisionCompoundLoft { construction } => {
                construction.cache.write_fit_tolerance(value, write)
            }
            Self::RevisionG2Blend { construction } => {
                construction.cache.write_fit_tolerance(value, write)
            }
            Self::Sweep(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::TSpline { construction } => write_revision_form_tolerance(
                construction.cache.form_mut().map(|form| &mut form.cache),
                value,
                write,
            ),
            Self::Deformable(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::Blend(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::Compound(_)
            | Self::SubSurface(_)
            | Self::CompoundLoft(_)
            | Self::ScaledCompoundLoft(_)
            | Self::Skin(_)
            | Self::Law(_)
            | Self::Net(_)
            | Self::G2Blend(_)
            | Self::VariableBlend(_)
            | Self::VertexBlend(_)
            | Self::LinearSweep(_)
            | Self::AxisRevolution(_)
            | Self::Helix { .. }
            | Self::Subset(_)
            | Self::Replica { .. }
            | Self::ParallelOffset(_)
            | Self::DegenerateTorus { .. }
            | Self::CurveBounded { .. }
            | Self::Ruled { .. }
            | Self::RollingBallJet(_)
            | Self::Unknown { .. } => RevisionCacheWrite::NoForm,
        }
    }

    /// Whether the construction owns a revision-gated cache form, which then
    /// states its solved-cache fit tolerance.
    #[must_use]
    pub fn owns_revision_cache(&self) -> bool {
        self.revision_cache().is_some() || matches!(self, Self::VariableBlend(..))
    }
}

/// What a write to a revision-gated cache form found.
enum RevisionCacheWrite {
    /// The form states a solved cache, whose tolerance the write states.
    Written,
    /// The form states a parameterization, which has no solved cache.
    Parameterized,
    /// The construction owns no revision-gated cache form.
    NoForm,
}

impl RevisionCacheWrite {
    /// The outcome the fit-tolerance setter states for this write.
    const fn into_set_result(self) -> Result<(), CacheContractError> {
        match self {
            Self::Written => Ok(()),
            Self::Parameterized => Err(CacheContractError::Parameterized),
            Self::NoForm => Err(CacheContractError::Layout(NO_LEGACY_SLOT)),
        }
    }
}

/// Whether a tolerance write states the value or only raises to it.
#[derive(Clone, Copy)]
enum ToleranceWrite {
    /// State the value in place of the tolerance the form carries.
    State,
    /// Keep the tolerance the form carries unless the value exceeds it.
    Raise,
}

/// State the fit tolerance of an optional solved cache form.
fn write_revision_form_tolerance<P>(
    form: Option<&mut RevisionCacheForm<P>>,
    value: FitTolerance,
    write: ToleranceWrite,
) -> RevisionCacheWrite {
    form.map_or(RevisionCacheWrite::NoForm, |form| {
        form.write_fit_tolerance(value, write)
    })
}

/// The outcome of clearing the fit tolerance of a revision-gated cache form: a
/// solved cache cannot lose the tolerance it states, and a parameterized form
/// states none to lose.
const fn clear_revision_form_tolerance<P>(
    form: Option<&RevisionCacheForm<P>>,
) -> Result<(), CacheContractError> {
    match form {
        Some(RevisionCacheForm::SolvedCache { .. }) => Err(CacheContractError::MissingSolved),
        Some(RevisionCacheForm::Parameterization(_)) | None => Ok(()),
    }
}

/// A finite, non-negative fit tolerance.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "f64", into = "f64")]
pub struct FitTolerance(f64);

impl FitTolerance {
    /// A zero fit tolerance.
    pub const ZERO: Self = Self(0.0);

    /// Admit a finite, non-negative fit tolerance.
    pub fn try_new(value: f64) -> Result<Self, CacheContractError> {
        if value.is_finite() && value >= 0.0 {
            Ok(Self(value))
        } else {
            Err(CacheContractError::InvalidValue { value })
        }
    }

    /// The fit tolerance in carrier units.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// The tolerance times `scale`.
    ///
    /// A positive scale keeps the sign of a non-negative tolerance: zero
    /// stays zero, and rounding does not change a sign. The product is
    /// refused only when it overflows.
    #[must_use]
    pub fn scaled(self, scale: crate::scalar::PositiveReal) -> Option<Self> {
        let value = self.0 * scale.get();
        value.is_finite().then_some(Self(value))
    }
}

impl TryFrom<f64> for FitTolerance {
    type Error = CacheContractError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<FitTolerance> for f64 {
    fn from(value: FitTolerance) -> Self {
        value.get()
    }
}

/// A fit tolerance admits exactly the finite, non-negative values.
impl From<NonNegativeReal> for FitTolerance {
    fn from(value: NonNegativeReal) -> Self {
        Self(value.get())
    }
}

/// Fit contract of a solved cache that the construction states itself, used by
/// constructions with no revision-gated [`RevisionCacheForm`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LegacyCache {
    /// Fit tolerance of the solved cache.
    pub fit_tolerance: FitTolerance,
}

impl LegacyCache {
    /// A legacy cache with this fit tolerance.
    #[must_use]
    pub const fn new(fit_tolerance: FitTolerance) -> Self {
        Self { fit_tolerance }
    }

    /// A legacy cache stating an admissible fit tolerance.
    pub fn try_new(fit_tolerance: f64) -> Result<Self, CacheContractError> {
        FitTolerance::try_new(fit_tolerance).map(Self::new)
    }
}

/// The cache contract of a construction that has both a legacy and a
/// revision-gated layout: the tolerance is stated in exactly one of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "layout", rename_all = "snake_case", deny_unknown_fields)]
pub enum CacheContract<F> {
    /// Legacy layout, which states its own solved-cache tolerance.
    Legacy {
        /// Solved-cache fit contract, absent when the record stated none.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    /// Revision-gated layout, which states the tolerance in its cache form.
    Revision {
        /// Revision-gated form.
        form: F,
    },
}

impl<F> CacheContract<F> {
    /// A legacy layout with no solved-cache tolerance.
    #[must_use]
    pub const fn legacy() -> Self {
        Self::Legacy { cache: None }
    }

    /// The contract an optional revision-gated form states.
    #[must_use]
    pub fn from_form(form: Option<F>) -> Self {
        match form {
            Some(form) => Self::Revision { form },
            None => Self::legacy(),
        }
    }

    /// The revision-gated form, absent in the legacy layout.
    #[must_use]
    pub const fn form(&self) -> Option<&F> {
        match self {
            Self::Revision { form } => Some(form),
            Self::Legacy { .. } => None,
        }
    }

    /// Mutable revision-gated form, absent in the legacy layout.
    pub const fn form_mut(&mut self) -> Option<&mut F> {
        match self {
            Self::Revision { form } => Some(form),
            Self::Legacy { .. } => None,
        }
    }

    /// The legacy solved-cache tolerance, absent in the revision layout.
    #[must_use]
    pub const fn legacy_fit_tolerance(&self) -> Option<FitTolerance> {
        match self {
            Self::Legacy { cache: Some(cache) } => Some(cache.fit_tolerance),
            Self::Legacy { cache: None } | Self::Revision { .. } => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent in the revision layout.
    const fn legacy_cache_mut(&mut self) -> Option<&mut Option<LegacyCache>> {
        match self {
            Self::Legacy { cache } => Some(cache),
            Self::Revision { .. } => None,
        }
    }
}

impl<F> CacheContract<F> {
    /// Whether this is the legacy layout with no stated tolerance, which is
    /// what a record that states neither carries.
    #[must_use]
    pub const fn is_bare_legacy(&self) -> bool {
        matches!(self, Self::Legacy { cache: None })
    }
}

impl<F> Default for CacheContract<F> {
    fn default() -> Self {
        Self::legacy()
    }
}

/// A fit tolerance, or the cache contract that states it, is not admissible.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CacheContractError {
    /// A fit tolerance is negative or non-finite.
    #[error("fit_tolerance must be finite and non-negative, got {value}")]
    InvalidValue {
        /// Rejected tolerance.
        value: f64,
    },
    /// A parameterized cache form states no solved-cache tolerance.
    #[error("a parameterized cache form states no fit tolerance")]
    Parameterized,
    /// A solved cache form cannot lose its required tolerance.
    #[error("a solved cache form states a fit tolerance")]
    MissingSolved,
    /// The construction's layout states no solved-cache tolerance.
    #[error("{0}")]
    Layout(&'static str),
}

impl From<CacheContractError> for cadmpeg_core::CodecError {
    fn from(error: CacheContractError) -> Self {
        Self::Malformed(error.to_string())
    }
}

/// A rejected procedural definition or cache contract.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProceduralGeometryError {
    /// A local construction field violates its numeric contract.
    #[error("{0}")]
    Payload(&'static str),
    /// The cache contract is invalid for the definition.
    #[error(transparent)]
    Cache(#[from] CacheContractError),
    /// A member list violates its member contract.
    #[error(transparent)]
    Members(#[from] crate::features::BodySelectionError),
}

/// A construction whose layout states no solved-cache fit tolerance.
const NO_LEGACY_SLOT: &str = "this construction states no solved-cache fit tolerance";

const PARAMETERIZED_NO_SOLVED_CACHE: &str =
    "a parameterized cache form takes no solved-cache fit tolerance";

/// A construction whose layout always states one.
const REQUIRED_LEGACY_SLOT: &str = "this construction states a solved-cache fit tolerance";

/// The legacy solved-cache slot of one surface construction, borrowed for
/// writing.
///
/// A layout that may omit the cache lends `Optional`; a layout that always
/// states one lends `Required`, which is why losing the cache is the one
/// refusal a slot makes. Surface-only: `ProceduralCurveDefinition` has no
/// `Required` slot, so a curve lends its slot as a plain
/// `&mut Option<LegacyCache>`.
enum LegacyCacheSlot<'a> {
    /// Slot of a layout whose record may state no cache.
    Optional(&'a mut Option<LegacyCache>),
    /// Slot of a layout that always states a cache.
    Required(&'a mut LegacyCache),
}

impl LegacyCacheSlot<'_> {
    /// Replace the cache the slot holds.
    const fn set(&mut self, cache: Option<LegacyCache>) -> Result<(), CacheContractError> {
        match (self, cache) {
            (Self::Optional(slot), cache) => {
                **slot = cache;
                Ok(())
            }
            (Self::Required(slot), Some(cache)) => {
                **slot = cache;
                Ok(())
            }
            (Self::Required(_), None) => Err(CacheContractError::Layout(REQUIRED_LEGACY_SLOT)),
        }
    }
}

impl ProceduralSurfaceDefinition {
    /// Solved-cache fit contract this construction states outside a
    /// revision-gated cache form.
    #[must_use]
    pub fn legacy_cache(&self) -> Option<LegacyCache> {
        match self {
            Self::Exact(payload) => payload.legacy_cache(),
            Self::Compound(payload) => payload.legacy_cache(),
            Self::Taper(payload) => payload.legacy_cache(),
            Self::Loft(payload) => payload.legacy_cache(),
            Self::CompoundLoft(payload) => payload.legacy_cache(),
            Self::ScaledCompoundLoft(payload) => payload.legacy_cache(),
            Self::Skin(payload) => payload.legacy_cache(),
            Self::Law(payload) => payload.legacy_cache(),
            Self::Net(payload) => payload.legacy_cache(),
            Self::G2Blend(payload) => payload.legacy_cache(),
            Self::Extrusion(payload) => payload.legacy_cache(),
            Self::Revolution(payload) => payload.legacy_cache(),
            Self::Sum(payload) => payload.legacy_cache(),
            Self::Sweep(payload) => payload.legacy_cache(),
            Self::TSpline { construction } => construction.legacy_cache(),
            Self::Deformable(payload) => payload.legacy_cache(),
            Self::Offset(payload) => payload.legacy_cache(),
            Self::Subset(payload) => payload.legacy_cache(),
            Self::Blend(payload) => payload.legacy_cache(),
            Self::Ruled { cache, .. } | Self::Unknown { cache, .. } => *cache,
            Self::SubSurface(_)
            | Self::RevisionCompoundLoft { .. }
            | Self::RevisionG2Blend { .. }
            | Self::VariableBlend(_)
            | Self::VertexBlend(_)
            | Self::LinearSweep(_)
            | Self::AxisRevolution(_)
            | Self::Helix { .. }
            | Self::Replica { .. }
            | Self::ParallelOffset(_)
            | Self::DegenerateTorus { .. }
            | Self::CurveBounded { .. }
            | Self::RollingBallJet(_) => None,
        }
    }

    /// Mutable legacy solved-cache slot of this construction, absent when the
    /// layout states none.
    ///
    /// The one write route to the legacy cache: both the setter and the raise
    /// go through it, so a construction cannot answer them differently.
    fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        match self {
            Self::Exact(payload) => payload.legacy_cache_slot_mut(),
            Self::Compound(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Taper(payload) => payload.legacy_cache_slot_mut(),
            Self::Loft(payload) => payload.legacy_cache_slot_mut(),
            Self::CompoundLoft(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::ScaledCompoundLoft(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Skin(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Law(payload) => payload.legacy_cache_slot_mut(),
            Self::Net(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::G2Blend(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Extrusion(payload) => payload.legacy_cache_slot_mut(),
            Self::Revolution(payload) => payload.legacy_cache_slot_mut(),
            Self::Sum(payload) => payload.legacy_cache_slot_mut(),
            Self::Sweep(payload) => payload.legacy_cache_slot_mut(),
            Self::TSpline { construction } => construction.legacy_cache_slot_mut(),
            Self::Deformable(payload) => payload.legacy_cache_slot_mut(),
            Self::Offset(payload) => payload.legacy_cache_slot_mut(),
            Self::Subset(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Blend(payload) => payload.legacy_cache_slot_mut(),
            Self::Ruled { cache, .. } | Self::Unknown { cache, .. } => {
                Some(LegacyCacheSlot::Optional(cache))
            }
            Self::SubSurface(_)
            | Self::RevisionCompoundLoft { .. }
            | Self::RevisionG2Blend { .. }
            | Self::VariableBlend(_)
            | Self::VertexBlend(_)
            | Self::LinearSweep(_)
            | Self::AxisRevolution(_)
            | Self::Helix { .. }
            | Self::Replica { .. }
            | Self::ParallelOffset(_)
            | Self::DegenerateTorus { .. }
            | Self::CurveBounded { .. }
            | Self::RollingBallJet(_) => None,
        }
    }

    /// State or clear the legacy solved-cache fit contract.
    ///
    /// Fallible in both directions, because a surface construction can hold a
    /// cache it cannot lose. A layout that states none refuses `Some`, and a
    /// `LawSurfaceTail::Full` tail, whose `Required` slot always states a
    /// tolerance, refuses `None`.
    pub fn set_legacy_cache(
        &mut self,
        cache: Option<LegacyCache>,
    ) -> Result<(), CacheContractError> {
        match self.legacy_cache_slot_mut() {
            Some(mut slot) => slot.set(cache),
            None => cache.map_or(Ok(()), |_| Err(CacheContractError::Layout(NO_LEGACY_SLOT))),
        }
    }

    /// Effective fit tolerance of the solved cache: a revision-gated form
    /// states it in its cache form, a legacy layout in the cache it holds.
    #[must_use]
    pub fn cache_fit_tolerance(&self) -> Option<FitTolerance> {
        if let Self::VariableBlend(payload) = self {
            return payload.construction().cache.fit_tolerance();
        }
        match self.revision_cache() {
            Some(RevisionCacheForm::SolvedCache { fit_tolerance }) => Some(*fit_tolerance),
            Some(RevisionCacheForm::Parameterization(_)) => None,
            None => self.legacy_cache().map(|cache| cache.fit_tolerance),
        }
    }

    /// Change the effective fit tolerance without permitting a parameterized
    /// cache to acquire one or a solved cache to lose it.
    pub fn set_cache_fit_tolerance(
        &mut self,
        value: Option<FitTolerance>,
    ) -> Result<(), CacheContractError> {
        if let Self::VariableBlend(payload) = self {
            return payload.write_cache_fit_tolerance(value);
        }
        if self.owns_revision_cache() {
            return match value {
                Some(value) => self
                    .write_revision_fit_tolerance(value, ToleranceWrite::State)
                    .into_set_result(),
                None => clear_revision_form_tolerance(self.revision_cache()),
            };
        }
        self.set_legacy_cache(value.map(LegacyCache::new))
    }
}

impl ProceduralCurveDefinition {
    /// Solved-cache fit contract this construction states outside a
    /// revision-gated cache form.
    #[must_use]
    pub fn legacy_cache(&self) -> Option<LegacyCache> {
        match self {
            Self::Compound(construction) => construction.legacy_cache(),
            Self::Helix(construction) => construction.legacy_cache(),
            Self::Subset(payload) => payload.legacy_cache(),
            Self::VectorOffset(payload) => payload.legacy_cache(),
            Self::TwoSidedOffset(payload) => payload.legacy_cache(),
            Self::Spring(payload) => payload.legacy_cache(),
            Self::SurfaceOffset(payload) => payload.legacy_cache(),
            Self::Exact { cache }
            | Self::Law { cache, .. }
            | Self::Intersection { cache, .. }
            | Self::TolerantIntersection { cache, .. }
            | Self::Unknown { cache, .. } => *cache,
            Self::ThreeSurfaceIntersection(_)
            | Self::SurfaceCurve { .. }
            | Self::Silhouette(_)
            | Self::Deformable(_)
            | Self::Projection(_)
            | Self::Offset(_)
            | Self::SpatialOffset(_)
            | Self::Replica { .. }
            | Self::BlendSpine { .. } => None,
        }
    }

    /// Mutable legacy solved-cache slot of this construction, absent when the
    /// layout states none.
    ///
    /// The one write route to the legacy cache: the setter, the clear and the
    /// raise go through it, so a construction cannot answer them differently.
    /// No curve construction holds a cache it cannot lose, so the slot is a
    /// plain `&mut Option<LegacyCache>` and `LegacyCacheSlot` is surface-only.
    fn legacy_cache_slot_mut(&mut self) -> Option<&mut Option<LegacyCache>> {
        match self {
            Self::Compound(construction) => Some(construction.legacy_cache_slot_mut()),
            Self::Helix(construction) => Some(construction.legacy_cache_slot_mut()),
            Self::Subset(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::VectorOffset(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::TwoSidedOffset(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Spring(payload) => payload.legacy_cache_slot_mut(),
            Self::SurfaceOffset(payload) => payload.legacy_cache_slot_mut(),
            Self::Exact { cache }
            | Self::Law { cache, .. }
            | Self::Intersection { cache, .. }
            | Self::TolerantIntersection { cache, .. }
            | Self::Unknown { cache, .. } => Some(cache),
            Self::ThreeSurfaceIntersection(_)
            | Self::SurfaceCurve { .. }
            | Self::Silhouette(_)
            | Self::Deformable(_)
            | Self::Projection(_)
            | Self::Offset(_)
            | Self::SpatialOffset(_)
            | Self::Replica { .. }
            | Self::BlendSpine { .. } => None,
        }
    }

    /// State the legacy solved-cache fit contract. A construction whose
    /// layout states none refuses a contract.
    pub fn set_legacy_cache(&mut self, cache: LegacyCache) -> Result<(), CacheContractError> {
        match self.legacy_cache_slot_mut() {
            Some(slot) => {
                *slot = Some(cache);
                Ok(())
            }
            None => Err(CacheContractError::Layout(NO_LEGACY_SLOT)),
        }
    }

    /// Clear the legacy solved-cache fit contract.
    ///
    /// Total: a construction with a slot loses the contract it states, and a
    /// construction with no slot has none to lose. No curve construction holds
    /// a cache it cannot lose, so this refuses nothing.
    pub fn clear_legacy_cache(&mut self) {
        if let Some(slot) = self.legacy_cache_slot_mut() {
            *slot = None;
        }
    }

    /// Effective fit tolerance of the solved cache.
    #[must_use]
    pub fn cache_fit_tolerance(&self) -> Option<FitTolerance> {
        match self.revision_cache() {
            Some(RevisionCacheForm::SolvedCache { fit_tolerance }) => Some(*fit_tolerance),
            Some(RevisionCacheForm::Parameterization(_)) => None,
            None => self.legacy_cache().map(|cache| cache.fit_tolerance),
        }
    }

    /// Change the effective fit tolerance without permitting a parameterized
    /// cache to acquire one or a solved cache to lose it.
    pub fn set_cache_fit_tolerance(
        &mut self,
        value: Option<FitTolerance>,
    ) -> Result<(), CacheContractError> {
        if self.owns_revision_cache() {
            return match value {
                Some(value) => self
                    .write_revision_fit_tolerance(value, ToleranceWrite::State)
                    .into_set_result(),
                None => clear_revision_form_tolerance(self.revision_cache()),
            };
        }
        match value {
            Some(value) => self.set_legacy_cache(LegacyCache::new(value)),
            None => {
                self.clear_legacy_cache();
                Ok(())
            }
        }
    }

    /// State that a solved carrier of this construction was fitted to `value`,
    /// raising an existing contract rather than lowering it.
    ///
    /// Answers every layout. A construction whose legacy slot is empty takes
    /// the contract; one that already carries a cache raises it; a solved
    /// revision cache raises its own tolerance. A parameterized form and a
    /// layout that states no slot have no solved cache to state, so they
    /// refuse: a caller that asked for a solved cache never gets a silent
    /// nothing. The write goes through `legacy_cache_slot_mut`, the one write
    /// route to the slot, so a construction cannot answer it differently from
    /// the setter and the clear.
    pub fn require_cache_fit_tolerance(
        &mut self,
        value: FitTolerance,
    ) -> Result<(), CacheContractError> {
        match self.write_revision_fit_tolerance(value, ToleranceWrite::Raise) {
            RevisionCacheWrite::Written => Ok(()),
            RevisionCacheWrite::Parameterized => {
                Err(CacheContractError::Layout(PARAMETERIZED_NO_SOLVED_CACHE))
            }
            RevisionCacheWrite::NoForm => match self.legacy_cache_slot_mut() {
                Some(slot @ None) => {
                    *slot = Some(LegacyCache::new(value));
                    Ok(())
                }
                Some(Some(cache)) => {
                    if value.get() > cache.fit_tolerance.get() {
                        cache.fit_tolerance = value;
                    }
                    Ok(())
                }
                None => Err(CacheContractError::Layout(NO_LEGACY_SLOT)),
            },
        }
    }
}

fn set_variable_blend_cache(
    cache: &mut VariableBlendCache<FiniteReal>,
    value: Option<FitTolerance>,
) -> Result<(), CacheContractError> {
    match (cache, value) {
        (VariableBlendCache::Parameterization { .. }, Some(_)) => {
            Err(CacheContractError::Parameterized)
        }
        (VariableBlendCache::Parameterization { .. } | VariableBlendCache::Stale {}, None) => {
            Ok(())
        }
        (VariableBlendCache::Current { fit_tolerance, .. }, Some(value)) => {
            *fit_tolerance = value;
            Ok(())
        }
        (VariableBlendCache::Current { .. }, None) => Err(CacheContractError::MissingSolved),
        (VariableBlendCache::Stale {}, Some(_)) => Err(CacheContractError::Layout(
            "a stale variable-blend cache states no fit tolerance",
        )),
    }
}

impl ProceduralSurface {
    /// Build a procedural surface from its construction definition.
    pub fn new(
        id: ProceduralSurfaceId,
        definition: ProceduralSurfaceDefinition,
        record_bounds: Option<RecordBounds>,
    ) -> Self {
        Self {
            id,
            definition,
            record_bounds,
        }
    }

    /// Return the retained native record bounds, when present. The aggregate
    /// moves whole, so a reader that keeps it holds the finiteness the
    /// constructor established; `RecordBounds::get` opens it for computation.
    #[must_use]
    pub const fn record_bounds(&self) -> Option<RecordBounds> {
        self.record_bounds
    }

    /// Borrow the neutral construction definition.
    #[must_use]
    pub fn definition(&self) -> &ProceduralSurfaceDefinition {
        &self.definition
    }

    /// Edit the definition in place.
    pub fn edit_definition<R>(
        &mut self,
        edit: impl FnOnce(&mut ProceduralSurfaceDefinition) -> R,
    ) -> R {
        edit(&mut self.definition)
    }

    /// Effective fit tolerance of the solved cache.
    #[must_use]
    pub fn cache_fit_tolerance(&self) -> Option<FitTolerance> {
        self.definition.cache_fit_tolerance()
    }

    /// Change the effective fit tolerance without permitting a parameterized
    /// cache to acquire one or a solved cache to lose it.
    pub fn set_cache_fit_tolerance(
        &mut self,
        value: Option<f64>,
    ) -> Result<(), CacheContractError> {
        let value = value.map(FitTolerance::try_new).transpose()?;
        self.definition.set_cache_fit_tolerance(value)
    }

    /// Scale the effective cache-fit tolerance in place.
    ///
    /// A positive scale keeps the tolerance non-negative, so the scaled
    /// tolerance is refused only when it overflows, with the admission's own
    /// error.
    pub fn scale_cache_fit_tolerance(
        &mut self,
        scale: crate::scalar::PositiveReal,
    ) -> Result<(), CacheContractError> {
        if let Some(value) = self.cache_fit_tolerance() {
            let scaled = value
                .scaled(scale)
                .ok_or_else(|| CacheContractError::InvalidValue {
                    value: value.get() * scale.get(),
                })?;
            self.definition.set_cache_fit_tolerance(Some(scaled))?;
        }
        Ok(())
    }
}

/// Structurally selected deformable-surface payload.
// A source states raw values; a `DeformableSurfacePayload` holds the admitted
// payload, whose scalars, vectors and points are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeformableSurfaceData<R = f64, V = Vector3, P = Point3> {
    /// Mode-6 full embedded deformation payload.
    Full {
        /// Four leading deformation vectors.
        leading_vectors: [V; 4],
        /// Leading deformation scalar.
        leading_parameter: R,
        /// Three leading flags.
        leading_flags: [bool; 3],
        /// Native selector before the secondary support.
        selector: i64,
        /// Secondary embedded support surface.
        surface: SurfaceId,
        /// Native long after the support.
        native_id: i64,
        /// Native support-side flag.
        flag: bool,
        /// First scalar after the flag.
        first_parameter: R,
        /// Version-gated ASM long when present.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        version_value: Option<i64>,
        /// Second scalar after the optional long.
        second_parameter: R,
        /// Embedded deformation curve.
        curve: CurveId,
        /// Two ordered full vector frames.
        frames: Box<[DeformableVectorFrame<R, V>; 2]>,
        /// Native trailing long.
        trailing_value: i64,
    },
    /// Mode-5 surface-and-curve deformation payload.
    SurfaceCurve {
        /// Secondary embedded support surface.
        surface: SurfaceId,
        /// Native long identifier.
        native_id: i64,
        /// Native leading flag.
        flag: bool,
        /// First native scalar.
        first_parameter: R,
        /// Native selector integer.
        selector: i64,
        /// Second native scalar.
        second_parameter: R,
        /// Embedded deformation curve.
        curve: CurveId,
        /// Four ordered deformation vectors.
        vectors: [V; 4],
        /// Frame scalar after the vectors.
        frame_parameter: R,
        /// Three frame flags.
        flags: [bool; 3],
        /// Counted ordered scalar triples.
        parameter_triples: Vec<[R; 3]>,
    },
    /// Mode-1 deformation frame with counted parameter triples.
    Plain {
        /// Shared full deformation frame.
        frame: Box<DeformableSurfaceFrame<R, V, P>>,
        /// Ordered native scalar triples.
        parameter_triples: Vec<[R; 3]>,
    },
    /// Mode-3 deformation frame with a guide scalar.
    Guided {
        /// Shared full deformation frame.
        frame: Box<DeformableSurfaceFrame<R, V, P>>,
        /// Native guide selector.
        selector: i64,
        /// Native guide scalar.
        guide_parameter: R,
    },
    /// Mode-8 minimal four-vector scaffold.
    Minimal {
        /// Four ordered deformation vectors.
        vectors: [V; 4],
        /// Native trailing selector.
        selector: i64,
    },
    /// Revision-gated mode-3 deformation payload.
    RevisionMode3 {
        /// Four leading deformation vectors.
        leading_vectors: [V; 4],
        /// Scalar following the leading vectors.
        leading_parameter: R,
        /// Three flags following the leading scalar.
        leading_flags: [bool; 3],
        /// Position anchoring the trailing frame.
        trailing_point: P,
        /// Two vectors following the trailing point.
        trailing_vectors: [V; 2],
        /// Scalar following the trailing vectors.
        frame_parameter: R,
        /// Two flags following the trailing frame scalar.
        frame_flags: [bool; 2],
        /// Three ordered scalar parameters following the trailing frame.
        parameters: [R; 3],
        /// Five flags following the ordered scalar parameters.
        trailing_flags: [bool; 5],
        /// Scalar preceding the payload's final integer.
        trailing_parameter: R,
        /// Integer closing the revision mode-3 payload.
        trailing_value: i64,
    },
}

/// Four-vector frame used by full deformable surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeformableVectorFrame<R = f64, V = Vector3> {
    /// Four ordered vectors.
    pub vectors: [V; 4],
    /// Frame scalar.
    pub parameter: R,
    /// Three ordered flags.
    pub flags: [bool; 3],
}

/// Shared frame payload of deformable-surface modes 1 and 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeformableSurfaceFrame<R = f64, V = Vector3, P = Point3> {
    /// Four leading deformation vectors.
    pub leading_vectors: [V; 4],
    /// Leading frame scalar.
    pub leading_parameter: R,
    /// Three leading frame flags.
    pub leading_flags: [bool; 3],
    /// Three secondary deformation vectors.
    pub secondary_vectors: [V; 3],
    /// Secondary frame scalar.
    pub secondary_parameter: R,
    /// Two secondary frame flags.
    pub secondary_flags: [bool; 2],
    /// Native model-space frame point.
    pub point: P,
    /// Five trailing frame flags.
    pub trailing_flags: [bool; 5],
}

/// Complete native deformable-surface construction.
// A source states raw values; a `DeformableSurfacePayload` holds the admitted
// construction, whose scalars, vectors and points are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct DeformableSurfaceConstruction<R = f64, V = Vector3, P = Point3> {
    /// Surface being deformed.
    pub support: SurfaceId,
    /// Discriminator-selected deformation data.
    pub data: DeformableSurfaceData<R, V, P>,
    /// Cache contract: the revision-gated fields surrounding the support and
    /// shared surface tail, or the legacy solved-cache tolerance this
    /// construction states instead.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    pub cache: CacheContract<RevisionSurfaceForm<Vec<bool>, R>>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<R>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

const EPS_HELIX_SURFACE_RADIUS_RELATIVE: f64 = 1.0e-9;
const EPS_HELIX_CURVE_RADIUS: f64 = 1.0e-9;

/// Finite circular path of a helix surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixPathConstructionWire")]
pub struct HelixPathConstruction {
    angle_range: FiniteVector<2>,
    center: FinitePoint3,
    major: FiniteVector3,
    minor: FiniteVector3,
    pitch: FiniteVector3,
    apex_factor: FiniteReal,
    axis: FiniteVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HelixPathConstructionWire {
    /// Native helix angle interval.
    angle_range: [f64; 2],
    /// Helix center in model space.
    center: Point3,
    /// Major radial axis vector.
    major: Vector3,
    /// Minor radial axis vector.
    minor: Vector3,
    /// Axial advance of one turn, as a vector.
    pitch: Vector3,
    /// Native apex taper factor.
    apex_factor: f64,
    /// Helix axis direction.
    axis: Vector3,
}

/// The positioned frame a helix construction states.
///
/// The two helix constructions state the same five values, so they take them
/// as one argument and each admits them against its own contract.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HelixFrame {
    /// Helix centre in model space.
    pub center: Point3,
    /// Major radial vector.
    pub major: Vector3,
    /// Minor radial vector.
    pub minor: Vector3,
    /// Pitch vector along the axis.
    pub pitch: Vector3,
    /// Helix axis direction.
    pub axis: Vector3,
}

impl HelixPathConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        frame: HelixFrame,
        apex_factor: f64,
    ) -> Result<Self, &'static str> {
        let HelixFrame {
            center,
            major,
            minor,
            pitch,
            axis,
        } = frame;
        let major_length = major.norm();
        let minor_length = minor.norm();
        if !(major_length.is_finite()
            && minor_length.is_finite()
            && major_length > 0.0
            && minor_length > 0.0
            && (major_length - minor_length).abs()
                <= EPS_HELIX_SURFACE_RADIUS_RELATIVE * major_length.max(minor_length))
        {
            return Err("helix surface path major and minor must define a circular path");
        }

        let angle_range = FiniteVector::new(angle_range)
            .ok_or("HelixPathConstruction.angle_range must be finite")?;
        let center =
            FinitePoint3::new(center).ok_or("HelixPathConstruction.center must be finite")?;
        let major =
            FiniteVector3::new(major).ok_or("HelixPathConstruction.major must be finite")?;
        let minor =
            FiniteVector3::new(minor).ok_or("HelixPathConstruction.minor must be finite")?;
        let pitch =
            FiniteVector3::new(pitch).ok_or("HelixPathConstruction.pitch must be finite")?;
        let apex_factor = FiniteReal::new(apex_factor)
            .ok_or("HelixPathConstruction.apex_factor must be finite")?;
        let axis = FiniteVector3::new(axis).ok_or("HelixPathConstruction.axis must be finite")?;
        Ok(Self {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        })
    }
    /// Return the angle range.
    #[must_use]
    pub const fn angle_range(&self) -> &FiniteVector<2> {
        &self.angle_range
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &FinitePoint3 {
        &self.center
    }

    /// Return the major.
    #[must_use]
    pub const fn major(&self) -> &FiniteVector3 {
        &self.major
    }

    /// Return the minor.
    #[must_use]
    pub const fn minor(&self) -> &FiniteVector3 {
        &self.minor
    }

    /// Return the pitch.
    #[must_use]
    pub const fn pitch(&self) -> &FiniteVector3 {
        &self.pitch
    }

    /// Return the apex factor.
    #[must_use]
    pub const fn apex_factor(&self) -> FiniteReal {
        self.apex_factor
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &FiniteVector3 {
        &self.axis
    }
}

impl TryFrom<HelixPathConstructionWire> for HelixPathConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixPathConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.angle_range,
            HelixFrame {
                center: wire.center,
                major: wire.major,
                minor: wire.minor,
                pitch: wire.pitch,
                axis: wire.axis,
            },
            wire.apex_factor,
        )
    }
}

/// Finite ordered helix curve with a non-degenerate radial frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixCurveConstructionWire")]
pub struct HelixCurveConstruction {
    angle_range: FiniteVector<2>,
    center: FinitePoint3,
    major: FiniteVector3,
    minor: FiniteVector3,
    pitch: FiniteVector3,
    apex_factor: FiniteReal,
    axis: FiniteVector3,
    /// Solved-cache fit contract this construction states itself.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cache"
    )]
    cache: Option<LegacyCache>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HelixCurveConstructionWire {
    /// Native helix angle interval.
    angle_range: [f64; 2],
    /// Helix center in model space.
    center: Point3,
    /// Major radial axis vector.
    major: Vector3,
    /// Minor radial axis vector.
    minor: Vector3,
    /// Axial advance of one turn, as a vector.
    pitch: Vector3,
    /// Native apex taper factor.
    apex_factor: f64,
    /// Helix axis direction.
    axis: Vector3,
    /// Solved-cache fit contract this construction states itself.
    #[serde(default, deserialize_with = "deserialize_cache")]
    cache: Option<LegacyCache>,
}

impl HelixCurveConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        frame: HelixFrame,
        apex_factor: f64,
        cache: Option<LegacyCache>,
    ) -> Result<Self, &'static str> {
        let HelixFrame {
            center,
            major,
            minor,
            pitch,
            axis,
        } = frame;
        if angle_range[0] > angle_range[1] {
            return Err("helix curve angle_range must be ordered");
        }
        let major_radius = major.norm();
        let minor_radius = minor.norm();
        if major_radius <= f64::EPSILON
            || minor_radius <= f64::EPSILON
            || axis.norm() <= f64::EPSILON
        {
            return Err("helix curve major, minor, and axis must be non-degenerate");
        }
        if major.is_finite()
            && minor.is_finite()
            && (!major_radius.is_finite()
                || !minor_radius.is_finite()
                || (major_radius - minor_radius).abs() > EPS_HELIX_CURVE_RADIUS)
        {
            return Err("helix curve major and minor radii must agree");
        }

        let angle_range = FiniteVector::new(angle_range)
            .ok_or("HelixCurveConstruction.angle_range must be finite")?;
        let center =
            FinitePoint3::new(center).ok_or("HelixCurveConstruction.center must be finite")?;
        let major =
            FiniteVector3::new(major).ok_or("HelixCurveConstruction.major must be finite")?;
        let minor =
            FiniteVector3::new(minor).ok_or("HelixCurveConstruction.minor must be finite")?;
        let pitch =
            FiniteVector3::new(pitch).ok_or("HelixCurveConstruction.pitch must be finite")?;
        let apex_factor = FiniteReal::new(apex_factor)
            .ok_or("HelixCurveConstruction.apex_factor must be finite")?;
        let axis = FiniteVector3::new(axis).ok_or("HelixCurveConstruction.axis must be finite")?;
        Ok(Self {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
            cache,
        })
    }
    /// Return the angle range.
    #[must_use]
    pub const fn angle_range(&self) -> &FiniteVector<2> {
        &self.angle_range
    }

    /// Borrow the admitted center.
    #[must_use]
    pub const fn center(&self) -> &FinitePoint3 {
        &self.center
    }

    /// Return the major.
    #[must_use]
    pub const fn major(&self) -> &FiniteVector3 {
        &self.major
    }

    /// Return the minor.
    #[must_use]
    pub const fn minor(&self) -> &FiniteVector3 {
        &self.minor
    }

    /// Return the pitch.
    #[must_use]
    pub const fn pitch(&self) -> &FiniteVector3 {
        &self.pitch
    }

    /// Return the apex factor.
    #[must_use]
    pub const fn apex_factor(&self) -> FiniteReal {
        self.apex_factor
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &FiniteVector3 {
        &self.axis
    }
}

impl TryFrom<HelixCurveConstructionWire> for HelixCurveConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.angle_range,
            HelixFrame {
                center: wire.center,
                major: wire.major,
                minor: wire.minor,
                pitch: wire.pitch,
                axis: wire.axis,
            },
            wire.apex_factor,
            wire.cache,
        )
    }
}

impl HelixCurveConstruction {
    /// Reverse the parameter while preserving points and derivatives.
    /// A zero radius at the new interval start has no admitted helix frame.
    pub fn try_reverse_parameterization(&mut self) -> Result<(), &'static str> {
        let [start, end] = self.angle_range.get();
        let turns = (end - start) / std::f64::consts::TAU;
        let radial_end = self.apex_factor.get().mul_add(turns, 1.0);
        if !turns.is_finite() || !radial_end.is_finite() || radial_end == 0.0 {
            return Err("helix curve reversal has no finite nonzero starting radius");
        }
        let scale = |vector: Vector3| {
            Vector3::new(
                vector.x * radial_end,
                vector.y * radial_end,
                vector.z * radial_end,
            )
        };
        let candidate = Self::try_new(
            self.angle_range.reversed_negated().get(),
            HelixFrame {
                center: self.center.get().translated(self.pitch.get(), turns),
                major: scale(self.major.get()),
                minor: scale(self.minor.negated().get()),
                pitch: self.pitch.negated().get(),
                axis: self.axis.get(),
            },
            -self.apex_factor.get() / radial_end,
            self.cache,
        )?;
        *self = candidate;
        Ok(())
    }

    /// Scale lengths atomically and retain the old path when admission fails.
    pub fn try_scale_lengths(&mut self, scale: f64) -> Result<(), &'static str> {
        let vector =
            |value: Vector3| Vector3::new(value.x * scale, value.y * scale, value.z * scale);
        let candidate = Self::try_new(
            self.angle_range.get(),
            HelixFrame {
                center: Point3::new(
                    self.center.x * scale,
                    self.center.y * scale,
                    self.center.z * scale,
                ),
                major: vector(self.major.get()),
                minor: vector(self.minor.get()),
                pitch: vector(self.pitch.get()),
                axis: self.axis.get(),
            },
            self.apex_factor.get(),
            // The rebuilt construction is minted fresh; the solved-cache
            // contract this construction states travels with it.
            self.cache,
        )?;
        *self = candidate;
        Ok(())
    }
}

/// Finite circular helix profile with a nonzero signed radius.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixCircleProfileWire")]
pub struct HelixCircleProfile {
    length: FiniteReal,
    radius: FiniteReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HelixCircleProfileWire {
    /// Native profile length.
    length: f64,
    /// Signed circular profile radius.
    radius: f64,
}

impl HelixCircleProfile {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(length: f64, radius: f64) -> Result<Self, &'static str> {
        if radius == 0.0 {
            return Err("helix circle profile radius must be finite and nonzero");
        }
        let length = FiniteReal::new(length).ok_or("helix circle profile length must be finite")?;
        let radius = FiniteReal::new(radius).ok_or("helix circle profile radius must be finite")?;
        Ok(Self { length, radius })
    }
    /// Native profile length.
    #[must_use]
    pub const fn length(&self) -> FiniteReal {
        self.length
    }

    /// Signed circular profile radius.
    #[must_use]
    pub const fn radius(&self) -> FiniteReal {
        self.radius
    }
}

impl TryFrom<HelixCircleProfileWire> for HelixCircleProfile {
    type Error = &'static str;
    fn try_from(wire: HelixCircleProfileWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.length, wire.radius)
    }
}

/// Finite non-degenerate linear helix profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixLineProfileWire")]
pub struct HelixLineProfile {
    direction: FiniteVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HelixLineProfileWire {
    /// Finite non-degenerate profile direction.
    direction: Vector3,
}

impl HelixLineProfile {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(direction: Vector3) -> Result<Self, &'static str> {
        const INVALID: &str = "helix line profile direction must be finite and non-degenerate";
        let direction = FiniteVector3::new(direction).ok_or(INVALID)?;
        if direction.x == 0.0 && direction.y == 0.0 && direction.z == 0.0 {
            return Err(INVALID);
        }
        Ok(Self { direction })
    }
    /// Finite non-degenerate profile direction.
    #[must_use]
    pub const fn direction(&self) -> FiniteVector3 {
        self.direction
    }
}

impl TryFrom<HelixLineProfileWire> for HelixLineProfile {
    type Error = &'static str;
    fn try_from(wire: HelixLineProfileWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.direction)
    }
}

/// Profile-specific tail of a helix surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HelixSurfaceProfile {
    /// Circular profile swept along the helix.
    Circle(HelixCircleProfile),
    /// Linear profile swept along a direction.
    Line(HelixLineProfile),
}

/// Complete helix-surface construction with finite native intervals.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixSurfaceConstructionWire")]
pub struct HelixSurfaceConstruction {
    angle_range: FiniteVector<2>,
    dimension_range: FiniteVector<2>,
    path: HelixPathConstruction,
    profile: HelixSurfaceProfile,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HelixSurfaceConstructionWire {
    /// Native helix angle interval.
    angle_range: [f64; 2],
    /// Native profile dimension interval.
    dimension_range: [f64; 2],
    /// Circular path of the helix surface.
    path: HelixPathConstruction,
    /// Profile swept along the path.
    profile: HelixSurfaceProfile,
}

impl HelixSurfaceConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        dimension_range: [f64; 2],
        path: HelixPathConstruction,
        profile: HelixSurfaceProfile,
    ) -> Result<Self, &'static str> {
        let angle_range =
            FiniteVector::new(angle_range).ok_or("helix surface angle_range must be finite")?;
        let dimension_range = FiniteVector::new(dimension_range)
            .ok_or("helix surface dimension_range must be finite")?;
        Ok(Self {
            angle_range,
            dimension_range,
            path,
            profile,
        })
    }
    /// Return the angle range.
    #[must_use]
    pub const fn angle_range(&self) -> &FiniteVector<2> {
        &self.angle_range
    }

    /// Return the dimension range.
    #[must_use]
    pub const fn dimension_range(&self) -> &FiniteVector<2> {
        &self.dimension_range
    }

    /// Return the path.
    #[must_use]
    pub const fn path(&self) -> &HelixPathConstruction {
        &self.path
    }

    /// Return the profile.
    #[must_use]
    pub const fn profile(&self) -> &HelixSurfaceProfile {
        &self.profile
    }
}

impl TryFrom<HelixSurfaceConstructionWire> for HelixSurfaceConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.angle_range,
            wire.dimension_range,
            wire.path,
            wire.profile,
        )
    }
}

/// A non-negative native subtype-table index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "i64", into = "i64")]
pub struct SubtypeTableIndex(i64);

impl SubtypeTableIndex {
    /// Admit a non-negative native subtype-table index.
    pub fn try_new(index: i64) -> Result<Self, &'static str> {
        if index >= 0 {
            Ok(Self(index))
        } else {
            Err("subtype table index must be non-negative")
        }
    }

    /// Native subtype-table index.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for SubtypeTableIndex {
    type Error = &'static str;
    fn try_from(index: i64) -> Result<Self, Self::Error> {
        Self::try_new(index)
    }
}

impl From<SubtypeTableIndex> for i64 {
    fn from(index: SubtypeTableIndex) -> Self {
        index.get()
    }
}

/// An inline T-spline program and its non-empty companion values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    rename = "inline",
    try_from = "InlineTSplineSubtransformWire"
)]
pub struct InlineTSplineSubtransform {
    /// Line-oriented topology and geometry program.
    pub program: cadmpeg_core::text::NonBlankString,
    /// Optional native separator boolean.
    pub separator: Option<bool>,
    /// Companion values program.
    pub values: cadmpeg_core::text::NonBlankString,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
enum InlineTSplineSubtransformWire {
    /// Inline line-oriented T-spline program and companion values.
    Inline {
        /// Line-oriented topology and geometry program.
        program: String,
        /// Optional native separator boolean.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        separator: Option<bool>,
        /// Companion values program.
        values: String,
    },
}

impl TryFrom<InlineTSplineSubtransformWire> for InlineTSplineSubtransform {
    type Error = &'static str;
    fn try_from(wire: InlineTSplineSubtransformWire) -> Result<Self, Self::Error> {
        let InlineTSplineSubtransformWire::Inline {
            program,
            separator,
            values,
        } = wire;
        Self::try_new(program, separator, values)
    }
}

impl InlineTSplineSubtransform {
    /// Admit a non-empty T-spline program and companion values.
    pub fn try_new(
        program: impl Into<String>,
        separator: Option<bool>,
        values: impl Into<String>,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            program: cadmpeg_core::text::NonBlankString::new(program)
                .ok_or("T-spline program must not be empty")?,
            separator,
            values: cadmpeg_core::text::NonBlankString::new(values)
                .ok_or("T-spline values must not be empty")?,
        })
    }
}

/// Native T-spline subtransform storage form.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TSplineSubtransformWire")]
pub enum TSplineSubtransform {
    /// Inline line-oriented T-spline program and companion values.
    Inline(InlineTSplineSubtransform),
    /// Resolved reference to an earlier subtype-table entry.
    Resolved {
        /// Native subtype-table index.
        index: SubtypeTableIndex,
        /// Resolved shared program.
        transform: Box<InlineTSplineSubtransform>,
    },
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
enum TSplineSubtransformWire {
    /// Inline line-oriented T-spline program and companion values.
    Inline {
        /// Line-oriented topology and geometry program.
        program: String,
        /// Optional native separator boolean.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        separator: Option<bool>,
        /// Companion values program.
        values: String,
    },
    /// Resolved reference to an earlier subtype-table entry.
    Reference {
        /// Native subtype-table index.
        index: SubtypeTableIndex,
        /// Resolved shared program.
        resolved: Box<InlineTSplineSubtransform>,
    },
}

impl TryFrom<TSplineSubtransformWire> for TSplineSubtransform {
    type Error = &'static str;
    fn try_from(wire: TSplineSubtransformWire) -> Result<Self, Self::Error> {
        match wire {
            TSplineSubtransformWire::Inline {
                program,
                separator,
                values,
            } => InlineTSplineSubtransform::try_new(program, separator, values).map(Self::Inline),
            TSplineSubtransformWire::Reference { index, resolved } => Ok(Self::Resolved {
                index,
                transform: resolved,
            }),
        }
    }
}

impl Serialize for TSplineSubtransform {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum Wire<'a> {
            Reference {
                index: SubtypeTableIndex,
                resolved: &'a InlineTSplineSubtransform,
            },
        }
        match self {
            Self::Inline(inline) => inline.serialize(serializer),
            Self::Resolved { index, transform } => Wire::Reference {
                index: *index,
                resolved: transform,
            }
            .serialize(serializer),
        }
    }
}

impl TSplineSubtransform {
    /// Effective inline program, including resolved references.
    #[must_use]
    pub fn inline(&self) -> &InlineTSplineSubtransform {
        match self {
            Self::Inline(inline) => inline,
            Self::Resolved { transform, .. } => transform,
        }
    }
}

/// Complete native `t_spl_sur` wrapper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "TSplineSurfaceConstructionWire",
    into = "TSplineSurfaceConstructionWire"
)]
pub struct TSplineSurfaceConstruction {
    /// Ordered U and V native parameter intervals.
    parameter_ranges: [crate::topology::ParameterInterval; 2],
    /// Native T-spline type integer.
    type_code: i64,
    /// Inline or referenced shared subtransform object.
    subtransform: TSplineSubtransform,
    /// Native trailing integer.
    trailing_value: i64,
    /// Six ordered solved-surface discontinuity arrays.
    discontinuities: [Vec<FiniteReal>; 6],
    /// Native discontinuity tail flag.
    discontinuity_flag: bool,
    /// Cache contract: the revision-gated form, or the legacy solved-cache
    /// tolerance this construction states instead. The revision layout stores
    /// the shared tail first, then four optional parameter values
    /// (`support_bounds`), the type code as an enum, the nested subtransform
    /// scope, and the trailing integer.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    cache: CacheContract<RevisionSurfaceForm<Vec<bool>, FiniteReal>>,
}

impl TSplineSurfaceConstruction {
    /// Admit finite ordered ranges, finite discontinuities, a valid revision
    /// cache, and a resolved subtransform.
    pub fn try_new(
        parameter_ranges: [[f64; 2]; 2],
        type_code: i64,
        subtransform: TSplineSubtransform,
        trailing_value: i64,
        discontinuities: [Vec<f64>; 6],
        discontinuity_flag: bool,
        cache: CacheContract<RevisionSurfaceForm>,
    ) -> Result<Self, ProceduralGeometryError> {
        let parameter_ranges = [
            crate::topology::ParameterInterval::new(parameter_ranges[0])
                .map_err(ProceduralGeometryError::Payload)?,
            crate::topology::ParameterInterval::new(parameter_ranges[1])
                .map_err(ProceduralGeometryError::Payload)?,
        ];
        let discontinuities = FiniteReal::lanes(discontinuities).ok_or(
            ProceduralGeometryError::Payload("T-spline discontinuities must be finite"),
        )?;
        let cache = cache.admit_form(RevisionSurfaceForm::admit).ok_or(
            ProceduralGeometryError::Payload("T-spline revision cache form is invalid"),
        )?;
        Ok(Self {
            parameter_ranges,
            type_code,
            subtransform,
            trailing_value,
            discontinuities,
            discontinuity_flag,
            cache,
        })
    }

    /// Return ordered U and V intervals.
    pub fn parameter_ranges(&self) -> [crate::topology::ParameterInterval; 2] {
        self.parameter_ranges
    }

    /// Return the native type code value.
    pub const fn type_code(&self) -> i64 {
        self.type_code
    }

    /// Return the native subtransform value.
    pub const fn subtransform(&self) -> &TSplineSubtransform {
        &self.subtransform
    }

    /// Return the native trailing integer.
    pub const fn trailing_value(&self) -> i64 {
        self.trailing_value
    }

    /// Return the native discontinuities value.
    pub const fn discontinuities(&self) -> &[Vec<FiniteReal>; 6] {
        &self.discontinuities
    }

    /// Return the native discontinuity flag value.
    pub const fn discontinuity_flag(&self) -> bool {
        self.discontinuity_flag
    }

    /// Return the native revision form value.
    pub const fn revision_form(&self) -> Option<&RevisionSurfaceForm<Vec<bool>, FiniteReal>> {
        self.cache.form()
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "TSplineSurfaceConstruction"))]
#[serde(deny_unknown_fields)]
struct TSplineSurfaceConstructionWire {
    /// Ordered U and V native parameter intervals.
    parameter_ranges: [[f64; 2]; 2],
    /// Native T-spline type integer.
    type_code: i64,
    /// Inline or referenced shared subtransform object.
    subtransform: TSplineSubtransform,
    /// Native trailing integer.
    trailing_value: i64,
    /// Six ordered solved-surface discontinuity arrays.
    discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    discontinuity_flag: bool,
    /// Cache contract: the revision-gated form, or the legacy solved-cache
    /// tolerance this construction states instead. The revision layout stores
    /// the shared tail first, then four optional parameter values
    /// (`support_bounds`), the type code as an enum, the nested subtransform
    /// scope, and the trailing integer.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    cache: CacheContract<RevisionSurfaceForm>,
}

impl From<TSplineSurfaceConstruction> for TSplineSurfaceConstructionWire {
    fn from(construction: TSplineSurfaceConstruction) -> Self {
        Self {
            parameter_ranges: construction
                .parameter_ranges()
                .map(crate::topology::ParameterInterval::endpoints),
            type_code: construction.type_code,
            subtransform: construction.subtransform,
            trailing_value: construction.trailing_value,
            discontinuities: FiniteReal::raw_lanes(&construction.discontinuities),
            discontinuity_flag: construction.discontinuity_flag,
            cache: construction.cache.view_form(RevisionSurfaceForm::to_raw),
        }
    }
}

impl TryFrom<TSplineSurfaceConstructionWire> for TSplineSurfaceConstruction {
    type Error = ProceduralGeometryError;

    fn try_from(wire: TSplineSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.parameter_ranges,
            wire.type_code,
            wire.subtransform,
            wire.trailing_value,
            wire.discontinuities,
            wire.discontinuity_flag,
            wire.cache,
        )
    }
}

/// One oriented support of a procedural blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BlendSupport {
    /// The support surface.
    pub surface: SurfaceId,
    /// Selects the opposite surface-normal side when true.
    #[serde(default)]
    pub reversed: bool,
}

/// One parameter station of a rolling-ball jet, with its complete value rows.
// A source states raw values; `RollingBallJetStations` holds the admitted
// station, whose scalars, vectors and points are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallJetStation<R = f64, V = Vector3, P = Point3> {
    /// Native spine parameter.
    pub knot: R,
    /// Multiplicity of this parameter in the native knot vector.
    pub multiplicity: u32,
    /// Values and derivatives at this parameter.
    pub site: RollingBallJetSite<R, V, P>,
}

const EPS_ROLLING_BALL_RADIUS: f64 = 1.0e-9;

const ROLLING_BALL_KNOTS: &str = "rolling-ball jet knots must be finite and strictly increasing";

/// Refuse fewer than two stations, and multiplicities outside
/// `1..=degree + 1` or ends that are not clamped at `degree + 1`.
fn admit_rolling_ball_multiplicities<R, V, P>(
    degree: u32,
    stations: &[RollingBallJetStation<R, V, P>],
) -> Result<(), &'static str> {
    let maximum_multiplicity = degree
        .checked_add(1)
        .filter(|_| degree != 0)
        .ok_or("rolling-ball jet degree must be positive with a representable end multiplicity")?;
    if stations.len() < 2 {
        return Err("rolling-ball jet stations must contain at least two rows");
    }
    if stations[0].multiplicity != maximum_multiplicity
        || stations[stations.len() - 1].multiplicity != maximum_multiplicity
        || stations
            .iter()
            .any(|station| station.multiplicity == 0 || station.multiplicity > maximum_multiplicity)
    {
        return Err("rolling-ball jet multiplicities must be in 1..=degree+1 with clamped ends");
    }
    Ok(())
}

/// Refuse a site whose radii are not finite, whose first radius is not
/// positive, or whose two radii disagree beyond the relative tolerance.
fn admit_rolling_ball_radii(
    site: &RollingBallJetSite<FiniteReal, FiniteVector3, FinitePoint3>,
) -> Result<(), &'static str> {
    let first_radius = site.first_limit.distance(site.center.get());
    let second_radius = site.second_limit.distance(site.center.get());
    if !first_radius.is_finite()
        || first_radius <= 0.0
        || !second_radius.is_finite()
        || (first_radius - second_radius).abs()
            > EPS_ROLLING_BALL_RADIUS * first_radius.max(second_radius).max(1.0)
    {
        return Err("rolling-ball jet site radii must be finite and agree within tolerance, with a positive first radius");
    }
    Ok(())
}

/// Degree and finite clamped station data of a rolling-ball jet.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RollingBallJetReadWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallJetStations {
    degree: u32,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<RollingBallJetStation>"))]
    stations: Vec<RollingBallJetStation<FiniteReal, FiniteVector3, FinitePoint3>>,
}

impl RollingBallJetStations {
    /// Admit clamped increasing knots and finite equal-radius station data.
    pub fn try_new(
        degree: u32,
        stations: Vec<RollingBallJetStation>,
    ) -> Result<Self, &'static str> {
        admit_rolling_ball_multiplicities(degree, &stations)?;
        let knots = FiniteReal::lane(stations.iter().map(|station| station.knot).collect())
            .filter(|knots| knots.windows(2).all(|pair| pair[0] < pair[1]))
            .ok_or(ROLLING_BALL_KNOTS)?;
        let stations = stations
            .into_iter()
            .zip(knots)
            .map(|(station, knot)| {
                let site = station.site.admit()?;
                admit_rolling_ball_radii(&site)?;
                Ok::<_, &'static str>(RollingBallJetStation {
                    knot,
                    multiplicity: station.multiplicity,
                    site,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { degree, stations })
    }

    /// Admit stations whose knots, points, angles and derivatives are already
    /// finite. The multiplicity, knot-order and radius refusals are those of
    /// [`Self::try_new`], in its order.
    pub fn from_admitted(
        degree: u32,
        stations: Vec<RollingBallJetStation<FiniteReal, FiniteVector3, FinitePoint3>>,
    ) -> Result<Self, &'static str> {
        admit_rolling_ball_multiplicities(degree, &stations)?;
        if !stations.windows(2).all(|pair| pair[0].knot < pair[1].knot) {
            return Err(ROLLING_BALL_KNOTS);
        }
        for station in &stations {
            admit_rolling_ball_radii(&station.site)?;
        }
        Ok(Self { degree, stations })
    }

    /// Return the polynomial degree of each scalar channel.
    #[must_use]
    pub const fn degree(&self) -> u32 {
        self.degree
    }

    /// Return the ordered station data.
    #[must_use]
    pub fn stations(&self) -> &[RollingBallJetStation<FiniteReal, FiniteVector3, FinitePoint3>] {
        &self.stations
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct RollingBallJetReadWire {
    /// Polynomial degree of each scalar channel.
    degree: u32,
    /// Ordered station data.
    stations: Vec<RollingBallJetStation>,
}

impl TryFrom<RollingBallJetReadWire> for RollingBallJetStations {
    type Error = &'static str;

    fn try_from(wire: RollingBallJetReadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.degree, wire.stations)
    }
}

#[derive(Serialize)]
struct RollingBallJetWriteWire<'a> {
    degree: u32,
    stations: &'a [RollingBallJetStation<FiniteReal, FiniteVector3, FinitePoint3>],
}

impl Serialize for RollingBallJetStations {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RollingBallJetWriteWire {
            degree: self.degree,
            stations: &self.stations,
        }
        .serialize(serializer)
    }
}

/// One aligned knot site of an exact rolling-ball surface jet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallJetSite<R = f64, V = Vector3, P = Point3> {
    /// First limiting point at the knot.
    pub first_limit: P,
    /// Second limiting point at the knot.
    pub second_limit: P,
    /// Rolling-ball center at the knot.
    pub center: P,
    /// Signed opening angle at the knot, in radians.
    pub angle: R,
    /// First parameter derivative of all four value channels.
    pub first_derivative: RollingBallJetDerivative<R, V>,
    /// Second parameter derivative of all four value channels.
    pub second_derivative: RollingBallJetDerivative<R, V>,
}

/// One derivative row for the four channels of a rolling-ball jet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallJetDerivative<R = f64, V = Vector3> {
    /// Derivative of the first limiting point.
    pub first_limit: V,
    /// Derivative of the second limiting point.
    pub second_limit: V,
    /// Derivative of the rolling-ball center.
    pub center: V,
    /// Derivative of the signed opening angle.
    pub angle: R,
}

/// Cross-section family of a procedural blend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BlendCrossSection {
    /// Constant-radius circular cross-section.
    Circular,
    /// Conic (non-circular quadric) cross-section.
    Conic,
    /// Free-form polynomial cross-section.
    Polynomial,
}

/// Shared fields of a revision-gated spline-surface form: the revision
/// integer, optional support bounds and reference-curve endpoints, a
/// carrier-specific boolean run, and the shared tail enum, discontinuity
/// arrays, tail boolean, and post-tail boolean run.
// A source states raw scalars; a surface construction holds the admitted
// form, whose scalars are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "F: Deserialize<'de> + Default, R: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "F: JsonSchema + Default, R: JsonSchema + Serialize")
)]
pub struct RevisionSurfaceForm<F: Default = Vec<bool>, R = f64> {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Optional U/V bound fields following the support surface.
    #[serde(default)]
    pub support_bounds: [Option<R>; 4],
    /// Optional parameter endpoints following the embedded reference curve.
    #[serde(default)]
    pub reference_endpoints: [Option<R>; 2],
    /// Optional parameter endpoints following a second embedded curve, used
    /// by two-curve carriers such as `sum_spl_sur`.
    #[serde(default)]
    pub second_endpoints: [Option<R>; 2],
    /// Carrier-specific boolean run preceding the shared tail.
    #[serde(default)]
    pub flags: F,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm<RevisionSurfaceParameterization<R>>,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<R>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Boolean run following the shared tail.
    #[serde(default)]
    pub trailing_flags: Vec<bool>,
}

/// Mutually exclusive payloads of a revision-gated approximation cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RevisionCacheForm<P = RevisionSurfaceParameterization> {
    /// A solved cache followed by its carrier-specific cache contract.
    SolvedCache {
        /// Carrier-specific solved-cache contract.
        fit_tolerance: FitTolerance,
    },
    /// Parameterization stored in place of a solved cache.
    Parameterization(P),
}

impl<P> RevisionCacheForm<P> {
    /// State the fit tolerance this form carries for its solved cache.
    fn write_fit_tolerance(
        &mut self,
        value: FitTolerance,
        write: ToleranceWrite,
    ) -> RevisionCacheWrite {
        match self {
            Self::SolvedCache { fit_tolerance } => {
                if matches!(write, ToleranceWrite::State) || value.get() > fit_tolerance.get() {
                    *fit_tolerance = value;
                }
                RevisionCacheWrite::Written
            }
            Self::Parameterization(_) => RevisionCacheWrite::Parameterized,
        }
    }

    /// Parameterization carried in place of a solved cache.
    #[must_use]
    pub const fn parameterization(&self) -> Option<&P> {
        match self {
            Self::SolvedCache { .. } => None,
            Self::Parameterization(parameterization) => Some(parameterization),
        }
    }

    /// Fit tolerance carried by a solved cache.
    #[must_use]
    pub const fn fit_tolerance(&self) -> Option<FitTolerance> {
        match self {
            Self::SolvedCache { fit_tolerance } => Some(*fit_tolerance),
            Self::Parameterization(_) => None,
        }
    }
}

/// Approximation state and its dependent fit contract for a variable blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub enum VariableBlendCache<R = f64> {
    /// A nonzero approximation-current flag with an active fit contract.
    Current {
        /// Native approximation-current flag.
        shape_prefix: NonZeroI64,
        /// Fit tolerance in document length units.
        fit_tolerance: FitTolerance,
    },
    /// A zero approximation-current flag without an active fit contract.
    Stale {},
    /// Parameterization in place of a solved cache.
    Parameterization {
        /// Native approximation-current flag, independent of the parameterization.
        shape_prefix: i64,
        /// Surface parameterization.
        parameterization: RevisionSurfaceParameterization<R>,
    },
}

impl<R> VariableBlendCache<R> {
    /// Native approximation-current flag.
    #[must_use]
    pub const fn shape_prefix(&self) -> i64 {
        match self {
            Self::Current { shape_prefix, .. } => shape_prefix.get(),
            Self::Stale {} => 0,
            Self::Parameterization { shape_prefix, .. } => *shape_prefix,
        }
    }

    /// Parameterization carried in place of a solved cache.
    #[must_use]
    pub const fn parameterization(&self) -> Option<&RevisionSurfaceParameterization<R>> {
        match self {
            Self::Parameterization {
                parameterization, ..
            } => Some(parameterization),
            _ => None,
        }
    }

    /// Active fit tolerance, absent for a stale or parameterized approximation.
    #[must_use]
    pub const fn fit_tolerance(&self) -> Option<FitTolerance> {
        match self {
            Self::Current { fit_tolerance, .. } => Some(*fit_tolerance),
            _ => None,
        }
    }
}

/// Parameterization carried by tail-enum form `2` of the shared revision-gated
/// spline-surface tail. This form stores no approximation cache and no fit
/// tolerance; it stores the two parameter intervals followed by four enums, in
/// the order the fields appear below.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct RevisionSurfaceParameterization<R = f64> {
    /// U parameter interval, an ordered `[lo, hi]` pair of optional bounds.
    /// `None` is a false bound-presence flag.
    #[serde(default)]
    pub u_interval: [Option<R>; 2],
    /// V parameter interval, an ordered `[lo, hi]` pair of optional bounds.
    /// `None` is a false bound-presence flag.
    #[serde(default)]
    pub v_interval: [Option<R>; 2],
    /// U closure enum.
    pub u_closure: i64,
    /// V closure enum.
    pub v_closure: i64,
    /// U singularity enum.
    pub u_singularity: i64,
    /// V singularity enum.
    pub v_singularity: i64,
}

impl<R> Default for RevisionSurfaceParameterization<R> {
    /// Absent interval bounds and zero enums.
    fn default() -> Self {
        Self {
            u_interval: [None, None],
            v_interval: [None, None],
            u_closure: 0,
            v_closure: 0,
            u_singularity: 0,
            v_singularity: 0,
        }
    }
}

/// Subtype-specific tail of a native taper spline surface.
// A source states raw values; a `TaperSurfaceConstruction` holds the admitted
// tail, whose scalars and draft vector are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum TaperSurfaceKind<R = f64, V = Vector3> {
    /// Standard taper without a subtype-specific tail.
    Standard {},
    /// Orthogonal taper with a native sense flag.
    Orthogonal {
        /// Native orientation sense.
        sense: bool,
    },
    /// Edge taper with a model-space draft vector.
    Edge {
        /// Native draft vector.
        draft: V,
    },
    /// Shadow taper with a pre-factored draft angle.
    Shadow {
        /// Native draft vector.
        draft: V,
        /// Stored draft-angle sine.
        sine: R,
        /// Stored draft-angle cosine.
        cosine: R,
    },
    /// Ruled taper with a pre-factored angle and factor.
    Ruled {
        /// Native draft vector.
        draft: V,
        /// Stored draft-angle sine.
        sine: R,
        /// Stored draft-angle cosine.
        cosine: R,
        /// Native ruled-taper factor.
        factor: R,
    },
    /// Swept taper with a pre-factored draft angle.
    Swept {
        /// Native draft vector.
        draft: V,
        /// Stored draft-angle sine.
        sine: R,
        /// Stored draft-angle cosine.
        cosine: R,
    },
}

/// One scalar row in native loft subdata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct LoftSubdataRow<R = f64> {
    /// Leading ordered scalar pair.
    pub parameters: [R; 2],
    /// Ordered per-column scalar pairs; empty for subdata type 211.
    pub columns: Vec<[R; 2]>,
    /// Trailing scalar pair stored by the revision-gated row encoding.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extra"
    )]
    pub extra: Option<[R; 2]>,
}

/// Native loft constraint table with structurally consistent dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub enum LoftSubdata<R = f64> {
    /// Type 211 stores exactly one leading pair and no column pairs.
    Type211 {
        /// Native row/column header values. They do not count this form's payload.
        dimensions: [i64; 2],
        /// The sole leading scalar pair.
        row: [R; 2],
    },
    /// All other table types store rows of one shared column width.
    Table(LoftSubdataTable<R>),
}

/// Checked non-211 loft table payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(into = "LoftSubdataTableWire<R>"))]
#[serde(
    try_from = "LoftSubdataTableWire<R>",
    bound(deserialize = "R: Deserialize<'de>")
)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct LoftSubdataTable<R = f64> {
    type_code: i64,
    rows: Vec<LoftSubdataRow<R>>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
struct LoftSubdataTableWire<R = f64> {
    /// Native loft table type code.
    type_code: i64,
    /// Table rows, all of one column width.
    rows: Vec<LoftSubdataRow<R>>,
}

/// The written form of a loft table, borrowing its rows.
#[derive(Serialize)]
struct LoftSubdataTableWriteWire<'a, R> {
    type_code: i64,
    rows: &'a [LoftSubdataRow<R>],
}

impl<R: Serialize> Serialize for LoftSubdataTable<R> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LoftSubdataTableWriteWire {
            type_code: self.type_code,
            rows: &self.rows,
        }
        .serialize(serializer)
    }
}

/// The rows of a loft table do not share one column width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("loft subdata rows do not share one column width")]
pub struct RaggedLoftTable;

impl<R> LoftSubdataTable<R> {
    /// Admit a table whose rows share one column width.
    pub fn new(type_code: i64, rows: Vec<LoftSubdataRow<R>>) -> Result<Self, RaggedLoftTable> {
        if rows.len() > i64::MAX as usize
            || rows
                .first()
                .is_some_and(|row| row.columns.len() > i64::MAX as usize)
        {
            return Err(RaggedLoftTable);
        }
        let column_count = rows.first().map_or(0, |row| row.columns.len());
        if rows.iter().any(|row| row.columns.len() != column_count) {
            return Err(RaggedLoftTable);
        }
        Ok(Self { type_code, rows })
    }
}

impl<R> TryFrom<LoftSubdataTableWire<R>> for LoftSubdataTable<R> {
    type Error = RaggedLoftTable;

    fn try_from(wire: LoftSubdataTableWire<R>) -> Result<Self, Self::Error> {
        Self::new(wire.type_code, wire.rows)
    }
}

impl LoftSubdata {
    /// Construct the fixed type-211 form.
    #[must_use]
    pub fn type_211(dimensions: [i64; 2], row: [f64; 2]) -> Self {
        Self::Type211 { dimensions, row }
    }

    /// Construct a non-211 table whose rows have one shared column width.
    #[must_use]
    pub fn table(type_code: i64, rows: Vec<LoftSubdataRow>) -> Option<Self> {
        LoftSubdataTable::new(type_code, rows).ok().map(Self::Table)
    }
}

impl<R> LoftSubdata<R> {
    /// Native table type discriminator.
    #[must_use]
    pub fn type_code(&self) -> i64 {
        match self {
            Self::Type211 { .. } => 211,
            Self::Table(table) => table.type_code,
        }
    }

    /// Native row header: a stored value for type 211, otherwise the row count.
    #[must_use]
    pub fn row_count(&self) -> i64 {
        match self {
            Self::Type211 { dimensions, .. } => dimensions[0],
            Self::Table(table) => table.rows.len() as i64,
        }
    }

    /// Native column header: stored for type 211, otherwise the shared row width.
    #[must_use]
    pub fn column_count(&self) -> i64 {
        match self {
            Self::Type211 { dimensions, .. } => dimensions[1],
            Self::Table(table) => table.rows.first().map_or(0, |row| row.columns.len() as i64),
        }
    }

    /// Visit the leading and column pairs in each row.
    pub fn visit_rows(&self, mut visit: impl FnMut(&[R; 2], &[[R; 2]], Option<&[R; 2]>)) {
        match self {
            Self::Type211 { row, .. } => visit(row, &[], None),
            Self::Table(table) => {
                for row in &table.rows {
                    visit(&row.parameters, &row.columns, row.extra.as_ref());
                }
            }
        }
    }
}

/// The complete constraint payload of a classic loft or skin profile.
///
/// Every field the classic form carries is required and every field it cannot
/// carry is absent from the type, so the revision-gated `support_bounds` and
/// the type-zero `secondary_pcurve` are unknown keys here rather than values a
/// reader has to refuse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "LoftProfileData"))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct ClassicLoftProfileData<R = f64, V = Vector3> {
    /// Required support surface.
    pub surface: SurfaceId,
    /// Nullable parameter curve on the support.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pcurve"
    )]
    pub pcurve: Option<PcurveGeometry>,
    /// First native constraint flag.
    pub first_flag: bool,
    /// ASM extension integer preceding the subdata.
    pub asm_extension: i64,
    /// Native constraint table.
    pub subdata: LoftSubdata<R>,
    /// Optional direction selected by the second native flag.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<V>,
}

/// Type-selected fields of one loft profile member.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub enum LoftMemberForm<R = f64, V = Vector3> {
    /// Support-surface form. Legacy layouts can use type zero; revision-gated
    /// layouts select this form with a nonzero type code.
    Support {
        /// Native member type discriminator.
        type_code: i64,
        /// Constraint support surface, absent for the native `null_surface`
        /// sentinel.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_surface"
        )]
        surface: Option<SurfaceId>,
        /// Optional U/V bound fields following the support surface in the
        /// revision-gated encoding.
        #[serde(default)]
        support_bounds: [Option<R>; 4],
        /// UV curve on the support, absent for `nullbs`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pcurve"
        )]
        pcurve: Option<PcurveGeometry>,
        /// First native constraint flag.
        first_flag: bool,
        /// ASM extension integer when the stream version carries it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_asm_extension"
        )]
        asm_extension: Option<i64>,
        /// Native constraint table.
        subdata: LoftSubdata<R>,
        /// Optional direction selected by the second native flag.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<V>,
    },
    /// Revision-gated type-zero form with two nullable UV curve slots.
    PcurvePair {
        /// First UV curve slot, absent for `nullbs`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pcurve"
        )]
        pcurve: Option<PcurveGeometry>,
        /// Second UV curve slot, absent for `nullbs`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_secondary_pcurve"
        )]
        secondary_pcurve: Option<PcurveGeometry>,
        /// ASM extension integer when the stream version carries it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_asm_extension"
        )]
        asm_extension: Option<i64>,
        /// Native constraint table.
        subdata: LoftSubdata<R>,
        /// Optional direction selected by the second native flag.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_direction"
        )]
        direction: Option<V>,
    },
}

impl<R, V> LoftMemberForm<R, V> {
    /// Return the native type code selected by this form.
    #[must_use]
    pub fn type_code(&self) -> i64 {
        match self {
            Self::Support { type_code, .. } => *type_code,
            Self::PcurvePair { .. } => 0,
        }
    }

    /// Return the support surface when this is the support form.
    #[must_use]
    pub fn surface(&self) -> Option<&SurfaceId> {
        match self {
            Self::Support { surface, .. } => surface.as_ref(),
            Self::PcurvePair { .. } => None,
        }
    }

    /// Return the constraint subdata.
    #[must_use]
    pub fn subdata(&self) -> &LoftSubdata<R> {
        match self {
            Self::Support { subdata, .. } | Self::PcurvePair { subdata, .. } => subdata,
        }
    }

    /// Return the optional direction selected by the second native flag.
    #[must_use]
    pub fn direction(&self) -> Option<&V> {
        match self {
            Self::Support { direction, .. } | Self::PcurvePair { direction, .. } => {
                direction.as_ref()
            }
        }
    }
}

/// One referenced curve together with its optional native parameter bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct LoftPathCurve<R = f64> {
    /// Referenced curve carrier.
    #[serde(rename = "curve")]
    pub id: CurveId,
    /// Optional parameter endpoints following the curve in a revision-gated encoding.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_endpoints"
    )]
    pub endpoints: Option<[Option<R>; 2]>,
}

/// One curve member of a loft profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct LoftProfileMember<R = f64, V = Vector3> {
    /// Profile curve and its revision-gated parameter endpoints.
    pub profile: LoftPathCurve<R>,
    /// Structurally selected surface-side constraint form.
    pub form: LoftMemberForm<R, V>,
}

/// Native path data attached to one loft section entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct LoftPath<R = f64> {
    /// Primary path curve and its optional endpoints, absent for `null_curve`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_path"
    )]
    pub path: Option<LoftPathCurve<R>>,
    /// Ordered auxiliary BS3 curves.
    pub auxiliaries: Vec<CurveId>,
    /// Native path tail integer.
    pub flag: i64,
}

/// One parameterized entry in a native loft section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct LoftSectionEntry<R = f64, V = Vector3> {
    /// Native section parameter.
    pub parameter: R,
    /// Ordered profile members.
    pub profile: Vec<LoftProfileMember<R, V>>,
    /// Native path data.
    pub path: LoftPath<R>,
}

/// Revision-gated `loft_spl_sur` form fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct LoftRevisionForm<R = f64> {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Four booleans following the parameter intervals.
    #[serde(default)]
    pub flags: [bool; 4],
    /// Two integers preceding the shared tail.
    #[serde(default)]
    pub ints: [i64; 2],
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm<RevisionSurfaceParameterization<R>>,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<R>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
}

/// Ordered native loft section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct LoftSection<R = f64, V = Vector3> {
    /// Ordered entries in the section.
    pub entries: Vec<LoftSectionEntry<R, V>>,
}

/// Token retained from the variable bridge preceding a loft solved cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum LoftBridgeToken<R = f64> {
    /// Native boolean token.
    Boolean(bool),
    /// Native integer token.
    Integer(i64),
    /// Native double token.
    Double(R),
    /// Native string token.
    Text(String),
    /// Native enum token.
    Enum(i64),
}

/// Common carrier fields of one G2 blend side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct G2BlendSide<V = Vector3> {
    /// Native side label.
    pub label: String,
    /// Primary support surface.
    pub surface: SurfaceId,
    /// Primary side curve.
    pub curve: CurveId,
    /// First and second ordered BS2 pcurves; each may be `nullbs`.
    pub pcurves: [Option<PcurveGeometry>; 2],
    /// Native side direction.
    pub direction: V,
}

/// Singularity-specific payload of the first G2 blend side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub enum G2BlendFirstShape<R = f64> {
    /// Full singularity with an optional BS3 support surface.
    Full {
        /// Exact BS3 support and fit tolerance, when serialized.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_support"
        )]
        support: Option<G2BlendFullSupport>,
    },
    /// Non-singular nine-scalar frame and tertiary pcurve.
    None {
        /// Ordered native frame scalars.
        coefficients: [R; 9],
        /// Native fit tolerance.
        tolerance: FitTolerance,
        /// Optional intervening native token.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_extension"
        )]
        extension: Option<LoftBridgeToken<R>>,
        /// Tertiary BS2 pcurve, absent for `nullbs`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pcurve"
        )]
        pcurve: Option<PcurveGeometry>,
    },
}

/// Exact support surface and fit tolerance of a full G2 first-side shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct G2BlendFullSupport {
    /// Exact BS3 support surface.
    pub surface: SurfaceId,
    /// Fit tolerance of the support, in document length units.
    pub tolerance: FitTolerance,
}

/// Full native G2 blend construction graph.
// A source states raw values; a `G2BlendSurfacePayload` holds the admitted
// construction, whose scalars and vectors are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct G2BlendConstruction<R = f64, V = Vector3> {
    /// First side common fields.
    pub first: G2BlendSide<V>,
    /// Native first-side singularity enum.
    pub singularity: i64,
    /// First-side singularity payload.
    pub first_shape: G2BlendFirstShape<R>,
    /// Second side common fields.
    pub second: G2BlendSide<V>,
    /// Exact second-side spline support.
    pub second_exact_surface: SurfaceId,
    /// Center or transition curve.
    pub center_curve: CurveId,
    /// Ordered center-curve scalars.
    pub center_parameters: [R; 2],
    /// Native center tail integer.
    pub center_flag: i64,
    /// Native U and V intervals.
    pub parameter_ranges: [[R; 2]; 2],
    /// Four ordered trailing scalars.
    pub trailing_parameters: [R; 4],
    /// Three ordered ASM discontinuity arrays.
    pub discontinuities: [Vec<R>; 3],
}

/// A present rolling-ball support surface and its native UV bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallSupportSurface<S = SurfaceId, R = f64> {
    /// Support surface or embedded geometry.
    pub surface: S,
    /// Optional native U and V endpoints.
    pub parameter_ranges: [[Option<R>; 2]; 2],
}

/// A present rolling-ball side curve and its native parameter bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallSupportCurve<C = CurveId, R = f64> {
    /// Side curve or embedded geometry.
    pub curve: C,
    /// Optional native parameter endpoints.
    pub parameter_range: [Option<R>; 2],
}

/// The optional rolling-ball extension clause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(bound(deserialize = "P: Deserialize<'de>"))]
#[serde(deny_unknown_fields)]
pub struct RollingBallSideExtension<P = PcurveGeometry> {
    /// Native integer introducing the clause.
    pub value: i64,
    /// Tertiary BS2 pcurve, absent for `nullbs`.
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    pub pcurve: Option<P>,
}

/// One complete native rolling-ball support side.
// A source states raw values; a blend construction holds the admitted side,
// whose endpoints are `FiniteReal` values and whose location is a
// `FinitePoint3`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(bound(
    serialize = "S: Serialize, C: Serialize, P: Serialize, R: Serialize, L: Serialize",
    deserialize = "S: Deserialize<'de>, C: Deserialize<'de>, P: Deserialize<'de>, R: Deserialize<'de>, L: Deserialize<'de>"
))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "S: JsonSchema + Serialize, C: JsonSchema + Serialize, P: JsonSchema + Serialize, R: JsonSchema + Serialize, L: JsonSchema + Serialize"
    )
)]
pub struct RollingBallSide<S = SurfaceId, C = CurveId, P = PcurveGeometry, R = f64, L = Point3> {
    /// Geometry role selected by the support-side discriminator.
    pub support_kind: VariableBlendSupportKind,
    /// Primary support surface and bounds, absent for `null_surface`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rolling_ball_side_surface"
    )]
    pub surface: Option<RollingBallSupportSurface<S, R>>,
    /// Side curve and bounds, absent for `null_curve`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curve"
    )]
    pub curve: Option<RollingBallSupportCurve<C, R>>,
    /// Primary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rolling_ball_side_pcurve"
    )]
    pub pcurve: Option<P>,
    /// Native model-space side location.
    pub location: L,
    /// ASM secondary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rolling_ball_side_secondary_pcurve"
    )]
    pub secondary_pcurve: Option<P>,
    /// Native extension integer and nullable tertiary pcurve.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rolling_ball_side_extension"
    )]
    pub extension: Option<RollingBallSideExtension<P>>,
}

/// Third support graph appended by `sss_blend_spl_sur`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallThirdSide<V = Vector3> {
    /// Native side label.
    pub label: String,
    /// Third support surface.
    pub surface: SurfaceId,
    /// Third side curve.
    pub curve: CurveId,
    /// Primary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pcurve"
    )]
    pub pcurve: Option<PcurveGeometry>,
    /// Native side vector.
    pub direction: V,
    /// ASM secondary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_secondary_pcurve"
    )]
    pub secondary_pcurve: Option<PcurveGeometry>,
    /// Native ASM integer following the secondary pcurve.
    pub extension: i64,
    /// ASM tertiary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tertiary_pcurve"
    )]
    pub tertiary_pcurve: Option<PcurveGeometry>,
    /// Final ASM flag.
    pub flag: bool,
}

/// Native optional-radius selector in a rolling-ball construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RollingBallRadiusSelector<T = f64> {
    /// The construction states no radius.
    None {},
    /// Explicit native selector scalar.
    Value {
        /// Stored scalar value.
        value: T,
    },
}

/// Complete byte-backed rolling-ball or three-surface blend context.
// A source states raw values; a `BlendSurfacePayload` holds the admitted
// construction, whose scalars, vectors and points are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct RollingBallConstruction<R = f64, V = Vector3, P = Point3> {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Two ordered primary support sides.
    pub sides: [RollingBallSide<SurfaceId, CurveId, PcurveGeometry, R, P>; 2],
    /// Stored slice or center curve.
    pub slice: CurveId,
    /// Optional native slice-curve parameter endpoints.
    #[serde(default)]
    pub slice_range: [Option<R>; 2],
    /// Two signed support offsets in document length units.
    pub offsets: [R; 2],
    /// Optional-radius selector field.
    pub radius_selector: RollingBallRadiusSelector<R>,
    /// Native optional U interval endpoints.
    pub u_range: [Option<R>; 2],
    /// Native optional V interval endpoints.
    pub v_range: [Option<R>; 2],
    /// Native integer preceding the trailing scalars.
    pub shape_prefix: i64,
    /// Two ordered trailing scalars.
    pub parameters: [R; 2],
    /// Native long following the trailing scalars.
    pub tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm<RevisionSurfaceParameterization<R>>,
    /// Six ordered ASM discontinuity arrays closing the shared tail.
    pub discontinuities: [Vec<R>; 6],
    /// Native Boolean closing the shared tail.
    #[serde(default)]
    pub tail_flag: bool,
    /// Third side present only for `sss_blend_spl_sur`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_third"
    )]
    pub third: Option<Box<RollingBallThirdSide<V>>>,
    /// Three ASM integers preceding the subtype close.
    #[serde(default)]
    pub tail_extensions: [i64; 3],
}

/// Geometry role selected by a variable-blend support-side discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendSupportKind {
    /// Support defined by a cosine curve.
    CosineCurve,
    /// Support defined by a general curve.
    Curve,
    /// Support defined by a point curve.
    PointCurve,
    /// Support defined by a surface.
    Surface,
    /// Support defined by a zero curve.
    ZeroCurve,
}

/// Convexity selected for a variable-radius blend surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendConvexity {
    /// The blend bends toward the support intersection.
    Convex,
    /// The blend bends away from the support intersection.
    Concave,
}

/// Solved-surface representation selected for a variable-radius blend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendRenderMode {
    /// The solved surface is the rolling-ball envelope.
    RollingBallEnvelope,
    /// The solved surface is a rolling-ball snapshot.
    RollingBallSnapshot,
}

/// One interpolation control point in a variable blend-value law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VariableBlendInterpolationPoint<R = f64, V = Vector3, P = Point3> {
    /// Law parameter.
    pub parameter: R,
    /// Radius in document length units.
    pub radius: R,
    /// Optional first and second derivative scalars.
    pub tangents: [Option<R>; 2],
    /// Model-space control location.
    pub location: P,
    /// Control normal.
    pub normal: V,
}

/// Native edge-offset blend-value sub-discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "i64", into = "i64")]
pub enum EdgeOffsetDiscriminator {
    /// Explicit zero sub-discriminator.
    Zero,
    /// One sub-discriminator, including the elided default.
    One,
}

impl EdgeOffsetDiscriminator {
    /// Admit the two native edge-offset codes.
    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Zero),
            1 => Some(Self::One),
            _ => None,
        }
    }

    /// Native integer code.
    pub const fn code(self) -> i64 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
        }
    }
}

/// An edge-offset blend value carries sub-discriminator 0 or 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("edge-offset discriminator must be 0 or 1")]
pub struct EdgeOffsetDiscriminatorCode;

impl TryFrom<i64> for EdgeOffsetDiscriminator {
    type Error = EdgeOffsetDiscriminatorCode;

    fn try_from(code: i64) -> Result<Self, Self::Error> {
        Self::from_code(code).ok_or(EdgeOffsetDiscriminatorCode)
    }
}

impl From<EdgeOffsetDiscriminator> for i64 {
    fn from(discriminator: EdgeOffsetDiscriminator) -> Self {
        discriminator.code()
    }
}

/// Numeric or symbolic terminal of a functional blend-value law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendTerminal<R = f64> {
    /// Native double token.
    Double(R),
    /// Native string token.
    Text(String),
}

/// Complete recursive native `getBlendValues` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VariableBlendValue<R = f64, V = Vector3, P = Point3> {
    /// Native Boolean following the calibrated enum.
    pub modern_flag: bool,
    /// Native calibrated enum.
    pub calibrated: i64,
    /// Type-specific payload with its native sub-discriminator.
    pub payload: VariableBlendValuePayload<R, V, P>,
}

/// Type-specific payload of a variable blend value.
///
/// Each arm carries the native sub-discriminator it admits: the edge-offset
/// arm is one of two codes, so an edge-offset value with any other
/// sub-discriminator has no spelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableBlendValuePayload<R = f64, V = Vector3, P = Point3> {
    /// Law-domain parameter range and two endpoint radii.
    TwoEnds {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Law-domain parameter range (lower, upper).
        parameters: [R; 2],
        /// Endpoint radii in document length units.
        radii: [R; 2],
    },
    /// Fixed-width branch: the parameter-range bounds and the chamfer width
    /// scalar, stored unscaled.
    FixedWidth {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Parameter-range lower and upper bounds.
        parameters: [R; 2],
        /// Chamfer width.
        width: R,
    },
    /// Edge-offset branch.
    EdgeOffset {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: EdgeOffsetDiscriminator,
        /// Ordered native scalar payload.
        scalars: [R; 2],
        /// Ordered length payload in document units.
        lengths: [R; 1],
    },
    /// Functional radius law carried by a BS2 pcurve.
    Functional {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Leading scalar.
        parameter: R,
        /// Leading length in document units.
        radius: R,
        /// Scalar function whose first coordinate is radius in document units.
        function: PcurveGeometry,
        /// Numeric or symbolic terminal value.
        terminal: VariableBlendTerminal<R>,
    },
    /// Constant law followed by a recursive chamfer value.
    Constant {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Ordered native scalars.
        parameters: [R; 2],
        /// Radius in document length units.
        radius: R,
        /// Native variable-chamfer enum.
        variable_chamfer: i64,
        /// Native chamfer-type enum.
        chamfer_type: i64,
        /// Recursively nested blend value.
        nested: Box<VariableBlendValue<R, V, P>>,
    },
    /// Interpolated radius law.
    Interpolated {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Leading scalar.
        parameter: R,
        /// Leading radius in document length units.
        radius: R,
        /// Scalar function whose first coordinate is radius in document units.
        function: PcurveGeometry,
        /// Native extension enum, stored ahead of the radius-point count. It
        /// gates nothing; the payload ends at the last radius point.
        enum_count: i64,
        /// Whether the extension enum is stored as a `0x15` enum token
        /// (revision-gated streams) rather than a `0x04` integer.
        #[serde(default)]
        enum_tagged: bool,
        /// Counted radius-point array: each control carries a parameter,
        /// radius, two derivative scalars, a position, and a vector.
        points: Vec<VariableBlendInterpolationPoint<R, V, P>>,
    },
}

impl<R, V, P> VariableBlendValuePayload<R, V, P> {
    /// Native sub-discriminator preceding the calibrated enum.
    pub const fn discriminator(&self) -> i64 {
        match self {
            Self::EdgeOffset { discriminator, .. } => discriminator.code(),
            Self::TwoEnds { discriminator, .. }
            | Self::FixedWidth { discriminator, .. }
            | Self::Functional { discriminator, .. }
            | Self::Constant { discriminator, .. }
            | Self::Interpolated { discriminator, .. } => *discriminator,
        }
    }

    /// Native type name introducing this payload.
    pub const fn native_name(&self) -> &'static str {
        match self {
            Self::TwoEnds { .. } => "two_ends",
            Self::FixedWidth { .. } => "fixed_width",
            Self::EdgeOffset { .. } => "edge_offset",
            Self::Functional { .. } => "functional",
            Self::Constant { .. } => "const",
            Self::Interpolated { .. } => "interp",
        }
    }
}

/// Radius-law payloads of a variable blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableBlendRadii<R = f64, V = Vector3, P = Point3> {
    /// One radius law controls both support sides.
    Single {
        /// Shared radius law.
        value: VariableBlendValue<R, V, P>,
    },
    /// Each support side has an independent radius law.
    Two {
        /// First support-side radius law.
        first: VariableBlendValue<R, V, P>,
        /// Second support-side radius law.
        second: VariableBlendValue<R, V, P>,
    },
}

impl<R, V, P> VariableBlendRadii<R, V, P> {
    /// First radius law in native order.
    #[must_use]
    pub const fn first(&self) -> &VariableBlendValue<R, V, P> {
        match self {
            Self::Single { value } | Self::Two { first: value, .. } => value,
        }
    }

    /// Second radius law when both sides are controlled independently.
    #[must_use]
    pub const fn second(&self) -> Option<&VariableBlendValue<R, V, P>> {
        match self {
            Self::Single { .. } => None,
            Self::Two { second, .. } => Some(second),
        }
    }

    /// Whether one radius law controls both sides.
    #[must_use]
    pub const fn is_single(&self) -> bool {
        matches!(self, Self::Single { .. })
    }
}

/// Cross-section clause following the variable-radius laws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub enum VariableBlendCrossSection<R = f64, V = Vector3, P = Point3> {
    /// Circular section with no additional parameters.
    Circular {},
    /// Thumbweight-controlled section with two ordered shape parameters.
    Thumbweights {
        /// Ordered native shape parameters.
        parameters: [R; 2],
    },
    /// Rounded chamfer with an optional independent rounding-radius law.
    RoundedChamfer {
        /// Rounding-radius law; absent when the clause stores `no_radius`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radius"
        )]
        radius: Option<Box<VariableBlendValue<R, V, P>>>,
    },
    /// Curvature-continuous round with two ordered shape parameters.
    G2Round {
        /// Ordered native shape parameters.
        parameters: [R; 2],
    },
    /// A zero-width native selector whose record framing is known but whose
    /// geometric cross-section law is not classified.
    UnclassifiedBare {
        /// Exact numeric selector retained from the native record.
        selector: VariableBlendBareCrossSection,
    },
}

/// Native zero-width variable-blend cross-section selectors whose framing is
/// established while their geometric laws remain unclassified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[repr(i64)]
#[serde(deny_unknown_fields)]
pub enum VariableBlendBareCrossSection {
    /// Native selector `2`.
    Selector2 = 2,
    /// Native selector `4`.
    Selector4 = 4,
    /// Native selector `5`.
    Selector5 = 5,
    /// Native selector `6`.
    Selector6 = 6,
}

impl VariableBlendBareCrossSection {
    /// Numeric selector stored in the native variable-blend record.
    pub const fn native_selector(self) -> i64 {
        self as i64
    }
}

impl TryFrom<i64> for VariableBlendBareCrossSection {
    type Error = ();

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            2 => Ok(Self::Selector2),
            4 => Ok(Self::Selector4),
            5 => Ok(Self::Selector5),
            6 => Ok(Self::Selector6),
            _ => Err(()),
        }
    }
}

/// Native variable-radius blend surface subtype.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendSurfaceSubtype {
    /// General variable-blend surface.
    #[default]
    VariableBlend,
    /// Surface-to-surface variable blend.
    SurfaceSurface,
    /// Curve-to-curve variable blend.
    CurveCurve,
    /// Curve-to-surface variable blend.
    CurveSurface,
    /// Free surface-curve variable blend.
    SurfaceCurveFree,
}

/// Complete native variable-radius blend construction graph.
// A source states raw values; a `VariableBlendSurfacePayload` holds the
// admitted construction, whose scalars, vectors and points are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct VariableBlendConstruction<R = f64, V = Vector3, P = Point3> {
    /// Native surface subtype selecting the variable-blend behavior class.
    #[serde(default)]
    pub subtype: VariableBlendSurfaceSubtype,
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Two ordered support-side graphs in the rolling-ball side layout.
    pub sides: [RollingBallSide<SurfaceId, CurveId, PcurveGeometry, R, P>; 2],
    /// Stored slice curve.
    pub slice: CurveId,
    /// Optional native slice-curve parameter endpoints.
    #[serde(default)]
    pub slice_range: [Option<R>; 2],
    /// Two signed support offsets in document length units.
    pub offsets: [R; 2],
    /// Structurally selected radius-control payloads.
    pub radii: VariableBlendRadii<R, V, P>,
    /// Cross-section clause following the complete radius-law sequence.
    /// Absence denotes an elided default circular section; an explicit
    /// circular clause remains distinct.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cross_section"
    )]
    pub cross_section: Option<VariableBlendCrossSection<R, V, P>>,
    /// Support-side parameter interval `(T0, T1)`; both bounds present in
    /// every instance.
    pub u_range: [R; 2],
    /// Second interval: a lower bound with an unbounded-above marker,
    /// encoded as `(T lo, F)` and decoding to `[Some(lo), None]`. The `F`
    /// upper-bound marker is an interval bound, not a standalone Boolean.
    #[serde(
        rename = "v_lower",
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_v_lower"
    )]
    pub v_lower: Option<R>,
    /// Requested fit tolerance for the surface cache.
    pub shape_parameter: R,
    /// Achieved fit tolerance for the surface cache, in document units.
    pub shape_length: R,
    /// Native integer immediately before the shared tail's enum.
    pub shape_tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: VariableBlendCache<R>,
    /// Six ordered ASM discontinuity arrays closing the shared tail.
    pub discontinuities: [Vec<R>; 6],
    /// Native Boolean following the discontinuity arrays.
    pub tail_flag: bool,
    /// Three ASM integers following the tail Boolean.
    pub tail_extensions: [i64; 3],
    /// Secondary curve and its bounds, absent for `null_curve`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_secondary_curve"
    )]
    pub secondary_curve: Option<RollingBallSupportCurve<CurveId, R>>,
    /// Blend convexity.
    pub convexity: VariableBlendConvexity,
    /// Solved-surface representation.
    pub render_mode: VariableBlendRenderMode,
    /// Native optional post-shape interval endpoints.
    pub post_range: [Option<R>; 2],
    /// Native post-shape BS3 curve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_post_curve"
    )]
    pub post_curve: Option<CurveId>,
    /// Native post-shape BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_post_pcurve"
    )]
    pub post_pcurve: Option<PcurveGeometry>,
}

/// Complete native revision-gated `g2_blend_spl_sur` construction. The
/// revision layout stores the two support sides in the variable-blend side
/// layout and ends with the shared revision-gated surface tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "RevisionG2BlendConstructionWire")]
pub struct RevisionG2BlendConstruction {
    revision: PositiveI64,
    leading_parameters: [FiniteReal; 2],
    #[cfg_attr(feature = "schema", schemars(with = "Box<[RollingBallSide; 2]>"))]
    sides: [RollingBallSide<SurfaceId, CurveId, PcurveGeometry, FiniteReal, FinitePoint3>; 2],
    center: CurveId,
    #[serde(default)]
    center_range: [Option<FiniteReal>; 2],
    radii: [FiniteReal; 2],
    radius_selector: RollingBallRadiusSelector<PositiveI64>,
    u_range: [Option<FiniteReal>; 2],
    v_range: [Option<FiniteReal>; 2],
    shape_prefix: i64,
    shape_parameter: FiniteReal,
    shape_length: FiniteReal,
    shape_tail: i64,
    #[cfg_attr(feature = "schema", schemars(with = "RevisionCacheForm"))]
    cache: RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>>,
    #[serde(default)]
    discontinuities: [Vec<FiniteReal>; 6],
    tail_flag: bool,
    tail_extensions: [i64; 3],
}

/// Stored fields of a revision-gated `g2_blend_spl_sur` construction before
/// admission. This is the wire shape the deserializer reads and the only
/// input `RevisionG2BlendConstruction::admit` accepts.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "RevisionG2BlendConstruction"))]
#[serde(deny_unknown_fields)]
pub struct RevisionG2BlendConstructionWire {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Two native scalars following the revision integer.
    pub leading_parameters: [f64; 2],
    /// Two ordered support-side graphs in the variable-blend side layout.
    pub sides: Box<[RollingBallSide; 2]>,
    /// Stored center curve.
    pub center: CurveId,
    /// Optional native center-curve parameter endpoints.
    #[serde(default)]
    pub center_range: [Option<f64>; 2],
    /// Two signed blend radii in document length units.
    pub radii: [f64; 2],
    /// Integer-valued optional-radius selector following the radii.
    pub radius_selector: RollingBallRadiusSelector<PositiveI64>,
    /// Native optional U interval endpoints.
    pub u_range: [Option<f64>; 2],
    /// Native optional V interval endpoints.
    pub v_range: [Option<f64>; 2],
    /// Native integer before the solved shape.
    pub shape_prefix: i64,
    /// Native scalar before the solved shape.
    pub shape_parameter: f64,
    /// Native length before the solved shape, in document units.
    pub shape_length: f64,
    /// Native integer immediately before the shared tail.
    pub shape_tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Three ASM integers following the shared tail.
    pub tail_extensions: [i64; 3],
}

impl RevisionG2BlendConstruction {
    /// Admit stored fields whose scalars are all finite. This is the
    /// admission of the `revision_g2_blend` procedural surface: a decoder
    /// that reads the fields admits them here, and the deserializer runs the
    /// same walk. The fields are private, so this and the deserializer are
    /// the only routes to a value.
    pub fn admit(wire: RevisionG2BlendConstructionWire) -> Result<Self, ProceduralGeometryError> {
        let invalid = || {
            ProceduralGeometryError::Payload("revision g2 blend construction payload is invalid")
        };
        let [first, second] = *wire.sides;
        let (
            Some(leading_parameters),
            Some(center_range),
            Some(radii),
            Some(u_range),
            Some(v_range),
            Some([shape_parameter, shape_length]),
            Some(discontinuities),
            Some(first),
            Some(second),
            Some(cache),
        ) = (
            FiniteReal::array(wire.leading_parameters),
            FiniteReal::optional(wire.center_range),
            FiniteReal::array(wire.radii),
            FiniteReal::optional(wire.u_range),
            FiniteReal::optional(wire.v_range),
            FiniteReal::array([wire.shape_parameter, wire.shape_length]),
            FiniteReal::lanes(wire.discontinuities),
            first.admit(),
            second.admit(),
            wire.cache.admit(),
        )
        else {
            return Err(invalid());
        };
        Ok(Self {
            revision: wire.revision,
            leading_parameters,
            sides: [first, second],
            center: wire.center,
            center_range,
            radii,
            radius_selector: wire.radius_selector,
            u_range,
            v_range,
            shape_prefix: wire.shape_prefix,
            shape_parameter,
            shape_length,
            shape_tail: wire.shape_tail,
            cache,
            discontinuities,
            tail_flag: wire.tail_flag,
            tail_extensions: wire.tail_extensions,
        })
    }

    /// Return the positive serializer-revision integer.
    #[must_use]
    pub const fn revision(&self) -> PositiveI64 {
        self.revision
    }

    /// Return the two native scalars following the revision integer.
    #[must_use]
    pub const fn leading_parameters(&self) -> [FiniteReal; 2] {
        self.leading_parameters
    }

    /// Return the two ordered support-side graphs.
    #[must_use]
    pub const fn sides(
        &self,
    ) -> &[RollingBallSide<SurfaceId, CurveId, PcurveGeometry, FiniteReal, FinitePoint3>; 2] {
        &self.sides
    }

    /// Return the stored center curve.
    #[must_use]
    pub const fn center(&self) -> &CurveId {
        &self.center
    }

    /// Return the optional center-curve parameter endpoints.
    #[must_use]
    pub const fn center_range(&self) -> [Option<FiniteReal>; 2] {
        self.center_range
    }

    /// Return the two signed blend radii.
    #[must_use]
    pub const fn radii(&self) -> [FiniteReal; 2] {
        self.radii
    }

    /// Return the optional-radius selector.
    #[must_use]
    pub const fn radius_selector(&self) -> &RollingBallRadiusSelector<PositiveI64> {
        &self.radius_selector
    }

    /// Return the optional U interval endpoints.
    #[must_use]
    pub const fn u_range(&self) -> [Option<FiniteReal>; 2] {
        self.u_range
    }

    /// Return the optional V interval endpoints.
    #[must_use]
    pub const fn v_range(&self) -> [Option<FiniteReal>; 2] {
        self.v_range
    }

    /// Return the integer before the solved shape.
    #[must_use]
    pub const fn shape_prefix(&self) -> i64 {
        self.shape_prefix
    }

    /// Return the scalar before the solved shape.
    #[must_use]
    pub const fn shape_parameter(&self) -> FiniteReal {
        self.shape_parameter
    }

    /// Return the length before the solved shape.
    #[must_use]
    pub const fn shape_length(&self) -> FiniteReal {
        self.shape_length
    }

    /// Return the integer immediately before the shared tail.
    #[must_use]
    pub const fn shape_tail(&self) -> i64 {
        self.shape_tail
    }

    /// Return the approximation-cache form.
    #[must_use]
    pub const fn cache(&self) -> &RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>> {
        &self.cache
    }

    /// Return the six ordered discontinuity arrays.
    #[must_use]
    pub const fn discontinuities(&self) -> &[Vec<FiniteReal>; 6] {
        &self.discontinuities
    }

    /// Return the Boolean terminating the shared tail.
    #[must_use]
    pub const fn tail_flag(&self) -> bool {
        self.tail_flag
    }

    /// Return the three ASM integers following the shared tail.
    #[must_use]
    pub const fn tail_extensions(&self) -> [i64; 3] {
        self.tail_extensions
    }
}

impl TryFrom<RevisionG2BlendConstructionWire> for RevisionG2BlendConstruction {
    type Error = ProceduralGeometryError;

    fn try_from(wire: RevisionG2BlendConstructionWire) -> Result<Self, Self::Error> {
        Self::admit(wire)
    }
}

/// Complete native revision-gated `cl_loft_spl_sur` construction. The
/// revision layout is cache-first: the revision integer and shared
/// revision-gated surface tail precede the construction fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "RevisionCompoundLoftConstructionWire")]
pub struct RevisionCompoundLoftConstruction {
    revision: PositiveI64,
    #[cfg_attr(feature = "schema", schemars(with = "RevisionCacheForm"))]
    cache: RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>>,
    #[serde(default)]
    discontinuities: [Vec<FiniteReal>; 6],
    tail_flag: bool,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<LoftProfileMember>"))]
    base_profile: Vec<LoftProfileMember<FiniteReal, FiniteVector3>>,
    #[cfg_attr(feature = "schema", schemars(with = "LoftPath"))]
    base_path: LoftPath<FiniteReal>,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<LoftSectionEntry>"))]
    entries: Vec<LoftSectionEntry<FiniteReal, FiniteVector3>>,
    flags: [bool; 2],
    kind_flags: [bool; 2],
    #[cfg_attr(feature = "schema", schemars(with = "CompoundLoftDirection"))]
    direction: CompoundLoftDirection<FiniteVector3>,
    tail: RevisionCompoundLoftTail<CurveId, FiniteReal>,
}

/// Stored fields of a revision-gated `cl_loft_spl_sur` construction before
/// admission. This is the wire shape the deserializer reads and the only
/// input `RevisionCompoundLoftConstruction::admit` accepts.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(rename = "RevisionCompoundLoftConstruction")
)]
#[serde(deny_unknown_fields)]
pub struct RevisionCompoundLoftConstructionWire {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Leading unparameterized scale block: ordered profile members and path.
    pub base_profile: Vec<LoftProfileMember>,
    /// Path data of the leading scale block.
    pub base_path: LoftPath,
    /// Counted parameterized entries; the native parameter trails each
    /// entry's fields.
    pub entries: Vec<LoftSectionEntry>,
    /// Two flags following the entries.
    pub flags: [bool; 2],
    /// Two flags opening the kind-zero payload.
    pub kind_flags: [bool; 2],
    /// Direction carrier selected by the kind-zero direction tag.
    pub direction: CompoundLoftDirection,
    /// Trailing bounds and their dependent BS3 curve.
    pub tail: RevisionCompoundLoftTail<CurveId>,
}

impl RevisionCompoundLoftConstruction {
    /// Admit stored fields whose scalars are all finite. This is the
    /// admission of the `revision_compound_loft` procedural surface: a
    /// decoder that reads the fields admits them here, and the deserializer
    /// runs the same walk. The fields are private, so this and the
    /// deserializer are the only routes to a value.
    pub fn admit(
        wire: RevisionCompoundLoftConstructionWire,
    ) -> Result<Self, ProceduralGeometryError> {
        let invalid = || {
            ProceduralGeometryError::Payload(
                "revision compound loft construction payload is invalid",
            )
        };
        let (
            Some(discontinuities),
            Some(tail),
            Some(cache),
            Some(base_profile),
            Some(base_path),
            Some(entries),
            Some(direction),
        ) = (
            FiniteReal::lanes(wire.discontinuities),
            wire.tail.admit(),
            wire.cache.admit(),
            wire.base_profile
                .into_iter()
                .map(LoftProfileMember::admit)
                .collect::<Option<Vec<_>>>(),
            wire.base_path.admit(),
            wire.entries
                .into_iter()
                .map(LoftSectionEntry::admit)
                .collect::<Option<Vec<_>>>(),
            wire.direction.admit(),
        )
        else {
            return Err(invalid());
        };
        Ok(Self {
            revision: wire.revision,
            cache,
            discontinuities,
            tail_flag: wire.tail_flag,
            base_profile,
            base_path,
            entries,
            flags: wire.flags,
            kind_flags: wire.kind_flags,
            direction,
            tail,
        })
    }

    /// Return the positive serializer-revision integer.
    #[must_use]
    pub const fn revision(&self) -> PositiveI64 {
        self.revision
    }

    /// Return the approximation-cache form.
    #[must_use]
    pub const fn cache(&self) -> &RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>> {
        &self.cache
    }

    /// Return the six ordered discontinuity arrays.
    #[must_use]
    pub const fn discontinuities(&self) -> &[Vec<FiniteReal>; 6] {
        &self.discontinuities
    }

    /// Return the Boolean terminating the shared tail.
    #[must_use]
    pub const fn tail_flag(&self) -> bool {
        self.tail_flag
    }

    /// Return the profile members of the leading scale block.
    #[must_use]
    pub fn base_profile(&self) -> &[LoftProfileMember<FiniteReal, FiniteVector3>] {
        &self.base_profile
    }

    /// Return the path data of the leading scale block.
    #[must_use]
    pub const fn base_path(&self) -> &LoftPath<FiniteReal> {
        &self.base_path
    }

    /// Return the counted parameterized entries.
    #[must_use]
    pub fn entries(&self) -> &[LoftSectionEntry<FiniteReal, FiniteVector3>] {
        &self.entries
    }

    /// Return the two flags following the entries.
    #[must_use]
    pub const fn flags(&self) -> [bool; 2] {
        self.flags
    }

    /// Return the two flags opening the kind-zero payload.
    #[must_use]
    pub const fn kind_flags(&self) -> [bool; 2] {
        self.kind_flags
    }

    /// Return the direction carrier of the kind-zero payload.
    #[must_use]
    pub const fn direction(&self) -> &CompoundLoftDirection<FiniteVector3> {
        &self.direction
    }

    /// Return the trailing bounds and their dependent BS3 curve.
    #[must_use]
    pub const fn tail(&self) -> &RevisionCompoundLoftTail<CurveId, FiniteReal> {
        &self.tail
    }
}

impl TryFrom<RevisionCompoundLoftConstructionWire> for RevisionCompoundLoftConstruction {
    type Error = ProceduralGeometryError;

    fn try_from(wire: RevisionCompoundLoftConstructionWire) -> Result<Self, Self::Error> {
        Self::admit(wire)
    }
}

/// Trailing parameter bounds of a revision compound loft.
/// Both bounds select a trailing curve; all other forms contain no curve.
// A source states raw bounds; a `RevisionCompoundLoftConstruction` holds the
// admitted tail, whose bounds are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(
        serialize = "T: Serialize, S: Serialize",
        deserialize = "T: Deserialize<'de>, S: Deserialize<'de>"
    )
)]
pub enum RevisionCompoundLoftTail<T, S = f64> {
    /// Neither parameter bound is present.
    Unbounded {},
    /// Only the lower parameter bound is present.
    LowerBound {
        /// Lower parameter bound.
        lower: S,
    },
    /// Only the upper parameter bound is present.
    UpperBound {
        /// Upper parameter bound.
        upper: S,
    },
    /// Both parameter bounds and their selected curve.
    Curve {
        /// Ordered lower and upper parameter bounds.
        interval: [S; 2],
        /// Curve selected by the complete parameter pair.
        curve: T,
    },
}

impl<T> RevisionCompoundLoftTail<T> {
    /// The tail with admitted bounds, absent when a stored bound is not
    /// finite.
    fn admit(self) -> Option<RevisionCompoundLoftTail<T, FiniteReal>> {
        Some(match self {
            Self::Unbounded {} => RevisionCompoundLoftTail::Unbounded {},
            Self::LowerBound { lower } => RevisionCompoundLoftTail::LowerBound {
                lower: FiniteReal::new(lower)?,
            },
            Self::UpperBound { upper } => RevisionCompoundLoftTail::UpperBound {
                upper: FiniteReal::new(upper)?,
            },
            Self::Curve { interval, curve } => RevisionCompoundLoftTail::Curve {
                interval: FiniteReal::array(interval)?,
                curve,
            },
        })
    }
}

impl<T, S: Copy> RevisionCompoundLoftTail<T, S> {
    /// Optional bounds in native order.
    #[must_use]
    pub const fn interval(&self) -> [Option<S>; 2] {
        match self {
            Self::Unbounded {} => [None, None],
            Self::LowerBound { lower } => [Some(*lower), None],
            Self::UpperBound { upper } => [None, Some(*upper)],
            Self::Curve { interval, .. } => [Some(interval[0]), Some(interval[1])],
        }
    }

    /// Curve selected by a complete parameter pair.
    #[must_use]
    pub const fn curve(&self) -> Option<&T> {
        match self {
            Self::Curve { curve, .. } => Some(curve),
            _ => None,
        }
    }
}

/// One boundary record in a native vertex-blend patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct VertexBlendBoundary<R = f64, V = Vector3, P = Point3> {
    /// Native cross flag. The wire form is a logical, so the value is the
    /// tag itself and no payload follows.
    pub boundary_type: bool,
    /// Native magic direction with finite components, stored as read. It is a
    /// direction, never a length, so it carries no unit scale.
    pub magic: V,
    /// Native U-smoothing flag, a logical on the wire.
    pub u_smoothing: bool,
    /// Native V-smoothing flag, a logical on the wire.
    pub v_smoothing: bool,
    /// Native fullness scalar.
    pub fullness: R,
    /// Structurally selected boundary geometry.
    pub geometry: VertexBlendBoundaryGeometry<R, V, P>,
}

/// Twist payload selected by a vertex-blend circle form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VertexBlendTwists<P = Point3> {
    /// Native form zero: no twist entries.
    None {},
    /// Native form one: one twist entry.
    One {
        /// The one twist entry.
        twist: P,
    },
    /// Native form three: two ordered twist entries.
    Two {
        /// The two ordered twist entries.
        twists: [P; 2],
    },
}

impl<P> VertexBlendTwists<P> {
    /// Native form selected by the twist payload.
    #[must_use]
    pub const fn form(&self) -> i64 {
        match self {
            Self::None {} => 0,
            Self::One { .. } => 1,
            Self::Two { .. } => 3,
        }
    }

    /// Ordered twist entries. Their coordinate semantics depend on the layout revision.
    #[must_use]
    pub fn entries(&self) -> &[P] {
        match self {
            Self::None {} => &[],
            Self::One { twist } => std::slice::from_ref(twist),
            Self::Two { twists } => twists,
        }
    }
}

/// Type-specific geometry of a vertex-blend boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub enum VertexBlendBoundaryGeometry<R = f64, V = Vector3, P = Point3> {
    /// Curve boundary with a circle/ellipse/unknown twist form.
    Circle {
        /// Boundary curve.
        curve: CurveId,
        /// Optional native curve parameter endpoints stored by the
        /// revision-gated layout.
        #[serde(default)]
        curve_endpoints: [Option<R>; 2],
        /// Twist payload. Pre-revision layouts store model-space locations;
        /// the revision-gated layout stores unscaled twist vectors.
        twists: VertexBlendTwists<P>,
        /// Two ordered curve parameters.
        parameters: [R; 2],
        /// Native sense flag, a logical on the wire.
        sense: bool,
    },
    /// Degenerate boundary at a model-space location.
    Degenerate {
        /// Degenerate location.
        location: P,
        /// Two ordered boundary normals.
        normals: [V; 2],
    },
    /// Surface pcurve boundary.
    Pcurve {
        /// Support surface.
        surface: SurfaceId,
        /// Optional U/V bound fields stored after the support by the
        /// revision-gated layout.
        #[serde(default)]
        support_bounds: [Option<R>; 4],
        /// Native BS2 pcurve, absent for `nullbs`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pcurve"
        )]
        pcurve: Option<PcurveGeometry>,
        /// Native sense flag, a logical on the wire.
        sense: bool,
        /// Parameter-space fit tolerance.
        fit_tolerance: FitTolerance,
    },
    /// Planar boundary described by a normal and curve.
    Plane {
        /// Plane normal.
        normal: V,
        /// Two ordered plane parameters.
        parameters: [R; 2],
        /// Boundary curve.
        curve: CurveId,
        /// Optional native curve parameter endpoints stored by the
        /// revision-gated layout.
        #[serde(default)]
        curve_endpoints: [Option<R>; 2],
    },
}

/// Complete native vertex-blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct VertexBlendConstruction<R = f64, V = Vector3, P = Point3> {
    /// Positive serializer-revision integer selecting the revision-gated
    /// layout; absent from the pre-revision layout.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_revision"
    )]
    pub revision: Option<PositiveI64>,
    /// Ordered boundary records.
    pub boundaries: Vec<VertexBlendBoundary<R, V, P>>,
    /// Native grid-size integer.
    pub grid_size: i64,
    /// Native model-space fit tolerance.
    pub fit_tolerance: FitTolerance,
}

/// One member of a compound-loft scale block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct CompoundLoftScaleMember<R = f64, V = Vector3> {
    /// Native member integer.
    pub type_code: i64,
    /// Member curve.
    pub curve: CurveId,
    /// Native loft constraint data.
    pub data: ClassicLoftProfileData<R, V>,
}

/// Complete `_readScaleClLoft` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct CompoundLoftScale<R = f64, V = Vector3> {
    /// Ordered scale members.
    pub members: Vec<CompoundLoftScaleMember<R, V>>,
    /// Scale path curve.
    pub path: CurveId,
    /// Ordered BS3 auxiliary curves.
    pub auxiliaries: Vec<CurveId>,
    /// Two native trailing integers.
    pub tail: [i64; 2],
}

/// Direction carrier in the zero-kind compound-loft tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompoundLoftDirection<V = Vector3> {
    /// Inline direction vector. This form has no native selector.
    Vector {
        /// Stored direction.
        value: V,
    },
    /// BS3 direction curve and its nonzero native selector.
    Curve {
        /// Stored curve.
        curve: CurveId,
        /// Exact nonzero native selector retained for byte-faithful export.
        selector: NonZeroI64,
    },
}

impl<V> CompoundLoftDirection<V> {
    /// Native selector for this direction form.
    #[must_use]
    pub const fn selector(&self) -> i64 {
        match self {
            Self::Vector { .. } => 0,
            Self::Curve { selector, .. } => selector.get(),
        }
    }
}

/// Structurally selected tail of `cl_loft_spl_sur`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub enum CompoundLoftTail<R = f64, V = Vector3> {
    /// Native kind `6` tail.
    Six {
        /// Two leading flags.
        flags: [bool; 2],
        /// Required scale block.
        scale: Box<CompoundLoftScale<R, V>>,
        /// Native integer following the scale.
        selector: i64,
        /// Stored direction.
        direction: V,
        /// Native parameter interval.
        parameter_range: [R; 2],
        /// BS3 tail curve.
        curve: CurveId,
    },
    /// Native kind `7` tail.
    Seven {
        /// First flag.
        first_flag: bool,
        /// First optional scale block.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_first_scale"
        )]
        first_scale: Option<Box<CompoundLoftScale<R, V>>>,
        /// Second flag.
        second_flag: bool,
        /// Required second scale block.
        second_scale: Box<CompoundLoftScale<R, V>>,
        /// Native selector integer.
        selector: i64,
        /// Stored direction.
        direction: V,
        /// Two trailing flags.
        trailing_flags: [bool; 2],
    },
    /// Native kind `0` tail.
    Zero {
        /// Two leading flags.
        flags: [bool; 2],
        /// Vector or BS3 curve with its derived native selector.
        direction: CompoundLoftDirection<V>,
        /// Two trailing flags.
        trailing_flags: [bool; 2],
    },
}

/// A bounded list of compound-loft scales.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    try_from = "Vec<CompoundLoftScale<R, V>>",
    bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>")
)]
pub struct CompoundLoftScales<const CAPACITY: usize, R = f64, V = Vector3>(
    Vec<CompoundLoftScale<R, V>>,
);

impl<const CAPACITY: usize, R: Serialize, V: Serialize> Serialize
    for CompoundLoftScales<CAPACITY, R, V>
{
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

#[cfg(feature = "schema")]
impl<const CAPACITY: usize, R, V> JsonSchema for CompoundLoftScales<CAPACITY, R, V>
where
    CompoundLoftScale<R, V>: JsonSchema,
{
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("CompoundLoftScales_{CAPACITY}").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = Vec::<CompoundLoftScale<R, V>>::json_schema(generator);
        schema.insert("maxItems".into(), CAPACITY.into());
        schema
    }
}

impl<const CAPACITY: usize, R, V> CompoundLoftScales<CAPACITY, R, V> {
    /// Admit a leading scale list within the native slot capacity.
    pub fn try_new(scales: Vec<CompoundLoftScale<R, V>>) -> Result<Self, &'static str> {
        if scales.len() > CAPACITY {
            return Err("compound loft scales exceed slot capacity");
        }
        Ok(Self(scales))
    }

    /// Admit native optional slots whose present values form a leading prefix.
    pub fn try_from_slots(
        slots: impl IntoIterator<Item = Option<CompoundLoftScale<R, V>>>,
    ) -> Result<Self, &'static str> {
        let mut scales = Vec::new();
        let mut absent = false;
        for (index, slot) in slots.into_iter().enumerate() {
            if index >= CAPACITY {
                return Err("compound loft scales exceed slot capacity");
            }
            match slot {
                Some(scale) if !absent => scales.push(scale),
                Some(_) => return Err("compound loft scales must form a leading prefix"),
                None => absent = true,
            }
        }
        Ok(Self(scales))
    }

    /// Present scales in native order.
    #[must_use]
    pub fn as_slice(&self) -> &[CompoundLoftScale<R, V>] {
        &self.0
    }
}

impl<const CAPACITY: usize, R, V> TryFrom<Vec<CompoundLoftScale<R, V>>>
    for CompoundLoftScales<CAPACITY, R, V>
{
    type Error = &'static str;
    fn try_from(scales: Vec<CompoundLoftScale<R, V>>) -> Result<Self, Self::Error> {
        Self::try_new(scales)
    }
}

/// Complete native compound-loft construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct CompoundLoftConstruction<R = f64, V = Vector3> {
    /// Present scales, up to the five native slots.
    pub scales: CompoundLoftScales<5, R, V>,
    /// Two flags before the tail kind.
    pub flags: [bool; 2],
    /// Kind-specific trailing graph.
    pub tail: CompoundLoftTail<R, V>,
}

/// Initial solved-shape branch of a scaled compound loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ScaledCompoundLoftShape<R = f64> {
    /// A solved NURBS cache follows the singularity enum.
    Full {},
    /// The cache is replaced by two intervals and two scalar arrays.
    None {
        /// Two ordered native intervals.
        parameter_ranges: [[R; 2]; 2],
        /// Two ordered native scalar arrays.
        parameters: [Vec<R>; 2],
    },
}

/// Structurally selected middle branch of a scaled compound loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub enum ScaledCompoundLoftBranch<R = f64, V = Vector3> {
    /// Extended branch ending in a direction vector.
    ExtendedVector {
        /// Optional first scale block.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_first_scale"
        )]
        first_scale: Option<Box<CompoundLoftScale<R, V>>>,
        /// Required second scale block.
        second_scale: Box<CompoundLoftScale<R, V>>,
        /// Native selector integer.
        selector: i64,
        /// Stored direction vector.
        direction: V,
    },
    /// Extended branch ending in a singularity and curve.
    ExtendedCurve {
        /// Optional scale block.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_scale"
        )]
        scale: Option<Box<CompoundLoftScale<R, V>>>,
        /// Native branch flag.
        flag: bool,
        /// Native singularity enum.
        singularity: i64,
        /// Stored BS3 curve.
        curve: CurveId,
    },
    /// Direct vector-or-curve branch.
    Direct {
        /// Native branch flag.
        flag: bool,
        /// Vector or BS3 curve with its derived native selector.
        direction: CompoundLoftDirection<V>,
    },
}

/// Complete native scaled compound-loft construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct ScaledCompoundLoftConstruction<R = f64, V = Vector3> {
    /// Native leading singularity enum.
    pub singularity: i64,
    /// Singularity-selected solved-shape payload.
    pub shape: ScaledCompoundLoftShape<R>,
    /// Six ordered discontinuity arrays.
    pub discontinuities: [Vec<R>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
    /// Present leading scales within the three native slots.
    pub scales: CompoundLoftScales<3, R, V>,
    /// Two native flags preceding the selector.
    pub flags: [bool; 2],
    /// Native integer preceding the middle branch.
    pub selector: i64,
    /// Structurally selected middle branch.
    pub branch: ScaledCompoundLoftBranch<R, V>,
    /// Two trailing branch flags.
    pub trailing_flags: [bool; 2],
    /// Native trailing kind integer.
    pub tail_kind: i64,
    /// Two native trailing vectors.
    pub tail_directions: [V; 2],
    /// Native trailing singularity enum.
    pub tail_singularity: i64,
    /// Native trailing BS3 curve.
    pub tail_curve: CurveId,
}

/// One recursively framed native law formula.
// A source states raw values; a law store holds the admitted formula, whose
// scalars, vectors and points are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub enum LawFormula<R = f64, V = Vector3, P = Point3> {
    /// The native null law, with no variables.
    Null {},
    /// Named formula and its ordered recursive variables.
    Named {
        /// Native formula name.
        name: cadmpeg_core::text::NonBlankString,
        /// Ordered recursive variables.
        variables: Vec<LawExpression<R, V, P>>,
    },
}

impl<R, V, P> LawFormula<R, V, P> {
    /// Ordered recursive variables; empty for the null variant.
    #[must_use]
    pub fn variables(&self) -> &[LawExpression<R, V, P>] {
        match self {
            Self::Null {} => &[],
            Self::Named { variables, .. } => variables,
        }
    }
}

/// A law formula whose expression tree carries only finite scalars.
///
/// The carrier of the `law_int_cur` formulas. It serializes as the formula it
/// holds, so the curve wire states the formula alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LawFormula")]
pub struct FiniteLawFormula(
    #[cfg_attr(feature = "schema", schemars(with = "LawFormula"))]
    LawFormula<FiniteReal, FiniteVector3, FinitePoint3>,
);

impl TryFrom<LawFormula> for FiniteLawFormula {
    type Error = &'static str;

    fn try_from(formula: LawFormula) -> Result<Self, Self::Error> {
        Self::try_new(formula)
    }
}

impl FiniteLawFormula {
    /// Admit a law formula whose expression tree carries only finite scalars.
    pub fn try_new(formula: LawFormula) -> Result<Self, &'static str> {
        formula.admit().map(Self).ok_or(LAW_FORMULA_NOT_FINITE)
    }

    /// The admitted law formula.
    #[must_use]
    pub const fn formula(&self) -> &LawFormula<FiniteReal, FiniteVector3, FinitePoint3> {
        &self.0
    }
}

/// Refusal of a law formula that carries a non-finite scalar.
const LAW_FORMULA_NOT_FINITE: &str = "law formula constants must be finite";

/// Recursion bound of the law-expression walk. An expression nested deeper than
/// this is refused.
const LAW_EXPRESSION_DEPTH_LIMIT: usize = 64;

/// Complete recursive construction stored by a native law spline surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct LawSurfaceConstruction<R = f64, V = Vector3, P = Point3> {
    /// Legacy U and V parameter intervals; absent from modern layouts.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parameter_ranges"
    )]
    pub parameter_ranges: Option<[[R; 2]; 2]>,
    /// Primary recursive surface law.
    pub primary: LawFormula<R, V, P>,
    /// Ordered counted auxiliary laws referenced by the primary law.
    pub additional: Vec<LawFormula<R, V, P>>,
    /// Standard surface-tail mode and its mode-specific fields.
    pub tail: LawSurfaceTail<R>,
    /// Six ordered discontinuity arrays from the standard surface tail.
    pub discontinuities: [Vec<R>; 6],
}

/// Mode-specific payload of a native law surface's standard surface tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LawSurfaceTail<R = f64> {
    /// Selector 0; the surface record carries a solved NURBS cache, whose fit
    /// contract this tail states.
    Full {
        /// Fit contract of the solved cache this tail requires.
        cache: LegacyCache,
    },
    /// Selector 1; compact parameter summaries replace the solved cache.
    Summary {
        /// Ordered U and V parameter summaries.
        parameters: [Vec<R>; 2],
        /// Native model-space fit tolerance.
        fit_tolerance: FitTolerance,
        /// Ordered U and V closure enums.
        closures: [i64; 2],
        /// Ordered U and V singularity enums.
        singularities: [i64; 2],
    },
    /// Selector 2; exact parameter intervals and boundary classifications.
    None {
        /// Ordered U and V parameter intervals.
        parameter_ranges: [[R; 2]; 2],
        /// Ordered U and V closure enums.
        closures: [i64; 2],
        /// Ordered U and V singularity enums.
        singularities: [i64; 2],
    },
    /// Selector 3; no mode-specific payload.
    Historical {},
    /// Selector 4; no mode-specific payload.
    Optimal {},
}

/// One native law-expression node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub enum LawExpression<R = f64, V = Vector3, P = Point3> {
    /// Zero-payload spelling of the native null law.
    Null {},
    /// Serializer-preserved textual law expression.
    Text {
        /// Exact text stored in the native law slot.
        value: cadmpeg_core::text::NonBlankString,
    },
    /// Tagged integer constant.
    Integer {
        /// Stored integer value.
        value: i64,
    },
    /// Tagged double constant.
    Double {
        /// Stored scalar value.
        value: R,
    },
    /// Tagged model-space point constant.
    Point {
        /// Stored point value.
        value: P,
    },
    /// Tagged direction-vector constant.
    Vector {
        /// Stored vector value.
        value: V,
    },
    /// Inline transform-law payload.
    Transform {
        /// Thirteen ordered transform scalars.
        scalars: [R; 13],
        /// Three ordered transform enums.
        enums: [i64; 3],
    },
    /// Vector-serialized transform-law payload: four ordered vectors, a scale,
    /// and three flags, in place of the thirteen-scalar/three-enum form.
    TransformVec {
        /// Four ordered transform vectors.
        vectors: [V; 4],
        /// Trailing transform scale.
        scale: R,
        /// Three ordered transform flags.
        flags: [bool; 3],
    },
    /// Curve-backed edge law.
    Edge {
        /// Embedded curve carrier and its optional revision-gated endpoints.
        #[serde(flatten)]
        curve: LoftPathCurve<R>,
        /// Two native curve parameters.
        parameters: [R; 2],
    },
    /// Spline-law payload.
    Spline {
        /// Native spline-law integer.
        native_id: i64,
        /// Ordered spline-law knots.
        knots: Vec<R>,
        /// Ordered spline-law controls.
        controls: Vec<R>,
        /// Native model-space point.
        point: P,
    },
    /// Algebraic operator and its recursively framed operands.
    Algebraic {
        /// Native operator token.
        operator: String,
        /// Ordered operands.
        operands: Vec<LawExpression<R, V, P>>,
    },
}

/// One profile entry in the expanded skin layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct SkinSurfaceProfile<R = f64, V = Vector3> {
    /// Native profile type integer.
    pub type_code: i64,
    /// Profile curve.
    pub curve: CurveId,
    /// Native loft constraint data.
    pub data: ClassicLoftProfileData<R, V>,
}

/// Structurally selected native skin payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub enum SkinSurfaceLayout<R = f64, V = Vector3> {
    /// Expanded sequence of profile curves and loft constraints.
    Profiles {
        /// Ordered profile entries.
        profiles: Vec<SkinSurfaceProfile<R, V>>,
        /// Trailing path curve.
        path: CurveId,
        /// Two native trailing integers.
        tail: [i64; 2],
    },
    /// Compact curve/subdata form.
    Compact {
        /// Native compact-layout inner integer.
        inner_count: i64,
        /// Primary curve.
        curve: CurveId,
        /// Native loft subdata.
        subdata: LoftSubdata<R>,
        /// Integer after the subdata.
        first_tail: i64,
        /// Secondary curve.
        secondary_curve: CurveId,
        /// Final compact-layout integer.
        second_tail: i64,
    },
}

impl<R, V> SkinSurfaceLayout<R, V> {
    /// Native inner count, derived from the profile list in the expanded form.
    pub fn inner_count(&self) -> i64 {
        match self {
            Self::Profiles { profiles, .. } => profiles.len() as i64,
            Self::Compact { inner_count, .. } => *inner_count,
        }
    }
}

/// Complete native `skin_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct SkinSurfaceConstruction<R = f64, V = Vector3, P = Point3> {
    /// Native `SURF_BOOL` enum.
    pub surface_boolean: i64,
    /// Native `SURF_NORM` enum.
    pub surface_normal: i64,
    /// Native `SURF_DIR` enum.
    pub surface_direction: i64,
    /// Native leading count.
    pub count: i64,
    /// Native leading scalar.
    pub parameter: R,
    /// Structurally selected skin payload.
    pub layout: SkinSurfaceLayout<R, V>,
    /// Stored direction vector.
    pub direction: V,
    /// Native scalar before the formula.
    pub trailing_parameter: R,
    /// Recursive parametric law.
    pub formula: LawFormula<R, V, P>,
    /// Trailing curve after the formula.
    pub parameter_curve: CurveId,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<R>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Complete native `net_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct NetSurfaceConstruction<R = f64, V = Vector3, P = Point3> {
    /// Two ordered loft-section graphs.
    pub sections: Box<[LoftSection<R, V>; 2]>,
    /// Twelve ordered frame scalars.
    pub frame_parameters: [R; 12],
    /// Native frame integer.
    pub flag: i64,
    /// Four ordered frame directions.
    pub directions: [V; 4],
    /// Four ordered parameter laws.
    pub formulas: Box<[LawFormula<R, V, P>; 4]>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<R>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Structurally selected native sweep payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub enum SweepSurfaceLayout<R = f64, V = Vector3, P = Point3> {
    /// Profile-first modern ASM sweep layout.
    ProfileFirst {
        /// Second native sweep enum.
        secondary_kind: i64,
        /// Five ordered frame directions.
        directions: [V; 5],
        /// Native model-space frame origin.
        origin: P,
        /// Four ordered native frame scalars.
        parameters: [R; 4],
        /// Three ordered parametric laws.
        formulas: Box<[LawFormula<R, V, P>; 3]>,
    },
    /// Explicit sweep layout whose trajectory is controlled by a formula.
    ExplicitFormula {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [R; 2],
        /// Optional explicit profile frame.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        profile_frame: Option<(P, V)>,
        /// Sweep frame origin.
        origin: P,
        /// Three ordered sweep frame directions.
        directions: [V; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [R; 2],
        /// Native trajectory scalar.
        path_parameter: R,
        /// Native formula-side boolean.
        formula_flag: bool,
        /// Parametric trajectory formula.
        formula: LawFormula<R, V, P>,
        /// Native trailing boolean.
        trailing_flag: bool,
    },
    /// Explicit sweep layout controlled by an auxiliary guide curve.
    ExplicitGuide {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [R; 2],
        /// Optional explicit profile frame.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        profile_frame: Option<(P, V)>,
        /// Sweep frame origin.
        origin: P,
        /// Three ordered sweep frame directions.
        directions: [V; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [R; 2],
        /// Native trajectory scalar.
        path_parameter: R,
        /// Two guide-side booleans.
        guide_flags: [bool; 2],
        /// Auxiliary guide curve.
        guide_curve: CurveId,
        /// Guide parameter interval.
        guide_range: [R; 2],
        /// Two native guide integers.
        guide_modes: [i64; 2],
        /// Six ordered guide scalars.
        guide_parameters: [R; 6],
        /// Three trailing guide booleans.
        trailing_flags: [bool; 3],
    },
    /// Explicit sweep layout controlled by a support surface.
    ExplicitSurface {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [R; 2],
        /// Optional explicit profile frame.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        profile_frame: Option<(P, V)>,
        /// Sweep frame origin.
        origin: P,
        /// Three ordered sweep frame directions.
        directions: [V; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [R; 2],
        /// Native trajectory scalar.
        path_parameter: R,
        /// Native singularity enum.
        singularity: i64,
        /// Support surface controlling the sweep.
        support_surface: SurfaceId,
        /// Optional auxiliary curve.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        auxiliary_curve: Option<CurveId>,
        /// Native support-side boolean.
        support_flag: bool,
        /// Legacy pre-219 trailing boolean when present.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        legacy_flag: Option<bool>,
    },
    /// Explicit-prefix sweep layout controlled by recursive laws.
    LawDriven {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [R; 2],
        /// Optional explicit profile frame.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        profile_frame: Option<(P, V)>,
        /// Sweep frame origin.
        origin: P,
        /// Three ordered sweep frame directions.
        directions: [V; 3],
        /// Leading recursive sweep law.
        first_law: Box<LawExpression<R, V, P>>,
        /// Native integer after the leading law.
        first_mode: i64,
        /// First law parameter interval.
        first_range: [R; 2],
        /// Native law direction.
        law_direction: V,
        /// Native path integer.
        path_mode: i64,
        /// Native path boolean.
        path_flag: bool,
        /// Path parameter interval.
        path_range: [R; 2],
        /// Native path scalar.
        path_parameter: R,
        /// Native second-law boolean.
        second_law_flag: bool,
        /// Trailing recursive sweep law.
        second_law: Box<LawExpression<R, V, P>>,
        /// Native integer before the formula.
        formula_mode: i64,
        /// Parametric trajectory formula.
        formula: LawFormula<R, V, P>,
        /// Native trailing boolean.
        trailing_flag: bool,
    },
}

/// Revision-gated `sweep_sur` form fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct SweepRevisionForm<R = f64> {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Boolean replacing the pre-revision primary enum.
    pub primary_flag: bool,
    /// Optional parameter endpoints following the embedded profile curve.
    #[serde(default)]
    pub profile_endpoints: [Option<R>; 2],
    /// Optional parameter endpoints following the embedded path curve.
    #[serde(default)]
    pub path_endpoints: [Option<R>; 2],
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm<RevisionSurfaceParameterization<R>>,
}

/// Complete native `sweep_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct SweepSurfaceConstruction<R = f64, V = Vector3, P = Point3> {
    /// Leading native sweep enum.
    pub primary_kind: i64,
    /// Cache contract: the revision-gated form, or the legacy solved-cache
    /// tolerance this construction states instead.
    #[serde(default, skip_serializing_if = "CacheContract::is_bare_legacy")]
    pub cache: CacheContract<SweepRevisionForm<R>>,
    /// Structurally selected sweep layout.
    pub layout: SweepSurfaceLayout<R, V, P>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<R>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Radius law for a procedural blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BlendRadiusLaw {
    /// Constant blend radius along the whole spine.
    Constant {
        /// Signed radius, in document length units; sign selects the support offset side.
        signed_radius: crate::scalar::Length,
    },
    /// Radius varying linearly from `start` to `end` along the spine.
    Linear {
        /// Signed radius at the spine start, in document length units.
        start: crate::scalar::Length,
        /// Signed radius at the spine end, in document length units.
        end: crate::scalar::Length,
    },
    /// Radius varying along the spine per an explicit law curve.
    Law {
        /// Curve whose parameterization gives the signed radius along the spine.
        curve: NurbsCurve,
    },
}

/// The refusal of a blend radius law with a non-finite radius.
const NON_FINITE_BLEND_RADIUS: ProceduralGeometryError =
    ProceduralGeometryError::Payload("blend radius law is not finite");

impl BlendRadiusLaw {
    /// Admit a constant law from a finite signed radius. The sign of a radius
    /// selects the support offset side, so a negative radius is admitted.
    pub fn constant(signed_radius: f64) -> Result<Self, ProceduralGeometryError> {
        Ok(Self::Constant {
            signed_radius: crate::scalar::Length::new(signed_radius)
                .ok_or(NON_FINITE_BLEND_RADIUS)?,
        })
    }

    /// Admit a linear law from finite signed radii at the spine start and
    /// end, with the refusal of [`BlendRadiusLaw::constant`].
    pub fn linear(start: f64, end: f64) -> Result<Self, ProceduralGeometryError> {
        Ok(Self::Linear {
            start: crate::scalar::Length::new(start).ok_or(NON_FINITE_BLEND_RADIUS)?,
            end: crate::scalar::Length::new(end).ok_or(NON_FINITE_BLEND_RADIUS)?,
        })
    }
}

/// A neutral curve construction linked to its solved carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProceduralCurve {
    /// Stable construction identity.
    pub id: ProceduralCurveId,
    /// Neutral construction definition, which states its own cache contract.
    definition: ProceduralCurveDefinition,
}

/// A parameter-space support curve and its optional affine parameter map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SupportPcurve {
    /// UV curve carried by the support side.
    pub geometry: PcurveGeometry,
    /// Directed pcurve interval corresponding to the solved interval.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parameter_range"
    )]
    pub parameter_range: Option<DirectedParameterRange>,
}

impl SupportPcurve {
    /// Pair a pcurve geometry with an optional checked parameter interval.
    #[must_use]
    pub const fn new(
        geometry: PcurveGeometry,
        parameter_range: Option<DirectedParameterRange>,
    ) -> Self {
        Self {
            geometry,
            parameter_range,
        }
    }
}

impl From<PcurveGeometry> for SupportPcurve {
    fn from(geometry: PcurveGeometry) -> Self {
        Self::new(geometry, None)
    }
}

/// A finite, non-zero-width directed parameter interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct DirectedParameterRange([f64; 2]);

/// Error returned when a directed parameter range cannot be admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterRangeError;

impl std::fmt::Display for ParameterRangeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("parameter range endpoints must be finite and distinct")
    }
}

impl std::error::Error for ParameterRangeError {}

impl DirectedParameterRange {
    /// Construct an interval. Either ascending or descending direction is valid.
    pub fn new(values: [f64; 2]) -> Result<Self, ParameterRangeError> {
        if values.iter().all(|value| value.is_finite()) && values[0] != values[1] {
            Ok(Self(values))
        } else {
            Err(ParameterRangeError)
        }
    }

    /// Return the directed endpoints.
    #[must_use]
    pub const fn endpoints(self) -> [f64; 2] {
        self.0
    }
}

impl<'de> Deserialize<'de> for DirectedParameterRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(<[f64; 2]>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Refuse a zero-width interval under a support side with an explicit pcurve
/// mapping.
fn require_mapped_interval(
    sides: &[IntcurveSupportSide; 2],
    parameter_range: crate::topology::ParameterInterval,
) -> Result<(), &'static str> {
    let [start, end] = parameter_range.endpoints();
    if start == end
        && sides.iter().any(|side| {
            side.pcurve
                .as_ref()
                .is_some_and(|pcurve| pcurve.parameter_range.is_some())
        })
    {
        return Err(
            "support context parameter_range must be nonzero for an explicit pcurve mapping",
        );
    }
    Ok(())
}

/// One paired surface and parameter-space curve in an intcurve construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IntcurveSupportSide {
    /// Supporting surface, absent for the native `null_surface` sentinel.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_surface"
    )]
    pub surface: Option<SurfaceId>,
    /// UV curve on `surface`, absent for the native `nullbs` sentinel.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_intcurve_support_side_pcurve"
    )]
    pub pcurve: Option<SupportPcurve>,
}

impl IntcurveSupportSide {
    /// Return the mapped pcurve interval, when this side's pcurve states one.
    #[must_use]
    pub fn pcurve_parameter_range(&self) -> Option<DirectedParameterRange> {
        self.pcurve
            .as_ref()
            .and_then(|pcurve| pcurve.parameter_range)
    }

    /// Map one solved-curve parameter into this side's pcurve parameter.
    ///
    /// Returns `None` when this side has no pcurve, an explicit map has a
    /// zero-width solved interval, or the mapped parameter cannot be
    /// represented as finite. Each branch admits the parameter it returns
    /// where it computes it.
    #[must_use]
    pub fn pcurve_parameter(
        &self,
        solved_parameter_range: crate::topology::ParameterInterval,
        parameter: f64,
    ) -> Option<FiniteReal> {
        let admitted_parameter = FiniteReal::new(parameter)?;
        let Some(pcurve_interval) = self.pcurve.as_ref()?.parameter_range else {
            return Some(admitted_parameter);
        };
        let [pcurve_start, pcurve_end] = pcurve_interval.finite_endpoints();
        let pcurve_range = pcurve_interval.endpoints();
        let solved_parameter_range = solved_parameter_range.endpoints();
        let solved_span = solved_parameter_range[1] - solved_parameter_range[0];
        if solved_span == 0.0 {
            return None;
        }
        if parameter == solved_parameter_range[0] {
            return Some(pcurve_start);
        }
        if parameter == solved_parameter_range[1] {
            return Some(pcurve_end);
        }
        let offset = parameter - solved_parameter_range[0];
        let mapped_span = pcurve_range[1] - pcurve_range[0];
        if solved_span.is_finite() && offset.is_finite() && mapped_span.is_finite() {
            let fraction = offset / solved_span;
            let advance = fraction * mapped_span;
            if fraction.is_normal() && advance.is_normal() {
                if let Some(mapped) = fast_dot(
                    [pcurve_range[0], advance],
                    [1.0, 1.0],
                    [pcurve_range[0], advance],
                ) {
                    return Some(mapped);
                }
            }
        }
        // Evaluate the affine numerator before division. A tiny fraction can
        // underflow even when the final mapped parameter is representable.
        let mut numerator = ExactSignedSum::default();
        numerator.add_product(pcurve_range[0], solved_parameter_range[1]);
        numerator.add_product(-pcurve_range[0], parameter);
        numerator.add_product(pcurve_range[1], parameter);
        numerator.add_product(-pcurve_range[1], solved_parameter_range[0]);
        let mut denominator = ExactSignedSum::default();
        denominator.add_product(solved_parameter_range[1], 1.0);
        denominator.add_product(-solved_parameter_range[0], 1.0);
        let denominator = denominator.finish()?;
        numerator.finish().map_or(Some(FiniteReal::ZERO), |value| {
            value.quotient(denominator).ok()
        })
    }
}

/// Version-stamp prefix and unbounded interval carried by the stamped
/// `law_int_cur` serializer form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LawCurveVersionFormWire")]
pub struct LawCurveVersionForm {
    /// Serializer version stamp emitted after the subtype name.
    stamp: i64,
    /// Native enum following the version stamp.
    post_enum: i64,
    /// Solved-curve interval endpoints; `None` records an unbounded bound.
    parameter_range: [Option<FiniteReal>; 2],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LawCurveVersionFormWire {
    /// Serializer version stamp emitted after the subtype name.
    stamp: i64,
    /// Native enum following the version stamp.
    post_enum: i64,
    /// Solved-curve interval endpoints; `None` records an unbounded bound.
    parameter_range: [Option<f64>; 2],
}

impl TryFrom<LawCurveVersionFormWire> for LawCurveVersionForm {
    type Error = &'static str;

    fn try_from(wire: LawCurveVersionFormWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.stamp, wire.post_enum, wire.parameter_range)
    }
}

impl LawCurveVersionForm {
    /// Admit a stamped law-curve form whose interval endpoints are finite.
    pub fn try_new(
        stamp: i64,
        post_enum: i64,
        parameter_range: [Option<f64>; 2],
    ) -> Result<Self, &'static str> {
        let parameter_range = FiniteReal::optional(parameter_range)
            .ok_or("law curve parameter_range bounds must be finite")?;
        Ok(Self {
            stamp,
            post_enum,
            parameter_range,
        })
    }

    /// Serializer version stamp emitted after the subtype name.
    #[must_use]
    pub const fn stamp(&self) -> i64 {
        self.stamp
    }

    /// Native enum following the version stamp.
    #[must_use]
    pub const fn post_enum(&self) -> i64 {
        self.post_enum
    }

    /// Solved-curve interval endpoints; `None` records an unbounded bound.
    #[must_use]
    pub const fn parameter_range(&self) -> [Option<FiniteReal>; 2] {
        self.parameter_range
    }
}

/// Shared support surfaces, UV curves, interval, and discontinuity arrays of a native intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "IntcurveSupportContextWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct IntcurveSupportContext {
    sides: [IntcurveSupportSide; 2],
    parameter_range: crate::topology::ParameterInterval,
    discontinuities: [Vec<FiniteReal>; 3],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct IntcurveSupportContextWire {
    /// Ordered support sides.
    sides: [IntcurveSupportSide; 2],
    /// Solved-curve interval.
    parameter_range: [f64; 2],
    /// Ordered discontinuity arrays.
    discontinuities: [Vec<f64>; 3],
}

impl TryFrom<IntcurveSupportContextWire> for IntcurveSupportContext {
    type Error = &'static str;

    fn try_from(wire: IntcurveSupportContextWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.sides, wire.parameter_range, wire.discontinuities)
    }
}

impl IntcurveSupportContext {
    /// Construct a support context with a finite ordered interval and finite discontinuities.
    pub fn try_new(
        sides: [IntcurveSupportSide; 2],
        parameter_range: [f64; 2],
        discontinuities: [Vec<f64>; 3],
    ) -> Result<Self, &'static str> {
        let parameter_range = crate::topology::ParameterInterval::new(parameter_range)
            .map_err(|_| "support context parameter_range must be finite and ordered")?;
        require_mapped_interval(&sides, parameter_range)?;
        let discontinuities = FiniteReal::lanes(discontinuities)
            .ok_or("support context discontinuities must be finite")?;
        Ok(Self {
            sides,
            parameter_range,
            discontinuities,
        })
    }

    /// Construct a support context from an admitted interval and admitted
    /// discontinuities. Only the interval's width against an explicit pcurve
    /// mapping is tested.
    pub fn from_parts(
        sides: [IntcurveSupportSide; 2],
        parameter_range: crate::topology::ParameterInterval,
        discontinuities: [Vec<FiniteReal>; 3],
    ) -> Result<Self, &'static str> {
        require_mapped_interval(&sides, parameter_range)?;
        Ok(Self {
            sides,
            parameter_range,
            discontinuities,
        })
    }

    /// Build a support context over an increasing interval with no
    /// discontinuities. The interval type states finite, ordered and nonzero
    /// endpoints, which is the whole interval condition of [`Self::try_new`],
    /// so nothing is checked.
    #[must_use]
    pub fn over_interval(
        sides: [IntcurveSupportSide; 2],
        parameter_range: crate::topology::IncreasingParameterInterval,
    ) -> Self {
        Self {
            sides,
            parameter_range: parameter_range.into(),
            discontinuities: std::array::from_fn(|_| Vec::new()),
        }
    }

    /// Set a support surface without changing its pcurve mapping.
    pub fn set_surface(&mut self, side: usize, surface: Option<SurfaceId>) {
        self.sides[side].surface = surface;
    }

    /// Set a support pcurve with the solved-curve parameterization.
    pub fn set_unmapped_pcurve(&mut self, side: usize, geometry: Option<PcurveGeometry>) {
        self.sides[side].pcurve = geometry.map(SupportPcurve::from);
    }

    /// Copy a pcurve mapping between support sides of this context.
    pub fn copy_pcurve(&mut self, source: usize, target: usize) {
        let (source, target) = match source.cmp(&target) {
            std::cmp::Ordering::Less => {
                let (before, after) = self.sides.split_at_mut(target);
                (&before[source], &mut after[0])
            }
            std::cmp::Ordering::Greater => {
                let (before, after) = self.sides.split_at_mut(source);
                (&after[0], &mut before[target])
            }
            std::cmp::Ordering::Equal => return,
        };
        target.pcurve.clone_from(&source.pcurve);
    }

    /// Return the ordered support sides.
    #[must_use]
    pub const fn sides(&self) -> &[IntcurveSupportSide; 2] {
        &self.sides
    }

    /// Return the solved-curve interval.
    #[must_use]
    pub const fn parameter_range(&self) -> crate::topology::ParameterInterval {
        self.parameter_range
    }

    /// Return the ordered discontinuity arrays.
    #[must_use]
    pub const fn discontinuities(&self) -> &[Vec<FiniteReal>; 3] {
        &self.discontinuities
    }
}

/// Finite endpoint witnesses and distinct supports of a tolerant intersection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TolerantIntersectionConstructionWire")]
pub struct TolerantIntersectionConstruction {
    supports: [SurfaceId; 2],
    endpoints: [FinitePoint3; 2],
    tolerance: NonNegativeReal,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TolerantIntersectionConstructionWire {
    /// Distinct support surfaces of the intersection.
    supports: [SurfaceId; 2],
    /// Model-space endpoint witnesses.
    endpoints: [Point3; 2],
    /// Finite non-negative intersection tolerance.
    tolerance: f64,
}

impl TryFrom<TolerantIntersectionConstructionWire> for TolerantIntersectionConstruction {
    type Error = &'static str;
    fn try_from(wire: TolerantIntersectionConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.supports, wire.endpoints, wire.tolerance)
    }
}

impl TolerantIntersectionConstruction {
    /// Admit distinct supports, finite endpoints, and a finite non-negative tolerance.
    pub fn try_new(
        supports: [SurfaceId; 2],
        endpoints: [Point3; 2],
        tolerance: f64,
    ) -> Result<Self, &'static str> {
        if supports[0] == supports[1] {
            return Err("tolerant intersection supports must be distinct");
        }
        let [start, end] = endpoints.map(FinitePoint3::new);
        let (Some(start), Some(end)) = (start, end) else {
            return Err("tolerant intersection endpoints must be finite");
        };
        let endpoints = [start, end];
        let tolerance = NonNegativeReal::new(tolerance)
            .ok_or("tolerant intersection tolerance must be finite and non-negative")?;
        Ok(Self {
            supports,
            endpoints,
            tolerance,
        })
    }

    /// Admit distinct supports with admitted endpoints and tolerance. The
    /// endpoint and tolerance types state their conditions, so only the
    /// distinctness of the supports is checked.
    pub fn from_parts(
        supports: [SurfaceId; 2],
        endpoints: [FinitePoint3; 2],
        tolerance: NonNegativeReal,
    ) -> Result<Self, &'static str> {
        if supports[0] == supports[1] {
            return Err("tolerant intersection supports must be distinct");
        }
        Ok(Self {
            supports,
            endpoints,
            tolerance,
        })
    }

    /// Return the supports.
    #[must_use]
    pub const fn supports(&self) -> &[SurfaceId; 2] {
        &self.supports
    }

    /// Return the endpoints.
    #[must_use]
    pub const fn endpoints(&self) -> &[FinitePoint3; 2] {
        &self.endpoints
    }

    /// Return the admitted tolerance.
    #[must_use]
    pub const fn tolerance(&self) -> NonNegativeReal {
        self.tolerance
    }
}

/// Complete neutral parameterization of one topology-bounded intersection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TolerantIntersectionParameterizationWire")]
pub struct TolerantIntersectionParameterization {
    /// Coincident support charts in support order.
    pub pcurves: [PcurveGeometry; 2],
    parameter_range: crate::topology::IncreasingParameterInterval,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct TolerantIntersectionParameterizationWire {
    /// Coincident support charts in support order.
    pcurves: [PcurveGeometry; 2],
    /// Common finite solved-curve interval.
    parameter_range: [f64; 2],
}

impl TryFrom<TolerantIntersectionParameterizationWire> for TolerantIntersectionParameterization {
    type Error = &'static str;
    fn try_from(wire: TolerantIntersectionParameterizationWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.pcurves, wire.parameter_range)
    }
}

impl TolerantIntersectionParameterization {
    /// Admit a finite strictly increasing solved-curve interval.
    pub fn try_new(
        pcurves: [PcurveGeometry; 2],
        parameter_range: [f64; 2],
    ) -> Result<Self, &'static str> {
        let parameter_range = crate::topology::IncreasingParameterInterval::new(parameter_range)
            .ok_or(
                "tolerant intersection parameter_range must be finite and strictly increasing",
            )?;
        Ok(Self {
            pcurves,
            parameter_range,
        })
    }

    /// Common finite solved-curve interval.
    #[must_use]
    pub const fn parameter_range(&self) -> crate::topology::IncreasingParameterInterval {
        self.parameter_range
    }
}

/// Cache-first shared-context fields absent from the context-first layout.
// A source states raw scalars; a curve construction holds the admitted form,
// whose scalars are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct CacheFirstCurveForm<R = f64> {
    /// Positive serializer-revision integer selecting the cache-first layout.
    pub revision: PositiveI64,
    /// Approximation-cache form selected by the shared context enum.
    pub cache: RevisionCacheForm<CacheFirstCurveParameterization<R>>,
    /// Optional U/V bound fields following each ordered support surface.
    #[serde(default)]
    pub support_bounds: [[Option<R>; 4]; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    #[serde(default)]
    pub solved_range: [Option<R>; 2],
    /// Native integer ASM extension following the discontinuity arrays.
    pub extension: i64,
}

/// One support slot in a context-first spring construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SpringSupport<R = f64> {
    /// Resolved support surface.
    Surface(SurfaceId),
    /// Native U/V ranges stored in place of `null_surface`.
    Ranges([[R; 2]; 2]),
}

/// First pcurve slot in a context-first spring construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SpringPcurve<R = f64> {
    /// Resolved parameter-space curve.
    Pcurve(PcurveGeometry),
    /// Native interval stored in place of `nullbs`.
    Range([R; 2]),
}

/// Mutually exclusive spring construction layouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
// Variant payloads retain the native layout as one value without separate heap ownership.
#[allow(clippy::large_enum_variant)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, I: JsonSchema + Serialize")
)]
pub enum SpringLayout<R = f64, I = [f64; 2]> {
    /// Support-first layout with inline null-carrier replacement ranges.
    ContextFirst {
        /// Two ordered support slots.
        supports: [SpringSupport<R>; 2],
        /// First pcurve or its null replacement range.
        first_pcurve: SpringPcurve<R>,
        /// Nullable second pcurve slot.
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        second_pcurve: Option<PcurveGeometry>,
        /// Native solved-curve parameter interval.
        parameter_range: I,
        /// Three ordered discontinuity arrays.
        discontinuities: [Vec<R>; 3],
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Solved-cache fit contract this layout states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    /// Cache-first layout, which carries no inline replacement ranges or
    /// context-first discontinuity flag.
    CacheFirst {
        /// Shared support context following the solved cache.
        context: IntcurveSupportContext,
        /// Cache-first serializer fields.
        form: CacheFirstCurveForm<R>,
    },
}

/// The support sides a context-first spring layout states.
fn spring_context_sides<R>(
    supports: &[SpringSupport<R>; 2],
    first_pcurve: &SpringPcurve<R>,
    second_pcurve: Option<&PcurveGeometry>,
) -> [IntcurveSupportSide; 2] {
    [
        IntcurveSupportSide {
            surface: match &supports[0] {
                SpringSupport::Surface(surface) => Some(surface.clone()),
                SpringSupport::Ranges(_) => None,
            },
            pcurve: match first_pcurve {
                SpringPcurve::Pcurve(pcurve) => Some(SupportPcurve::new(pcurve.clone(), None)),
                SpringPcurve::Range(_) => None,
            },
        },
        IntcurveSupportSide {
            surface: match &supports[1] {
                SpringSupport::Surface(surface) => Some(surface.clone()),
                SpringSupport::Ranges(_) => None,
            },
            pcurve: second_pcurve
                .cloned()
                .map(|pcurve| SupportPcurve::new(pcurve, None)),
        },
    ]
}

impl SpringLayout<FiniteReal, crate::topology::ParameterInterval> {
    /// Return the support context, deriving it for the context-first layout
    /// from the admitted interval and discontinuities.
    pub fn support_context(
        &self,
    ) -> Result<std::borrow::Cow<'_, IntcurveSupportContext>, &'static str> {
        match self {
            Self::CacheFirst { context, .. } => Ok(std::borrow::Cow::Borrowed(context)),
            Self::ContextFirst {
                supports,
                first_pcurve,
                second_pcurve,
                parameter_range,
                discontinuities,
                ..
            } => Ok(std::borrow::Cow::Owned(IntcurveSupportContext::from_parts(
                spring_context_sides(supports, first_pcurve, second_pcurve.as_ref()),
                *parameter_range,
                discontinuities.clone(),
            )?)),
        }
    }
}

impl<R, I> SpringLayout<R, I> {
    fn cache_first(&self) -> Option<&CacheFirstCurveForm<R>> {
        match self {
            Self::CacheFirst { form, .. } => Some(form),
            Self::ContextFirst { .. } => None,
        }
    }

    /// Write the fit tolerance of the cache-first layout's cache form.
    ///
    /// The narrow write route: the borrow of the form stays inside this
    /// method, so the layout lends no interior of the admitted payload that
    /// holds it. The context-first layout owns no cache form.
    fn write_revision_fit_tolerance(
        &mut self,
        value: FitTolerance,
        write: ToleranceWrite,
    ) -> RevisionCacheWrite {
        match self {
            Self::CacheFirst { form, .. } => form.cache.write_fit_tolerance(value, write),
            Self::ContextFirst { .. } => RevisionCacheWrite::NoForm,
        }
    }
}

/// Parameterization carried by cache form `2` of the shared cache-first
/// intcurve context. This form stores no solved curve cache and no fit
/// tolerance; it stores the curve interval followed by the closed-form enum, in
/// the order the fields appear below.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct CacheFirstCurveParameterization<R = f64> {
    /// Curve interval, an ordered `[lo, hi]` pair of optional bounds. `None` is
    /// a false bound-presence flag.
    #[serde(default)]
    pub interval: [Option<R>; 2],
    /// Closed-form enum following the interval.
    pub closed_form: i64,
}

/// Family-independent tail fields carried by a cache-first surface curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SurfaceCurveTailWire")]
pub struct SurfaceCurveTail {
    /// Native integer following the discontinuity arrays.
    extension: i64,
    /// Positive serializer-revision integer opening the cache-first layout.
    revision: PositiveI64,
    /// Approximation-cache form selected by the shared context enum.
    #[cfg_attr(
        feature = "schema",
        schemars(with = "RevisionCacheForm<CacheFirstCurveParameterization>")
    )]
    cache: RevisionCacheForm<CacheFirstCurveParameterization<FiniteReal>>,
    /// Optional U/V bound fields following each ordered support surface.
    support_bounds: [RecordBounds; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    #[serde(default)]
    solved_range: [Option<FiniteReal>; 2],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SurfaceCurveTailWire {
    /// Native integer following the discontinuity arrays.
    extension: i64,
    /// Positive serializer-revision integer opening the cache-first layout.
    revision: PositiveI64,
    /// Approximation-cache form selected by the shared context enum.
    cache: RevisionCacheForm<CacheFirstCurveParameterization>,
    /// Optional U/V bound fields following each ordered support surface.
    #[serde(default)]
    support_bounds: [[Option<f64>; 4]; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    #[serde(default)]
    solved_range: [Option<f64>; 2],
}

impl TryFrom<SurfaceCurveTailWire> for SurfaceCurveTail {
    type Error = &'static str;

    fn try_from(wire: SurfaceCurveTailWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.extension,
            wire.revision,
            wire.cache,
            wire.support_bounds,
            wire.solved_range,
        )
    }
}

impl SurfaceCurveTail {
    /// Admit a cache-first surface-curve tail around an admitted revision.
    /// The support bounds, the solved range and the cache interval are raw
    /// values; a non-finite one is refused.
    pub fn try_new(
        extension: i64,
        revision: PositiveI64,
        cache: RevisionCacheForm<CacheFirstCurveParameterization>,
        support_bounds: [[Option<f64>; 4]; 2],
        solved_range: [Option<f64>; 2],
    ) -> Result<Self, &'static str> {
        const INVALID: &str = "surface curve tail bounds must be finite";
        let [first, second] = support_bounds.map(|bounds| RecordBounds::try_new(bounds).ok());
        let (Some(first), Some(second), Some(solved_range), Some(cache)) = (
            first,
            second,
            FiniteReal::optional(solved_range),
            cache.admit(),
        ) else {
            return Err(INVALID);
        };
        Ok(Self {
            extension,
            revision,
            cache,
            support_bounds: [first, second],
            solved_range,
        })
    }

    /// Native integer following the discontinuity arrays.
    #[must_use]
    pub const fn extension(&self) -> i64 {
        self.extension
    }

    /// Serializer-revision integer opening the cache-first layout.
    #[must_use]
    pub const fn revision(&self) -> PositiveI64 {
        self.revision
    }

    /// Approximation-cache form selected by the shared context enum.
    #[must_use]
    pub const fn cache(&self) -> &RevisionCacheForm<CacheFirstCurveParameterization<FiniteReal>> {
        &self.cache
    }

    /// Optional U/V bound fields following each ordered support surface.
    #[must_use]
    pub const fn support_bounds(&self) -> &[RecordBounds; 2] {
        &self.support_bounds
    }

    /// Optional solved-curve interval endpoints.
    #[must_use]
    pub const fn solved_range(&self) -> &[Option<FiniteReal>; 2] {
        &self.solved_range
    }
}

/// Cache-first surface-curve tail paired with its family-specific flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SurfaceCurveCacheFirst<F> {
    /// Family-independent cache-first fields.
    pub form: SurfaceCurveTail,
    /// Flags admitted by the selected surface-curve family.
    pub flags: F,
}

/// Two terminating flags carried only by a parametric surface curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ParametricSurfaceCurveFlags {
    /// Support-slot selector.
    pub flag: bool,
    /// Optional later-revision terminating flag.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_flag"
    )]
    pub second_flag: Option<bool>,
}

/// Mutually exclusive tail forms of a native projected intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectionTail<R = f64> {
    /// The ASM flag is followed immediately by the subtype close.
    EarlyClose {
        /// Native ASM projection flag.
        flag: bool,
    },
    /// The ASM flag is followed by a retained source interval and role text.
    Ranged {
        /// Native ASM projection flag.
        flag: bool,
        /// Native parameter interval on the projected source curve.
        parameter_range: [R; 2],
        /// Projection support role.
        role: ProjectionRole,
    },
}

/// Support selected by a ranged projection tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum ProjectionRole {
    /// First support surface.
    #[serde(rename = "surf1")]
    Surf1,
    /// Second support surface.
    #[serde(rename = "surf2")]
    Surf2,
}

impl ProjectionRole {
    /// Parse the native ASM identifier.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "surf1" => Some(Self::Surf1),
            "surf2" => Some(Self::Surf2),
            _ => None,
        }
    }

    /// Native ASM identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Surf1 => "surf1",
            Self::Surf2 => "surf2",
        }
    }
}

impl<'de> Deserialize<'de> for ProjectionRole {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid projection role field value `{value}`; expected `surf1` or `surf2`"
            ))
        })
    }
}

/// Native surface-curve family with its support context and optional
/// cache-first form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "family", rename_all = "snake_case", deny_unknown_fields)]
pub enum SurfaceCurveFamily {
    /// Blend edge curve whose construction details live on its blend support.
    Blend {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tail"
        )]
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
    /// Curve constrained to a support surface.
    SurfaceConstrained {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tail"
        )]
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
    /// Parametric curve on a support surface.
    Parametric {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields with the parametric-only second flag.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_surface_curve_family_tail"
        )]
        tail: Option<SurfaceCurveCacheFirst<ParametricSurfaceCurveFlags>>,
    },
    /// Skin curve on a support surface.
    Skin {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_tail"
        )]
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
}

/// Discriminant of a native surface-curve family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SurfaceCurveFamilyKind {
    /// Blend edge curve whose construction details live on its blend support.
    Blend,
    /// Curve constrained to a support surface.
    SurfaceConstrained,
    /// Parametric curve on a support surface.
    Parametric,
    /// Skin curve on a support surface.
    Skin,
}

impl SurfaceCurveFamily {
    /// Return the family discriminant.
    #[must_use]
    pub const fn kind(&self) -> SurfaceCurveFamilyKind {
        match self {
            Self::Blend { .. } => SurfaceCurveFamilyKind::Blend,
            Self::SurfaceConstrained { .. } => SurfaceCurveFamilyKind::SurfaceConstrained,
            Self::Parametric { .. } => SurfaceCurveFamilyKind::Parametric,
            Self::Skin { .. } => SurfaceCurveFamilyKind::Skin,
        }
    }

    /// Borrow the shared support context.
    #[must_use]
    pub const fn context(&self) -> &IntcurveSupportContext {
        match self {
            Self::Blend { context, .. }
            | Self::SurfaceConstrained { context, .. }
            | Self::Parametric { context, .. }
            | Self::Skin { context, .. } => context,
        }
    }

    fn revision_cache(
        &self,
    ) -> Option<&RevisionCacheForm<CacheFirstCurveParameterization<FiniteReal>>> {
        match self {
            Self::Blend { tail, .. }
            | Self::SurfaceConstrained { tail, .. }
            | Self::Skin { tail, .. } => tail.as_ref().map(|first| &first.form.cache),
            Self::Parametric { tail, .. } => tail.as_ref().map(|first| &first.form.cache),
        }
    }

    /// Write the fit tolerance of the revision-gated solved cache.
    ///
    /// The narrow write route: the borrow of the cache form stays inside this
    /// method, so the family lends no admitted interior for writing.
    fn write_revision_fit_tolerance(
        &mut self,
        value: FitTolerance,
        write: ToleranceWrite,
    ) -> RevisionCacheWrite {
        let form = match self {
            Self::Blend { tail, .. }
            | Self::SurfaceConstrained { tail, .. }
            | Self::Skin { tail, .. } => tail.as_mut().map(|first| &mut first.form.cache),
            Self::Parametric { tail, .. } => tail.as_mut().map(|first| &mut first.form.cache),
        };
        write_revision_form_tolerance(form, value, write)
    }

    /// Return whether two families have the same discriminant and cache-first
    /// tail, excluding the editable support context.
    #[must_use]
    pub fn has_same_form(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Blend { tail: first, .. }, Self::Blend { tail: second, .. })
            | (
                Self::SurfaceConstrained { tail: first, .. },
                Self::SurfaceConstrained { tail: second, .. },
            )
            | (Self::Skin { tail: first, .. }, Self::Skin { tail: second, .. }) => first == second,
            (Self::Parametric { tail: first, .. }, Self::Parametric { tail: second, .. }) => {
                first == second
            }
            _ => false,
        }
    }
}

/// Native silhouette construction family and its exclusive tail fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SilhouetteKind {
    /// Standard implicit silhouette.
    Standard {},
    /// Parametric silhouette.
    Parametric {},
    /// Draft/taper silhouette with an explicit factor.
    Taper {
        /// Native unscaled draft factor.
        draft_factor: crate::scalar::FiniteReal,
    },
}

/// Discriminator-specific payload of a deformable native intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DeformableCurveData<R = f64, V = Vector3, P = Point3> {
    /// Mode 8 vector field followed by ordered scalar pairs.
    VectorField {
        /// Four ordered native vectors.
        vectors: [V; 4],
        /// Ordered pairs from the mode-8 scalar table.
        parameter_pairs: Vec<[R; 2]>,
    },
    /// Mode 3 fixed deformation payload.
    Mode3 {
        /// Four vectors at the start of the payload.
        leading_vectors: [V; 4],
        /// Scalar following the leading vectors.
        leading_parameter: R,
        /// Three flags following the leading scalar.
        leading_flags: [bool; 3],
        /// Position following the leading flags.
        trailing_point: P,
        /// Two vectors following the position.
        trailing_vectors: [V; 2],
        /// Scalar following the trailing frame.
        frame_parameter: R,
        /// Two flags following the frame scalar.
        frame_flags: [bool; 2],
        /// Three ordered scalars following the frame flags.
        parameters: [R; 3],
        /// Five flags following the ordered scalars.
        trailing_flags: [bool; 5],
        /// Final scalar before the trailing integer.
        trailing_parameter: R,
        /// Integer closing the mode-3 payload.
        trailing_value: i64,
    },
}

/// Source slot of a deformable native intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DeformableCurveSource {
    /// Source geometry resolved to a neutral curve carrier.
    Curve {
        /// Curve being deformed.
        curve: CurveId,
    },
    /// Native intcurve reference whose target is absent from the active subtype table.
    NativeReference {
        /// Boolean stored before the reference scope.
        flag: bool,
        /// Integer stored by the native `ref` subtype.
        index: i64,
    },
}

/// Orientation carrier of a planar curve offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "side", rename_all = "snake_case", deny_unknown_fields)]
pub enum OffsetSide<V = Vector3> {
    /// Unit plane normal defining the positive offset side.
    PlaneNormal {
        /// Unit plane normal.
        normal: V,
    },
    /// Explicit offset direction, optionally constrained to a support surface.
    Direction {
        /// Nonzero offset direction.
        direction: V,
        /// Support surface within which the offset is measured.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset_side_support"
        )]
        support: Option<SurfaceId>,
    },
}

/// Parameter interval and optional variable-distance law of a curve offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveOffsetRange<R = f64> {
    /// Constant-distance offset over a retained source interval.
    Uniform {
        /// Parameter interval on the source curve.
        #[serde(deserialize_with = "deserialize_curve_offset_interval")]
        parameter_range: crate::topology::IncreasingParameterInterval,
    },
    /// Variable-distance offset over the interval used by its law.
    Variable {
        /// Parameter interval on the source curve.
        #[serde(deserialize_with = "deserialize_curve_offset_interval")]
        parameter_range: crate::topology::IncreasingParameterInterval,
        /// Variable signed-distance law.
        distance_law: CurveOffsetDistanceLaw<R>,
    },
}

/// The refusal of a curve offset whose distance, side, range or law is
/// invalid.
pub(crate) const INVALID_CURVE_OFFSET: &str =
    "curve offset distance, side, range, or law is invalid";

/// Admit a curve-offset interval with the curve-offset refusal.
fn admit_curve_offset_interval(
    range: [f64; 2],
) -> Result<crate::topology::IncreasingParameterInterval, ProceduralGeometryError> {
    crate::topology::IncreasingParameterInterval::new(range)
        .ok_or(ProceduralGeometryError::Payload(INVALID_CURVE_OFFSET))
}

/// Read a curve-offset interval with the curve-offset refusal.
fn deserialize_curve_offset_interval<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<crate::topology::IncreasingParameterInterval, D::Error> {
    crate::topology::IncreasingParameterInterval::new(<[f64; 2]>::deserialize(deserializer)?)
        .ok_or_else(|| serde::de::Error::custom(INVALID_CURVE_OFFSET))
}

impl CurveOffsetRange {
    /// Admit a constant-distance range over a strictly increasing source
    /// interval. The refusal is the one
    /// [`OffsetCurveConstruction::try_new`](crate::geometry::curve_payloads::OffsetCurveConstruction::try_new)
    /// states for an invalid offset.
    pub fn uniform(parameter_range: [f64; 2]) -> Result<Self, ProceduralGeometryError> {
        Ok(Self::Uniform {
            parameter_range: admit_curve_offset_interval(parameter_range)?,
        })
    }

    /// Admit a variable-distance range over a strictly increasing source
    /// interval, with the refusal of [`CurveOffsetRange::uniform`].
    pub fn variable(
        parameter_range: [f64; 2],
        distance_law: CurveOffsetDistanceLaw,
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self::Variable {
            parameter_range: admit_curve_offset_interval(parameter_range)?,
            distance_law,
        })
    }
}

/// Neutral semantics for a procedural curve.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralCurveDefinition {
    /// An exact native intcurve whose solved NURBS cache is authoritative.
    Exact {
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Curve defined by recursive native law formulas.
    Law {
        /// Shared support surfaces, UV curves, interval, and discontinuities.
        context: IntcurveSupportContext,
        /// Version-stamped serializer form, absent for the legacy layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<LawCurveVersionForm>,
        /// Native ASM extension integer.
        extension: i64,
        /// Primary recursive law formula.
        #[cfg_attr(feature = "schema", schemars(with = "LawFormula"))]
        primary: FiniteLawFormula,
        /// Counted additional recursive law formulas.
        #[cfg_attr(feature = "schema", schemars(with = "Vec<LawFormula>"))]
        additional: Vec<FiniteLawFormula>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Ordered compound of native child curves with construction parameters.
    Compound(CompoundCurveConstruction),
    /// Circular or conical helix around an axis.
    Helix(HelixCurveConstruction),
    /// Intersection of two support surfaces.
    Intersection {
        /// Shared surfaces, UV curves, interval, and discontinuity metadata.
        context: IntcurveSupportContext,
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Tolerance-bounded intersection relation selected by topology endpoints.
    TolerantIntersection {
        /// Distinct supports and finite endpoint bounds.
        construction: TolerantIntersectionConstruction,
        /// Atomic neutral parameterization established by validated support charts.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameterization: Option<TolerantIntersectionParameterization>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Intersection constrained by a third ordered support surface.
    ThreeSurfaceIntersection(curve_payloads::ThreeSurfaceIntersectionCurvePayload),
    /// Surface-related curve whose native subtype has no tail beyond the shared prefix.
    SurfaceCurve {
        /// Native family, support context, and optional cache-first tail.
        family: SurfaceCurveFamily,
    },
    /// Silhouette of a cast surface in a light direction.
    Silhouette(curve_payloads::SilhouetteCurveConstruction),
    /// Curve offset relative to a surface parameterization.
    SurfaceOffset(curve_payloads::SurfaceOffsetCurveConstruction),
    /// Blend spring guide between two support sides.
    Spring(curve_payloads::SpringCurvePayload),
    /// Deformation of an embedded source curve.
    Deformable(curve_payloads::DeformableCurveConstruction),
    /// Projection of a source curve onto a support surface.
    Projection(curve_payloads::ProjectionCurvePayload),
    /// Offset from a source curve.
    Offset(curve_payloads::OffsetCurveConstruction),
    /// Free-space 3D offset using a reference direction.
    SpatialOffset(curve_payloads::SpatialOffsetCurveConstruction),
    /// Intersection of two surfaces after applying independent signed offsets.
    TwoSidedOffset(curve_payloads::TwoSidedOffsetCurveConstruction),
    /// Free-space vector offset of a source curve over a parameter interval.
    VectorOffset(curve_payloads::VectorOffsetCurveConstruction),
    /// A parameter sub-range of a parent curve.
    Subset(curve_payloads::SubsetCurveConstruction),
    /// Affine replica of a curve carrier, retaining the parent curve's
    /// parameter range and parameterization.
    Replica {
        /// Curve being replicated.
        source: CurveId,
        /// Affine map from the parent curve coordinates to this curve.
        transform: Transform,
    },
    /// Spine or center curve of a blend surface.
    BlendSpine {
        /// The blend surface this curve is the spine of, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blend_surface: Option<SurfaceId>,
    },
    /// Preserved construction without a neutral interpretation.
    Unknown {
        /// Source construction-family discriminator, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_kind: Option<String>,
        /// Reference to the preserved raw source record, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
}

#[derive(Deserialize)]
#[serde(
    remote = "ProceduralCurveDefinition",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum ProceduralCurveDefinitionWire {
    Exact {
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    Law {
        context: IntcurveSupportContext,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_version"
        )]
        version: Option<LawCurveVersionForm>,
        extension: i64,
        primary: FiniteLawFormula,
        additional: Vec<FiniteLawFormula>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    Compound(CompoundCurveConstruction),
    Helix(HelixCurveConstruction),
    Intersection {
        context: IntcurveSupportContext,
        discontinuity_flag: bool,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    TolerantIntersection {
        construction: TolerantIntersectionConstruction,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_parameterization"
        )]
        parameterization: Option<TolerantIntersectionParameterization>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    ThreeSurfaceIntersection(curve_payloads::ThreeSurfaceIntersectionCurvePayload),
    SurfaceCurve {
        family: SurfaceCurveFamily,
    },
    Silhouette(curve_payloads::SilhouetteCurveConstruction),
    SurfaceOffset(curve_payloads::SurfaceOffsetCurveConstruction),
    Spring(curve_payloads::SpringCurvePayload),
    Deformable(curve_payloads::DeformableCurveConstruction),
    Projection(curve_payloads::ProjectionCurvePayload),
    Offset(curve_payloads::OffsetCurveConstruction),
    SpatialOffset(curve_payloads::SpatialOffsetCurveConstruction),
    TwoSidedOffset(curve_payloads::TwoSidedOffsetCurveConstruction),
    VectorOffset(curve_payloads::VectorOffsetCurveConstruction),
    Subset(curve_payloads::SubsetCurveConstruction),
    Replica {
        source: CurveId,
        transform: Transform,
    },
    BlendSpine {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_blend_surface"
        )]
        blend_surface: Option<SurfaceId>,
    },
    Unknown {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_native_kind"
        )]
        native_kind: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_record"
        )]
        record: Option<UnknownId>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
}

impl<'de> Deserialize<'de> for ProceduralCurveDefinition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ProceduralCurveDefinitionWire::deserialize(deserializer)
    }
}

/// Codes attached to the two native vector-offset roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VectorOffsetRoles {
    /// Code of the source role.
    pub source: i64,
    /// Code of the offset role.
    pub offset: i64,
}

impl ProceduralCurveDefinition {
    fn revision_cache(
        &self,
    ) -> Option<&RevisionCacheForm<CacheFirstCurveParameterization<FiniteReal>>> {
        match self {
            Self::SurfaceCurve { family } => family.revision_cache(),
            Self::SurfaceOffset(payload) => payload.cache_first().as_ref().map(|form| &form.cache),
            Self::Spring(definition_payload) => {
                let layout = definition_payload.layout();
                layout.cache_first().map(|form| &form.cache)
            }
            Self::Deformable(definition_payload) => {
                let cache_first = definition_payload.cache_first();
                Some(&cache_first.cache)
            }
            _ => None,
        }
    }

    /// Write the fit tolerance of the revision-gated solved cache.
    ///
    /// The narrow write route: the borrow of the cache form stays inside this
    /// method, so no construction lends its admitted interior for writing.
    fn write_revision_fit_tolerance(
        &mut self,
        value: FitTolerance,
        write: ToleranceWrite,
    ) -> RevisionCacheWrite {
        match self {
            Self::SurfaceCurve { family } => family.write_revision_fit_tolerance(value, write),
            Self::SurfaceOffset(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::Spring(payload) => payload.write_revision_fit_tolerance(value, write),
            Self::Deformable(payload) => payload.write_revision_fit_tolerance(value, write),
            _ => RevisionCacheWrite::NoForm,
        }
    }
}

impl ProceduralCurveDefinition {
    /// Whether the construction owns a revision-gated cache form, which then
    /// states its solved-cache fit tolerance.
    #[must_use]
    pub fn owns_revision_cache(&self) -> bool {
        self.revision_cache().is_some()
    }
}

impl ProceduralCurve {
    /// Build a procedural curve from its construction definition.
    pub fn new(id: ProceduralCurveId, definition: ProceduralCurveDefinition) -> Self {
        Self { id, definition }
    }

    /// Borrow the neutral construction definition.
    #[must_use]
    pub fn definition(&self) -> &ProceduralCurveDefinition {
        &self.definition
    }

    /// Replace the construction definition. The cache contract travels with
    /// the definition, so nothing outside it changes.
    pub fn replace_definition(&mut self, definition: ProceduralCurveDefinition) {
        self.definition = definition;
    }

    /// Edit the definition in place.
    pub fn edit_definition<R>(
        &mut self,
        edit: impl FnOnce(&mut ProceduralCurveDefinition) -> R,
    ) -> R {
        edit(&mut self.definition)
    }

    /// Mutable checked support context of an intersection construction.
    pub fn intersection_context_mut(&mut self) -> Option<&mut IntcurveSupportContext> {
        match &mut self.definition {
            ProceduralCurveDefinition::Intersection { context, .. } => Some(context),
            _ => None,
        }
    }

    /// Effective fit tolerance of the solved cache.
    #[must_use]
    pub fn cache_fit_tolerance(&self) -> Option<FitTolerance> {
        self.definition.cache_fit_tolerance()
    }

    /// Change the effective fit tolerance without permitting a parameterized
    /// cache to acquire one or a solved cache to lose it.
    pub fn set_cache_fit_tolerance(
        &mut self,
        value: Option<f64>,
    ) -> Result<(), CacheContractError> {
        let value = value.map(FitTolerance::try_new).transpose()?;
        self.definition.set_cache_fit_tolerance(value)
    }

    /// State that a solved carrier of this construction was fitted to `value`,
    /// raising an existing contract rather than lowering it.
    pub fn require_cache_fit_tolerance(
        &mut self,
        value: FitTolerance,
    ) -> Result<(), CacheContractError> {
        self.definition.require_cache_fit_tolerance(value)
    }

    /// Scale the effective cache-fit tolerance in place.
    ///
    /// A positive scale keeps the tolerance non-negative, so the scaled
    /// tolerance is refused only when it overflows, with the admission's own
    /// error.
    pub fn scale_cache_fit_tolerance(
        &mut self,
        scale: crate::scalar::PositiveReal,
    ) -> Result<(), CacheContractError> {
        if let Some(value) = self.cache_fit_tolerance() {
            let scaled = value
                .scaled(scale)
                .ok_or_else(|| CacheContractError::InvalidValue {
                    value: value.get() * scale.get(),
                })?;
            self.definition.set_cache_fit_tolerance(Some(scaled))?;
        }
        Ok(())
    }
}

/// One procedural-surface row: the construction and the carrier it produces.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(crate) struct ProceduralSurfaceRow {
    /// Stable construction identity.
    id: ProceduralSurfaceId,
    /// Carrier surface this construction produces.
    surface: SurfaceId,
    /// Neutral construction definition.
    definition: ProceduralSurfaceDefinition,
    /// Four optional U/V parameter bounds following the record's subtype scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_record_bounds"
    )]
    record_bounds: Option<RecordBounds>,
}

/// One procedural-curve row: the construction and the carrier it produces.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(crate) struct ProceduralCurveRow {
    /// Stable construction identity.
    id: ProceduralCurveId,
    /// Carrier curve this construction produces.
    curve: CurveId,
    /// Neutral construction definition.
    definition: ProceduralCurveDefinition,
}

impl ProceduralSurfaceRow {
    /// The row a construction and its carrier state.
    pub(crate) fn new(surface: SurfaceId, procedural: &ProceduralSurface) -> Self {
        Self {
            id: procedural.id.clone(),
            surface,
            definition: procedural.definition.clone(),
            record_bounds: procedural.record_bounds,
        }
    }

    /// The carrier and the construction this row states.
    pub(crate) fn into_parts(self) -> (SurfaceId, ProceduralSurface) {
        (
            self.surface,
            ProceduralSurface::new(self.id, self.definition, self.record_bounds),
        )
    }
}

impl ProceduralCurveRow {
    /// The row a construction and its carrier state.
    pub(crate) fn new(curve: CurveId, procedural: &ProceduralCurve) -> Self {
        Self {
            id: procedural.id.clone(),
            curve,
            definition: procedural.definition.clone(),
        }
    }

    /// The carrier and the construction this row states.
    pub(crate) fn into_parts(self) -> (CurveId, ProceduralCurve) {
        (
            self.curve,
            ProceduralCurve {
                id: self.id,
                definition: self.definition,
            },
        )
    }
}

/// Independent variable used by a curve-offset distance law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CurveOffsetLawBasis {
    /// Distance measured along the source curve from the offset interval start.
    ArcLength,
    /// Native source-curve parameter.
    Parameter,
}

/// A one-based coordinate of a curve-offset distance function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "u8", into = "u8")]
pub struct CurveOffsetCoordinate(u8);

impl CurveOffsetCoordinate {
    /// Admit coordinate one, two, or three.
    pub fn try_new(coordinate: u8) -> Result<Self, &'static str> {
        match coordinate {
            1..=3 => Ok(Self(coordinate)),
            _ => Err("curve offset coordinate must be 1, 2, or 3"),
        }
    }

    /// One-based coordinate number.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for CurveOffsetCoordinate {
    type Error = &'static str;
    fn try_from(coordinate: u8) -> Result<Self, Self::Error> {
        Self::try_new(coordinate)
    }
}

impl From<CurveOffsetCoordinate> for u8 {
    fn from(coordinate: CurveOffsetCoordinate) -> Self {
        coordinate.get()
    }
}

/// Variable signed distance law for a planar curve offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveOffsetDistanceLaw<R = f64> {
    /// Linear interpolation between two distance controls.
    Linear {
        /// Independent-variable interpretation.
        basis: CurveOffsetLawBasis,
        /// Ordered signed distances in document length units.
        distances: [R; 2],
        /// Ordered arc-length or neutral carrier-parameter controls.
        #[serde(deserialize_with = "deserialize_curve_offset_interval")]
        control_range: crate::topology::IncreasingParameterInterval,
    },
    /// One coordinate of another curve defines the signed distance.
    Coordinate {
        /// Curve carrying the distance function.
        function: CurveId,
        /// One-based coordinate number on `function`.
        coordinate: CurveOffsetCoordinate,
        /// Independent-variable interpretation.
        basis: CurveOffsetLawBasis,
        /// Function parameter at zero source parameter or arc length.
        function_parameter_offset: R,
        /// Function-parameter change per neutral source parameter or length unit.
        function_parameter_scale: R,
    },
}

impl CurveOffsetDistanceLaw {
    /// Admit a linear law over strictly increasing controls, with the refusal
    /// of [`CurveOffsetRange::uniform`]. The distances are checked by the
    /// offset construction.
    pub fn linear(
        basis: CurveOffsetLawBasis,
        distances: [f64; 2],
        control_range: [f64; 2],
    ) -> Result<Self, ProceduralGeometryError> {
        Ok(Self::Linear {
            basis,
            distances,
            control_range: admit_curve_offset_interval(control_range)?,
        })
    }
}

#[cfg(test)]
mod tests;

impl CompoundCurveConstruction {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    const fn legacy_cache_slot_mut(&mut self) -> &mut Option<LegacyCache> {
        &mut self.cache
    }
}

impl HelixCurveConstruction {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    const fn legacy_cache_slot_mut(&mut self) -> &mut Option<LegacyCache> {
        &mut self.cache
    }
}

impl TSplineSurfaceConstruction {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        match self.cache.legacy_fit_tolerance() {
            Some(fit_tolerance) => Some(LegacyCache { fit_tolerance }),
            None => None,
        }
    }

    /// Mutable legacy solved-cache slot, absent when a revision-gated form
    /// states the tolerance instead.
    fn legacy_cache_slot_mut(&mut self) -> Option<LegacyCacheSlot<'_>> {
        self.cache.legacy_cache_mut().map(LegacyCacheSlot::Optional)
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_tertiary, bool, "tertiary");
cadmpeg_core::named_optional_field!(deserialize_cache, LegacyCache, "cache");
cadmpeg_core::named_optional_field!(deserialize_record, UnknownId, "record");
cadmpeg_core::named_optional_field!(
    deserialize_surface_geometry_cache,
    SolvedSurfaceGeometry,
    "cache"
);
cadmpeg_core::named_optional_field!(
    deserialize_source_object,
    SourceObjectAssociation,
    "source_object"
);
cadmpeg_core::named_optional_field!(
    deserialize_curve_geometry_cache,
    SolvedCurveGeometry,
    "cache"
);
cadmpeg_core::named_optional_field!(deserialize_extra<R>, [R; 2], "extra");
cadmpeg_core::named_optional_field!(deserialize_pcurve, PcurveGeometry, "pcurve");
cadmpeg_core::named_optional_field!(deserialize_direction<V>, V, "direction");
cadmpeg_core::named_optional_field!(deserialize_surface, SurfaceId, "surface");
cadmpeg_core::named_optional_field!(deserialize_asm_extension, i64, "asm_extension");
cadmpeg_core::named_optional_field!(
    deserialize_secondary_pcurve,
    PcurveGeometry,
    "secondary_pcurve"
);
cadmpeg_core::named_optional_field!(deserialize_endpoints<R>, [Option<R>; 2], "endpoints");
cadmpeg_core::named_optional_field!(deserialize_path<R>, LoftPathCurve<R>, "path");
cadmpeg_core::named_optional_field!(deserialize_support, G2BlendFullSupport, "support");
cadmpeg_core::named_optional_field!(deserialize_extension<R>, LoftBridgeToken<R>, "extension");
cadmpeg_core::named_optional_field!(
    deserialize_rolling_ball_side_surface<S, R>,
    RollingBallSupportSurface<S, R>,
    "surface"
);
cadmpeg_core::named_optional_field!(
    deserialize_curve<C, R>,
    RollingBallSupportCurve<C, R>,
    "curve"
);
cadmpeg_core::named_optional_field!(deserialize_rolling_ball_side_pcurve<P>, P, "pcurve");
cadmpeg_core::named_optional_field!(
    deserialize_rolling_ball_side_secondary_pcurve<P>,
    P,
    "secondary_pcurve"
);
cadmpeg_core::named_optional_field!(
    deserialize_rolling_ball_side_extension<P>,
    RollingBallSideExtension<P>,
    "extension"
);
cadmpeg_core::named_optional_field!(
    deserialize_tertiary_pcurve,
    PcurveGeometry,
    "tertiary_pcurve"
);
cadmpeg_core::named_optional_field!(deserialize_third<V>, Box<RollingBallThirdSide<V>>, "third");
cadmpeg_core::named_optional_field!(
    deserialize_radius<R, V, P>,
    Box<VariableBlendValue<R, V, P>>,
    "radius"
);
cadmpeg_core::named_optional_field!(
    deserialize_cross_section<R, V, P>,
    VariableBlendCrossSection<R, V, P>,
    "cross_section"
);
cadmpeg_core::named_optional_field!(deserialize_v_lower<R>, R, "v_lower");
cadmpeg_core::named_optional_field!(
    deserialize_secondary_curve<R>,
    RollingBallSupportCurve<CurveId, R>,
    "secondary_curve"
);
cadmpeg_core::named_optional_field!(deserialize_post_curve, CurveId, "post_curve");
cadmpeg_core::named_optional_field!(deserialize_post_pcurve, PcurveGeometry, "post_pcurve");
cadmpeg_core::named_optional_field!(deserialize_revision, PositiveI64, "revision");
cadmpeg_core::named_optional_field!(
    deserialize_first_scale<R, V>,
    Box<CompoundLoftScale<R, V>>,
    "first_scale"
);
cadmpeg_core::named_optional_field!(
    deserialize_scale<R, V>,
    Box<CompoundLoftScale<R, V>>,
    "scale"
);
cadmpeg_core::named_optional_field!(
    deserialize_parameter_ranges<R>,
    [[R; 2]; 2],
    "parameter_ranges"
);
cadmpeg_core::named_optional_field!(
    deserialize_parameter_range,
    DirectedParameterRange,
    "parameter_range"
);
cadmpeg_core::named_optional_field!(
    deserialize_intcurve_support_side_pcurve,
    SupportPcurve,
    "pcurve"
);
cadmpeg_core::named_optional_field!(deserialize_second_flag, bool, "second_flag");
cadmpeg_core::named_optional_field!(deserialize_tail, SurfaceCurveCacheFirst<bool>, "tail");
cadmpeg_core::named_optional_field!(
    deserialize_surface_curve_family_tail,
    SurfaceCurveCacheFirst<ParametricSurfaceCurveFlags>,
    "tail"
);
cadmpeg_core::named_optional_field!(deserialize_offset_side_support, SurfaceId, "support");
cadmpeg_core::named_optional_field!(deserialize_version, LawCurveVersionForm, "version");
cadmpeg_core::named_optional_field!(
    deserialize_parameterization,
    TolerantIntersectionParameterization,
    "parameterization"
);
cadmpeg_core::named_optional_field!(deserialize_blend_surface, SurfaceId, "blend_surface");
cadmpeg_core::named_optional_field!(deserialize_native_kind, String, "native_kind");
cadmpeg_core::named_optional_field!(deserialize_record_bounds, RecordBounds, "record_bounds");
